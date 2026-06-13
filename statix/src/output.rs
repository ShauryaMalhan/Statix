//! Batched JSON (schema v2), optional HTTP ingest, and raw per-event debug output.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use statix_common::StatixEvent;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Serialize;
use tokio::sync::{mpsc, Mutex};

use crate::aggregator::BatchPayload;
use statix_infra::env::read_env_u64;
use statix_wire::IngestBatch;

pub const SCHEMA_VERSION: u32 = 2;

const RETRY_QUEUE_CAPACITY: usize = 60;

const DEFAULT_BACKOFF_INITIAL_SECS: u64 = 1;
const DEFAULT_BACKOFF_MAX_SECS: u64 = 30;

const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_HTTP_POOL_IDLE_SECS: u64 = 55;

fn read_node_name_for_retry_worker() -> String {
    statix_infra::env::var("STATIX_NODE_NAME")
        .or_else(|| std::env::var("NODE_NAME").ok())
        .unwrap_or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "localhost".into())
        })
}

/// Deterministic node-hash spread over 30s + 0–5s PRNG (V3-15) — avoids post-outage thundering herd.
fn recovery_spread_sleep_secs(node_name: &str) -> f64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_name.hash(&mut hasher);
    let spread_secs = (hasher.finish() % 30_000) as f64 / 1000.0;
    rand::random::<f64>() * 5.0 + spread_secs
}

fn read_http_timeout_secs() -> u64 {
    read_env_u64("STATIX_HTTP_TIMEOUT_SECS", DEFAULT_HTTP_TIMEOUT_SECS)
}

fn read_http_pool_idle_secs() -> u64 {
    read_env_u64("STATIX_HTTP_POOL_IDLE_SECS", DEFAULT_HTTP_POOL_IDLE_SECS)
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static IS_HTTP_INGEST: OnceLock<bool> = OnceLock::new();
static RETRY_TX: OnceLock<mpsc::Sender<bytes::Bytes>> = OnceLock::new();
static RETRY_RX: OnceLock<Arc<Mutex<mpsc::Receiver<bytes::Bytes>>>> = OnceLock::new();

fn is_http_ingest() -> bool {
    *IS_HTTP_INGEST.get_or_init(|| statix_infra::env::var("STATIX_INGEST_URL").is_some())
}

/// Call once at startup when `STATIX_INGEST_URL` may be used (shared connection pool).
pub fn init_http_client() {
    let _ = HTTP_CLIENT.get_or_init(|| {
        let timeout_secs = read_http_timeout_secs();
        let pool_idle_secs = read_http_pool_idle_secs();
        log::info!(
            "HTTP ingest client: timeout={timeout_secs}s, pool_idle={pool_idle_secs}s \
             (STATIX_HTTP_TIMEOUT_SECS / STATIX_HTTP_POOL_IDLE_SECS)"
        );

        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .pool_idle_timeout(Duration::from_secs(pool_idle_secs));

        if let Some(token) = statix_infra::env::var("STATIX_API_TOKEN") {
            if !token.is_empty() {
                let mut headers = HeaderMap::new();
                let bearer = format!("Bearer {token}");
                match HeaderValue::from_str(&bearer) {
                    Ok(value) => {
                        headers.insert(AUTHORIZATION, value);
                        log::info!(
                            "API Token Authentication is configured; Authorization header will be attached to every ingest request"
                        );
                        builder = builder.default_headers(headers);
                    }
                    Err(e) => {
                        log::warn!(
                            "STATIX_API_TOKEN is set but invalid for HTTP headers ({e}); client built without auth"
                        );
                    }
                }
            }
        }

        builder
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    });
}

/// Spawns the background retry worker. Call once when `STATIX_INGEST_URL` is set (after `init_http_client`).
pub fn init_retry_worker(url: String) {
    init_http_client();

    let (tx, rx) = mpsc::channel(RETRY_QUEUE_CAPACITY);
    let _ = RETRY_TX.set(tx);
    let rx = Arc::new(Mutex::new(rx));
    let _ = RETRY_RX.set(Arc::clone(&rx));

    let initial_backoff = read_env_u64("STATIX_BACKOFF_INITIAL_SECS", DEFAULT_BACKOFF_INITIAL_SECS);
    let max_backoff = read_env_u64("STATIX_BACKOFF_MAX_SECS", DEFAULT_BACKOFF_MAX_SECS)
        .max(initial_backoff);

    let node_name = read_node_name_for_retry_worker();

    log::info!(
        "Ingest retry worker: backoff {initial_backoff}s..{max_backoff}s with 30% jitter; \
         recovery spread keyed on node={node_name:?} (STATIX_BACKOFF_* / STATIX_NODE_NAME)"
    );

    tokio::spawn(async move {
        let mut backoff_secs = initial_backoff;
        loop {
            let body = {
                let mut guard = rx.lock().await;
                match guard.recv().await {
                    Some(b) => b,
                    None => break,
                }
            };

            loop {
                match post_ingest(&url, body.clone()).await {
                    PostOutcome::Success => {
                        if backoff_secs > initial_backoff {
                            let sleep_secs = recovery_spread_sleep_secs(&node_name);
                            log::info!(
                                "Gateway recovered after outage; staggering backlog flush {:.2}s (node spread)",
                                sleep_secs
                            );
                            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;
                        }
                        backoff_secs = initial_backoff;
                        break;
                    }
                    PostOutcome::Retryable(reason) => {
                        let jitter = rand::random::<f64>() * (backoff_secs as f64 * 0.3);
                        let sleep_secs = backoff_secs as f64 + jitter;
                        log::warn!(
                            "ingest POST retryable failure: {reason} (sleep {:.2}s, base {backoff_secs}s; \
                             is statix-gateway up? make compose-up; curl http://127.0.0.1:3000/health)",
                            sleep_secs
                        );
                        tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;
                        backoff_secs = (backoff_secs * 2).min(max_backoff);
                    }
                    PostOutcome::NonRetryable(status) => {
                        log::error!(
                            "ingest POST non-retryable status {status}; discarding batch window"
                        );
                        backoff_secs = initial_backoff;
                        break;
                    }
                }
            }
        }
    });
}

enum PostOutcome {
    Success,
    Retryable(String),
    NonRetryable(reqwest::StatusCode),
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

async fn post_ingest(url: &str, body: bytes::Bytes) -> PostOutcome {
    let Some(client) = HTTP_CLIENT.get() else {
        return PostOutcome::Retryable("HTTP client not initialized".into());
    };

    let response = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return PostOutcome::Retryable(e.to_string()),
    };

    let status = response.status();
    if status.is_success() {
        PostOutcome::Success
    } else if is_retryable_status(status) {
        PostOutcome::Retryable(format!("HTTP {status}"))
    } else {
        PostOutcome::NonRetryable(status)
    }
}

fn enqueue_batch_json(json: bytes::Bytes) {
    let Some(tx) = RETRY_TX.get() else {
        log::error!("ingest retry worker not initialized; dropping batch");
        return;
    };

    match tx.try_send(json) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(json)) => {
            let Some(rx_arc) = RETRY_RX.get() else {
                log::error!("ingest retry queue full and receiver missing; dropping batch");
                return;
            };

            // Synchronous drop-oldest — never spawn on the ring-buffer hot path.
            if let Ok(mut rx) = rx_arc.try_lock() {
                if rx.try_recv().is_ok() {
                    log::error!(
                        "SEVERE: ingest retry queue full (>{} windows); dropping oldest batch to avoid OOM",
                        RETRY_QUEUE_CAPACITY
                    );
                }
                if let Err(e) = tx.try_send(json) {
                    log::error!(
                        "SEVERE: ingest retry queue still full after drop; dropping new batch: {e}"
                    );
                }
            } else {
                log::error!("SEVERE: ingest retry queue full and locked; dropping new batch");
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            log::error!("ingest retry worker channel closed; dropping batch");
        }
    }
}

pub fn emit_batch(payload: BatchPayload) {
    let batch = IngestBatch {
        schema_version: SCHEMA_VERSION,
        window_start_ns: payload.window_start_ns,
        window_end_ns: payload.window_end_ns,
        node: payload.node.as_ref().to_string(),
        batch_id: payload.batch_id,
        agent_version: payload.agent_version.to_string(),
        workloads: payload.workloads,
    };

    let json = match serde_json::to_string(&batch) {
        Ok(j) => j,
        Err(e) => {
            log::error!("batch JSON serialisation failed: {e}");
            return;
        }
    };

    if is_http_ingest() {
        enqueue_batch_json(bytes::Bytes::from(json));
    } else {
        println!("{json}");
    }
}

#[derive(Serialize)]
struct RawEventJson<'a> {
    kind: u8,
    pid: u32,
    tgid: u32,
    cpu_id: u32,
    cgroup_id: u64,
    timestamp_ns: u64,
    memory_bytes: u64,
    comm: &'a str,
}

pub fn emit_raw(event: &StatixEvent) {
    let comm = comm_to_str(&event.comm);
    let ev = RawEventJson {
        kind: event.kind,
        pid: event.pid,
        tgid: event.tgid,
        cpu_id: event.cpu_id,
        cgroup_id: event.cgroup_id,
        timestamp_ns: event.timestamp,
        memory_bytes: event.memory_bytes,
        comm,
    };
    if let Ok(json) = serde_json::to_string(&ev) {
        println!("{json}");
    }
}

fn comm_to_str(comm: &[u8; 16]) -> &str {
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);
    std::str::from_utf8(&comm[..end]).unwrap_or("<invalid-utf8>")
}

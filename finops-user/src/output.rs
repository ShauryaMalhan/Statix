//! Batched JSON (schema v2), optional HTTP ingest, and raw per-event debug output.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use finops_common::FinopsEvent;
use serde::Serialize;
use tokio::sync::{mpsc, Mutex};

use crate::aggregator::BatchPayload;

pub const SCHEMA_VERSION: u32 = 2;

const RETRY_QUEUE_CAPACITY: usize = 60;

const DEFAULT_BACKOFF_INITIAL_SECS: u64 = 1;
const DEFAULT_BACKOFF_MAX_SECS: u64 = 30;

const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 5;
const DEFAULT_HTTP_POOL_IDLE_SECS: u64 = 55;

fn read_http_timeout_secs() -> u64 {
    read_env_u64("FINOPS_HTTP_TIMEOUT_SECS", DEFAULT_HTTP_TIMEOUT_SECS)
}

fn read_http_pool_idle_secs() -> u64 {
    read_env_u64("FINOPS_HTTP_POOL_IDLE_SECS", DEFAULT_HTTP_POOL_IDLE_SECS)
}

fn read_env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(s) => match s.parse::<u64>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        Err(_) => default,
    }
}

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static RETRY_TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();
static RETRY_RX: OnceLock<Arc<Mutex<mpsc::Receiver<String>>>> = OnceLock::new();

/// Call once at startup when `FINOPS_INGEST_URL` may be used (shared connection pool).
pub fn init_http_client() {
    let _ = HTTP_CLIENT.get_or_init(|| {
        let timeout_secs = read_http_timeout_secs();
        let pool_idle_secs = read_http_pool_idle_secs();
        log::info!(
            "HTTP ingest client: timeout={timeout_secs}s, pool_idle={pool_idle_secs}s \
             (FINOPS_HTTP_TIMEOUT_SECS / FINOPS_HTTP_POOL_IDLE_SECS)"
        );
        reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .pool_idle_timeout(Duration::from_secs(pool_idle_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    });
}

/// Spawns the background retry worker. Call once when `FINOPS_INGEST_URL` is set (after `init_http_client`).
pub fn init_retry_worker(url: String) {
    init_http_client();

    let (tx, rx) = mpsc::channel(RETRY_QUEUE_CAPACITY);
    let _ = RETRY_TX.set(tx);
    let rx = Arc::new(Mutex::new(rx));
    let _ = RETRY_RX.set(Arc::clone(&rx));

    let initial_backoff = read_env_u64("FINOPS_BACKOFF_INITIAL_SECS", DEFAULT_BACKOFF_INITIAL_SECS);
    let max_backoff = read_env_u64("FINOPS_BACKOFF_MAX_SECS", DEFAULT_BACKOFF_MAX_SECS)
        .max(initial_backoff);

    log::info!(
        "Ingest retry worker: backoff {initial_backoff}s..{max_backoff}s with 30% jitter \
         (FINOPS_BACKOFF_INITIAL_SECS / FINOPS_BACKOFF_MAX_SECS)"
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
                match post_ingest(&url, &body).await {
                    PostOutcome::Success => {
                        backoff_secs = initial_backoff;
                        break;
                    }
                    PostOutcome::Retryable(reason) => {
                        let jitter = rand::random::<f64>() * (backoff_secs as f64 * 0.3);
                        let sleep_secs = backoff_secs as f64 + jitter;
                        log::warn!(
                            "ingest POST retryable failure: {reason} (sleep {:.2}s, base {backoff_secs}s; \
                             is finops-api up? make compose-up; curl http://127.0.0.1:3000/health)",
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

async fn post_ingest(url: &str, body: &str) -> PostOutcome {
    let client = HTTP_CLIENT
        .get()
        .cloned()
        .unwrap_or_else(reqwest::Client::new);

    let response = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
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

fn enqueue_batch_json(json: String) {
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
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut rx = rx_arc.lock().await;
                if rx.try_recv().is_ok() {
                    log::error!(
                        "SEVERE: ingest retry queue full (>{} windows / ~10 min backpressure); \
                         dropping oldest batch to avoid OOM",
                        RETRY_QUEUE_CAPACITY
                    );
                }
                drop(rx);
                if let Err(e) = tx.try_send(json) {
                    log::error!(
                        "SEVERE: ingest retry queue still full after drop-oldest; dropping new batch: {e}"
                    );
                }
            });
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            log::error!("ingest retry worker channel closed; dropping batch");
        }
    }
}

#[derive(Serialize)]
pub struct BatchJson<'a> {
    pub schema_version: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: &'a str,
    pub workloads: &'a [WorkloadBatchRow],
}

#[derive(Clone, Serialize)]
pub struct WorkloadBatchRow {
    pub cgroup_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    pub k8s_resolved: bool,
    pub memory_bytes_max: u64,
    pub memory_bytes_last: u64,
    pub exec_count: u32,
    pub sample_count: u32,
}

pub fn emit_batch(payload: &BatchPayload) {
    let batch = BatchJson {
        schema_version: SCHEMA_VERSION,
        window_start_ns: payload.window_start_ns,
        window_end_ns: payload.window_end_ns,
        node: &payload.node,
        workloads: &payload.workloads,
    };

    let json = match serde_json::to_string(&batch) {
        Ok(j) => j,
        Err(e) => {
            log::error!("batch JSON serialisation failed: {e}");
            return;
        }
    };

    if std::env::var("FINOPS_INGEST_URL").is_ok() {
        enqueue_batch_json(json);
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

pub fn emit_raw(event: &FinopsEvent) {
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

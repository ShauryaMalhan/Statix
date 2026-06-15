use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clickhouse::{Client, Row};
use serde::Serialize;
use statix_infra::env::{read_env_u64, read_env_usize};
use statix_wire::{IngestBatch, WorkloadRow};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const TABLE: &str = "statix.workload_metrics";

const DEFAULT_CHANNEL_SIZE: usize = 8192;
const MIN_CHANNEL_SIZE: usize = 1024;
const DEFAULT_BATCH_MAX: usize = 1024;
const DEFAULT_LINGER_MS: u64 = 50;
const DEFAULT_INSERT_TIMEOUT_SECS: u64 = 3;
const MAX_INSERT_RETRIES: u32 = 5;
const INITIAL_BACKOFF_MS: u64 = 100;
const MAX_BACKOFF_MS: u64 = 2_000;

#[derive(Row, Serialize)]
pub struct MetricRow {
    window_start_ns: u64,
    window_end_ns: u64,
    node: String,
    batch_id: String,
    agent_version: String,
    cgroup_id: u64,
    namespace: Option<String>,
    pod: Option<String>,
    container: Option<String>,
    k8s_resolved: bool,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    exec_count: u32,
    sample_count: u32,
}

impl MetricRow {
    pub fn from_ingest(batch: &IngestBatch, w: &WorkloadRow) -> Self {
        Self {
            window_start_ns: batch.window_start_ns,
            window_end_ns: batch.window_end_ns,
            node: batch.node.clone(),
            batch_id: batch.batch_id.clone(),
            agent_version: batch.agent_version.clone(),
            cgroup_id: w.cgroup_id,
            namespace: w.namespace.clone(),
            pod: w.pod.clone(),
            container: w.container.clone(),
            k8s_resolved: w.k8s_resolved,
            memory_bytes_max: w.memory_bytes_max,
            memory_bytes_last: w.memory_bytes_last,
            exec_count: w.exec_count,
            sample_count: w.sample_count,
        }
    }
}

pub struct ChWriter {
    pub tx: mpsc::Sender<MetricRow>,
    pub channel_capacity: usize,
    pub ch_healthy: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

pub fn ingest_channel_capacity() -> usize {
    read_env_usize("STATIX_INGEST_CHANNEL_SIZE", DEFAULT_CHANNEL_SIZE).max(MIN_CHANNEL_SIZE)
}

fn read_batch_max() -> usize {
    read_env_usize("STATIX_CH_BATCH_MAX", DEFAULT_BATCH_MAX).clamp(64, 16_384)
}

fn read_linger() -> Duration {
    let ms = read_env_u64("STATIX_CH_LINGER_MS", DEFAULT_LINGER_MS).clamp(1, 1000);
    Duration::from_millis(ms)
}

fn read_insert_timeout() -> Duration {
    let secs = read_env_u64("STATIX_CH_INSERT_TIMEOUT_SECS", DEFAULT_INSERT_TIMEOUT_SECS).clamp(1, 30);
    Duration::from_secs(secs)
}

pub fn spawn_writer(client: Client, channel_capacity: usize) -> ChWriter {
    let (tx, mut rx) = mpsc::channel::<MetricRow>(channel_capacity);
    let ch_healthy = Arc::new(AtomicBool::new(false));
    let flag = ch_healthy.clone();
    let task = tokio::spawn(async move {
        let batch_max = read_batch_max();
        let linger = read_linger();
        let ins_to = read_insert_timeout();
        ping_ready(&client, &flag).await;
        loop {
            let Some(batch) = fill_batch(&mut rx, batch_max, linger).await else {
                break;
            };
            flush_with_retry(&client, &flag, batch, ins_to).await;
        }
        drain_final(&mut rx, &client, &flag, batch_max, ins_to).await;
    });
    ChWriter {
        tx,
        channel_capacity,
        ch_healthy,
        task,
    }
}

impl ChWriter {
    pub async fn shutdown(self) {
        drop(self.tx);
        if let Err(e) = self.task.await {
            log::error!("ClickHouse writer task join error: {e}");
        }
    }
}

async fn ping_ready(client: &Client, flag: &AtomicBool) {
    let ok = client.query("SELECT 1").execute().await.is_ok();
    flag.store(ok, Ordering::Release);
    if ok {
        log::info!("ClickHouse ping OK; ingest writer ready");
    } else {
        log::warn!("ClickHouse ping failed; ch_healthy=false until insert succeeds");
    }
}

async fn fill_batch(
    rx: &mut mpsc::Receiver<MetricRow>,
    batch_max: usize,
    linger: Duration,
) -> Option<Vec<MetricRow>> {
    let first = rx.recv().await?;
    let mut batch = Vec::with_capacity(batch_max.min(64));
    batch.push(first);

    let linger_sleep = tokio::time::sleep(linger);
    tokio::pin!(linger_sleep);

    while batch.len() < batch_max {
        let room = batch_max - batch.len();
        tokio::select! {
            biased;
            _ = &mut linger_sleep => break,
            n = rx.recv_many(&mut batch, room) => {
                if n == 0 {
                    break;
                }
            }
        }
    }

    Some(batch)
}

async fn drain_final(
    rx: &mut mpsc::Receiver<MetricRow>,
    client: &Client,
    flag: &Arc<AtomicBool>,
    batch_max: usize,
    ins_to: Duration,
) {
    let mut batch = Vec::with_capacity(batch_max.min(64));
    loop {
        let room = batch_max.saturating_sub(batch.len());
        if room == 0 {
            flush_with_retry(client, flag, batch, ins_to).await;
            batch = Vec::with_capacity(batch_max.min(64));
            continue;
        }
        let n = rx.recv_many(&mut batch, room).await;
        if n == 0 {
            break;
        }
        if batch.len() >= batch_max {
            flush_with_retry(client, flag, batch, ins_to).await;
            batch = Vec::with_capacity(batch_max.min(64));
        }
    }
    if !batch.is_empty() {
        flush_with_retry(client, flag, batch, ins_to).await;
    }
    log::info!("ClickHouse writer drained channel and flushed final batch");
}

async fn flush_batch(
    client: &Client,
    batch: &[MetricRow],
    ins_to: Duration,
) -> Result<(), clickhouse::error::Error> {
    let mut insert = client.insert(TABLE)?;
    for row in batch {
        insert.write(row).await?;
    }
    match tokio::time::timeout(ins_to, insert.end()).await {
        Ok(result) => result,
        Err(_) => Err(clickhouse::error::Error::TimedOut),
    }
}

async fn flush_with_retry(
    client: &Client,
    flag: &Arc<AtomicBool>,
    batch: Vec<MetricRow>,
    ins_to: Duration,
) {
    let row_count = batch.len();
    let mut backoff_ms = INITIAL_BACKOFF_MS;
    let started = std::time::Instant::now();

    for attempt in 0..MAX_INSERT_RETRIES {
        match flush_batch(client, &batch, ins_to).await {
            Ok(()) => {
                flag.store(true, Ordering::Release);
                metrics::histogram!("statix_api_ch_insert_duration_seconds")
                    .record(started.elapsed().as_secs_f64());
                metrics::counter!("statix_api_ch_insert_rows_total").increment(row_count as u64);
                return;
            }
            Err(e) => {
                flag.store(false, Ordering::Release);
                metrics::counter!("statix_api_ch_insert_errors_total").increment(1);
                log::warn!(
                    "ClickHouse insert failed (attempt {}/{MAX_INSERT_RETRIES}, {row_count} rows): {e}",
                    attempt + 1
                );
                if attempt + 1 >= MAX_INSERT_RETRIES {
                    metrics::counter!("statix_api_ch_insert_dropped_total")
                        .increment(row_count as u64);
                    log::error!("ClickHouse insert retry budget exhausted; dropping {row_count} rows");
                    return;
                }
                let jitter = rand_jitter_ms(backoff_ms);
                tokio::time::sleep(Duration::from_millis(backoff_ms + jitter)).await;
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }
    }
}

fn rand_jitter_ms(base_ms: u64) -> u64 {
    let span = base_ms * 30 / 100;
    if span == 0 {
        return 0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
        % (span + 1)
}

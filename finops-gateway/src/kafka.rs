//! Bounded channel + background Kafka producer — HTTP handlers never await Kafka.
//!
//! Micro-batches rows: one `produce()` per partition batch (count or linger), not per message.
//! Records are routed by hashing the `node` key across topic partitions from broker metadata.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use finops_infra::env::{read_env_u64, read_env_usize};

use crate::error::GatewayError;
use rskafka::client::partition::{Compression, UnknownTopicHandling};
use rskafka::client::ClientBuilder;
use rskafka::record::Record;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub const TOPIC: &str = "finops-telemetry";

const DEFAULT_CHANNEL_SIZE: usize = 8192;
const MIN_CHANNEL_SIZE: usize = 1024;
const DEFAULT_BATCH_MAX: usize = 1024;
const DEFAULT_LINGER_MS: u64 = 50;

/// Ingest queue item: Kafka message key (`node`) + JSONEachRow payload.
pub type KafkaQueueItem = (Arc<[u8]>, Vec<u8>);

/// Configured ingest `mpsc` capacity (`FINOPS_KAFKA_CHANNEL_SIZE`, default [`DEFAULT_CHANNEL_SIZE`], min [`MIN_CHANNEL_SIZE`]).
pub fn ingest_channel_capacity() -> usize {
    read_env_usize("FINOPS_KAFKA_CHANNEL_SIZE", DEFAULT_CHANNEL_SIZE).max(MIN_CHANNEL_SIZE)
}

fn read_kafka_batch_max() -> usize {
    read_env_usize("FINOPS_KAFKA_BATCH_MAX", DEFAULT_BATCH_MAX).clamp(64, 16_384)
}

fn read_kafka_linger() -> Duration {
    let ms = read_env_u64("FINOPS_KAFKA_LINGER_MS", DEFAULT_LINGER_MS).clamp(1, 1000);
    Duration::from_millis(ms)
}

/// Called after a successful `try_send` on the ingest channel (depth gauge proxy).
#[inline]
pub fn on_kafka_enqueued() {
    metrics::gauge!("finops_api_kafka_channel_depth").increment(1.0);
}

#[inline]
fn on_kafka_dequeued(count: usize) {
    if count > 0 {
        metrics::gauge!("finops_api_kafka_channel_depth").decrement(count as f64);
    }
}

/// Handle to the background producer. Drop `tx` (via `shutdown`) then await the task.
pub struct KafkaProducer {
    pub tx: mpsc::Sender<KafkaQueueItem>,
    /// Capacity passed to `mpsc::channel` (same as [`ingest_channel_capacity`] at startup).
    pub channel_capacity: usize,
    /// Set `true` after broker connect + partition metadata load (`Ordering::Release`).
    pub is_ready: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl KafkaProducer {
    /// Close the ingest channel and flush remaining rows to Kafka (call after HTTP drain).
    pub async fn shutdown(self) {
        drop(self.tx);
        if let Err(e) = self.task.await {
            log::error!("Kafka producer task join error: {e}");
        }
    }
}

/// Spawn the background producer; returns a cloneable `tx` for ingest handlers.
pub fn spawn_producer(brokers: String) -> KafkaProducer {
    let channel_size = ingest_channel_capacity();
    log::info!(
        "Kafka ingest mpsc capacity={channel_size} (FINOPS_KAFKA_CHANNEL_SIZE, min {MIN_CHANNEL_SIZE})"
    );
    let (tx, rx) = mpsc::channel(channel_size);
    let is_ready = Arc::new(AtomicBool::new(false));

    // When this task ends, `rx` drops → all `tx` clones report `is_closed()` (/health → 503).
    let task_is_ready = Arc::clone(&is_ready);
    let task = tokio::spawn(async move {
        if let Err(e) = run_producer_loop(brokers, rx, task_is_ready).await {
            log::error!("Kafka producer task exited: {e:#}");
        }
    });

    KafkaProducer {
        tx,
        channel_capacity: channel_size,
        is_ready,
        task,
    }
}

fn hash_node_to_slot(node: &[u8], num_partitions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node.hash(&mut hasher);
    (hasher.finish() as usize) % num_partitions
}

fn partition_id_for_node(node: &[u8], partition_ids: &[i32]) -> i32 {
    let slot = hash_node_to_slot(node, partition_ids.len());
    partition_ids[slot]
}

fn bytes_to_record(node: Arc<[u8]>, payload: Vec<u8>, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        key: Some(node.to_vec()),
        value: Some(payload),
        headers: std::collections::BTreeMap::new(),
        timestamp: ts,
    }
}

async fn load_partition_clients(
    client: &rskafka::client::Client,
) -> Result<
    (Vec<i32>, HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>),
    GatewayError,
> {
    let topics = client.list_topics().await?;
    let mut partition_ids: Vec<i32> = topics
        .into_iter()
        .find(|t| t.name == TOPIC)
        .map(|t| t.partitions.into_iter().collect())
        .unwrap_or_else(|| {
            log::warn!(
                "topic {TOPIC} not in broker metadata yet; using partition 0 until auto-create"
            );
            vec![0]
        });

    partition_ids.sort_unstable();
    partition_ids.dedup();

    if partition_ids.is_empty() {
        return Err(GatewayError::NoTopicPartitions { topic: TOPIC });
    }

    let mut clients = HashMap::with_capacity(partition_ids.len());
    for &pid in &partition_ids {
        let pc = Arc::new(
            client
                .partition_client(TOPIC.to_owned(), pid, UnknownTopicHandling::Retry)
                .await?,
        );
        clients.insert(pid, pc);
    }

    Ok((partition_ids, clients))
}

async fn refresh_partition_metadata(
    client: &rskafka::client::Client,
    partition_ids: &mut Vec<i32>,
    clients: &mut HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>,
) {
    match load_partition_clients(client).await {
        Ok((new_ids, new_clients)) => {
            if *partition_ids != new_ids {
                log::info!(
                    "Kafka partition metadata refreshed: {partition_ids:?} -> {new_ids:?}"
                );
            }
            *partition_ids = new_ids;
            *clients = new_clients;
        }
        Err(e) => {
            log::warn!("Kafka metadata refresh failed (using stale): {e}");
        }
    }
}

async fn produce_grouped_batch(
    client: &rskafka::client::Client,
    partition_ids: &mut Vec<i32>,
    clients: &mut HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>,
    batch: &mut Vec<KafkaQueueItem>,
    batch_max: usize,
    by_partition: &mut HashMap<i32, Vec<KafkaQueueItem>>,
) {
    if batch.is_empty() {
        return;
    }

    by_partition.clear();
    for (node, payload) in batch.drain(..) {
        let pid = partition_id_for_node(&node, partition_ids);
        by_partition.entry(pid).or_default().push((node, payload));
    }

    for (pid, mut rows) in by_partition.drain() {
        let Some(partition_client) = clients.get(&pid).cloned() else {
            log::warn!("no partition client for partition {pid}; dropping {} rows", rows.len());
            continue;
        };
        while !rows.is_empty() {
            let chunk_len = rows.len().min(batch_max);
            let chunk: Vec<_> = rows.drain(..chunk_len).collect();
            let n = chunk.len();
            let batch_ts = Utc::now();
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(node, payload)| bytes_to_record(node, payload, batch_ts))
                .collect();
            let produce_started = Instant::now();
            if let Err(e) = partition_client
                .produce(records, Compression::default())
                .await
            {
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}");
                refresh_partition_metadata(client, partition_ids, clients).await;
            }
            metrics::histogram!("finops_api_kafka_produce_duration_seconds")
                .record(produce_started.elapsed().as_secs_f64());
        }
    }
}

async fn fill_batch(
    rx: &mut mpsc::Receiver<KafkaQueueItem>,
    batch: &mut Vec<KafkaQueueItem>,
    first: KafkaQueueItem,
    batch_max: usize,
    linger: Duration,
) {
    batch.clear();
    batch.push(first);

    let linger_sleep = tokio::time::sleep(linger);
    tokio::pin!(linger_sleep);

    while batch.len() < batch_max {
        let room = batch_max - batch.len();
        tokio::select! {
            biased;
            _ = &mut linger_sleep => break,
            n = rx.recv_many(batch, room) => {
                if n == 0 {
                    break;
                }
                on_kafka_dequeued(n);
            }
        }
    }
}

async fn run_producer_loop(
    brokers: String,
    mut rx: mpsc::Receiver<KafkaQueueItem>,
    is_ready: Arc<AtomicBool>,
) -> Result<(), GatewayError> {
    let batch_max = read_kafka_batch_max();
    let linger = read_kafka_linger();

    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let (mut partition_ids, mut clients) = load_partition_clients(&client).await?;

    is_ready.store(true, Ordering::Release);
    log::info!("Kafka producer connected and ready to accept traffic");

    log::info!(
        "Kafka producer ready (topic={TOPIC}, partitions={partition_ids:?}, \
         channel_depth=mpsc, batch_max={batch_max}, linger_ms={})",
        linger.as_millis()
    );

    let mut batch = Vec::with_capacity(batch_max);
    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> =
        HashMap::with_capacity(partition_ids.len());
    let mut metadata_interval = tokio::time::interval(Duration::from_secs(300));
    metadata_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = metadata_interval.tick() => {
                refresh_partition_metadata(&client, &mut partition_ids, &mut clients).await;
            }
            item = rx.recv() => match item {
                Some(first) => {
                    on_kafka_dequeued(1);
                    fill_batch(&mut rx, &mut batch, first, batch_max, linger).await;
                    produce_grouped_batch(
                        &client,
                        &mut partition_ids,
                        &mut clients,
                        &mut batch,
                        batch_max,
                        &mut by_partition,
                    )
                    .await;
                }
                None => {
                    // All senders dropped (graceful shutdown): drain channel into `batch`.
                    while {
                        let room = batch_max - batch.len();
                        let n = rx.recv_many(&mut batch, room).await;
                        if n > 0 {
                            on_kafka_dequeued(n);
                        }
                        n > 0
                    } {
                        if batch.len() >= batch_max {
                            produce_grouped_batch(
                                &client,
                                &mut partition_ids,
                                &mut clients,
                                &mut batch,
                                batch_max,
                                &mut by_partition,
                            )
                            .await;
                        }
                    }
                    produce_grouped_batch(
                        &client,
                        &mut partition_ids,
                        &mut clients,
                        &mut batch,
                        batch_max,
                        &mut by_partition,
                    )
                    .await;
                    log::info!("Kafka producer drained channel and flushed final batch");
                    break;
                }
            },
        }
    }

    Ok(())
}

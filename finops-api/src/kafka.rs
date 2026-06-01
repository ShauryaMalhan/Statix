//! Bounded channel + background Kafka producer — HTTP handlers never await Kafka.
//!
//! Micro-batches rows: one `produce()` per partition batch (count or linger), not per message.
//! Records are routed by hashing the `node` key across topic partitions from broker metadata.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
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
pub type KafkaQueueItem = (Vec<u8>, Vec<u8>);

fn read_env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(s) => match s.parse::<usize>() {
            Ok(v) if v > 0 => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        Err(_) => default,
    }
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

fn read_kafka_channel_size() -> usize {
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
    let channel_size = read_kafka_channel_size();
    log::info!(
        "Kafka ingest mpsc capacity={channel_size} (FINOPS_KAFKA_CHANNEL_SIZE, min {MIN_CHANNEL_SIZE})"
    );
    let (tx, rx) = mpsc::channel(channel_size);

    // When this task ends, `rx` drops → all `tx` clones report `is_closed()` (/health → 503).
    let task = tokio::spawn(async move {
        if let Err(e) = run_producer_loop(brokers, rx).await {
            log::error!("Kafka producer task exited: {e:#}");
        }
    });

    KafkaProducer { tx, task }
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

fn bytes_to_record(node: Vec<u8>, payload: Vec<u8>) -> Record {
    Record {
        key: Some(node),
        value: Some(payload),
        headers: std::collections::BTreeMap::new(),
        timestamp: Utc::now(),
    }
}

async fn load_partition_clients(
    client: &rskafka::client::Client,
) -> anyhow::Result<(Vec<i32>, HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>)> {
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
        anyhow::bail!("topic {TOPIC} has no partitions in metadata");
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

async fn produce_grouped_batch(
    partition_ids: &[i32],
    clients: &HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>,
    batch: &mut Vec<KafkaQueueItem>,
    batch_max: usize,
) {
    if batch.is_empty() {
        return;
    }

    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> = HashMap::new();
    for (node, payload) in batch.drain(..) {
        let pid = partition_id_for_node(node.as_slice(), partition_ids);
        by_partition.entry(pid).or_default().push((node, payload));
    }

    for (pid, mut rows) in by_partition {
        let Some(client) = clients.get(&pid) else {
            log::warn!("no partition client for partition {pid}; dropping {} rows", rows.len());
            continue;
        };
        while !rows.is_empty() {
            let chunk_len = rows.len().min(batch_max);
            let chunk: Vec<_> = rows.drain(..chunk_len).collect();
            let n = chunk.len();
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(node, payload)| bytes_to_record(node, payload))
                .collect();
            let produce_started = Instant::now();
            if let Err(e) = client.produce(records, Compression::default()).await {
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}");
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
) -> anyhow::Result<()> {
    let batch_max = read_kafka_batch_max();
    let linger = read_kafka_linger();

    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let (partition_ids, clients) = load_partition_clients(&client).await?;

    log::info!(
        "Kafka producer ready (topic={TOPIC}, partitions={partition_ids:?}, \
         channel_depth=mpsc, batch_max={batch_max}, linger_ms={})",
        linger.as_millis()
    );

    let mut batch = Vec::with_capacity(batch_max);

    loop {
        match rx.recv().await {
            Some(first) => {
                on_kafka_dequeued(1);
                fill_batch(&mut rx, &mut batch, first, batch_max, linger).await;
                produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max).await;
            }
            None => {
                // All senders dropped (graceful shutdown): drain channel into `batch` (no scratch vec).
                while {
                    let room = batch_max - batch.len();
                    let n = rx.recv_many(&mut batch, room).await;
                    if n > 0 {
                        on_kafka_dequeued(n);
                    }
                    n > 0
                } {
                    if batch.len() >= batch_max {
                        produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max)
                            .await;
                    }
                }
                produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max).await;
                log::info!("Kafka producer drained channel and flushed final batch");
                break;
            }
        }
    }

    Ok(())
}

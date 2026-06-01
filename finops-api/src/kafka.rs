//! Bounded channel + background Kafka producer — HTTP handlers never await Kafka.
//!
//! Micro-batches rows: one `produce()` per partition batch (count or linger), not per message.
//! Records are routed by hashing the `node` key across topic partitions from broker metadata.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use chrono::Utc;
use rskafka::client::partition::{Compression, UnknownTopicHandling};
use rskafka::client::ClientBuilder;
use rskafka::record::Record;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub const CHANNEL_SIZE: usize = 1024;
pub const TOPIC: &str = "finops-telemetry";

/// Max rows per Kafka produce request (avoids one broker round-trip per row).
const BATCH_MAX_RECORDS: usize = 256;
/// Flush partial batch after this wait so low-volume windows still ship promptly.
const BATCH_LINGER: Duration = Duration::from_millis(5);

/// Ingest queue item: Kafka message key (`node`) + JSONEachRow payload.
pub type KafkaQueueItem = (String, Bytes);

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
    let (tx, rx) = mpsc::channel(CHANNEL_SIZE);

    // When this task ends, `rx` drops → all `tx` clones report `is_closed()` (/health → 503).
    let task = tokio::spawn(async move {
        if let Err(e) = run_producer_loop(brokers, rx).await {
            log::error!("Kafka producer task exited: {e:#}");
        }
    });

    KafkaProducer { tx, task }
}

fn hash_node_to_slot(node: &str, num_partitions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node.hash(&mut hasher);
    (hasher.finish() as usize) % num_partitions
}

fn partition_id_for_node(node: &str, partition_ids: &[i32]) -> i32 {
    let slot = hash_node_to_slot(node, partition_ids.len());
    partition_ids[slot]
}

fn bytes_to_record(node: &str, payload: Bytes) -> Record {
    Record {
        key: Some(node.as_bytes().to_vec()),
        value: Some(payload.to_vec()),
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
) {
    if batch.is_empty() {
        return;
    }

    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> = HashMap::new();
    for (node, payload) in batch.drain(..) {
        let pid = partition_id_for_node(&node, partition_ids);
        by_partition.entry(pid).or_default().push((node, payload));
    }

    for (pid, mut rows) in by_partition {
        let Some(client) = clients.get(&pid) else {
            log::warn!("no partition client for partition {pid}; dropping {} rows", rows.len());
            continue;
        };
        while !rows.is_empty() {
            let chunk_len = rows.len().min(BATCH_MAX_RECORDS);
            let chunk: Vec<_> = rows.drain(..chunk_len).collect();
            let n = chunk.len();
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(node, payload)| bytes_to_record(&node, payload))
                .collect();
            if let Err(e) = client.produce(records, Compression::default()).await {
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}");
            }
        }
    }
}

async fn fill_batch(
    rx: &mut mpsc::Receiver<KafkaQueueItem>,
    batch: &mut Vec<KafkaQueueItem>,
    first: KafkaQueueItem,
) {
    batch.clear();
    batch.push(first);

    let linger = tokio::time::sleep(BATCH_LINGER);
    tokio::pin!(linger);

    while batch.len() < BATCH_MAX_RECORDS {
        let room = BATCH_MAX_RECORDS - batch.len();
        tokio::select! {
            biased;
            _ = &mut linger => break,
            n = rx.recv_many(batch, room) => {
                if n == 0 {
                    break;
                }
            }
        }
    }
}

async fn run_producer_loop(
    brokers: String,
    mut rx: mpsc::Receiver<KafkaQueueItem>,
) -> anyhow::Result<()> {
    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let (partition_ids, clients) = load_partition_clients(&client).await?;

    log::info!(
        "Kafka producer ready (topic={TOPIC}, partitions={:?}, batch_max={BATCH_MAX_RECORDS}, linger_ms={})",
        partition_ids,
        BATCH_LINGER.as_millis()
    );

    let mut batch = Vec::with_capacity(BATCH_MAX_RECORDS);

    loop {
        match rx.recv().await {
            Some(first) => {
                fill_batch(&mut rx, &mut batch, first).await;
                produce_grouped_batch(&partition_ids, &clients, &mut batch).await;
            }
            None => {
                // All senders dropped (graceful shutdown): drain channel into `batch` (no scratch vec).
                while {
                    let room = BATCH_MAX_RECORDS - batch.len();
                    rx.recv_many(&mut batch, room).await > 0
                } {
                    if batch.len() >= BATCH_MAX_RECORDS {
                        produce_grouped_batch(&partition_ids, &clients, &mut batch).await;
                    }
                }
                produce_grouped_batch(&partition_ids, &clients, &mut batch).await;
                log::info!("Kafka producer drained channel and flushed final batch");
                break;
            }
        }
    }

    Ok(())
}

//! Bounded channel + background Kafka producer — HTTP handlers never await Kafka.
//!
//! Micro-batches rows: one `produce()` per batch (count or linger), not per message.

use std::collections::BTreeMap;
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

/// Handle to the background producer. Drop `tx` (via `shutdown`) then await the task.
pub struct KafkaProducer {
    pub tx: mpsc::Sender<Bytes>,
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

    let task = tokio::spawn(async move {
        if let Err(e) = run_producer_loop(brokers, rx).await {
            log::error!("Kafka producer task exited: {e:#}");
        }
    });

    KafkaProducer { tx, task }
}

fn bytes_to_record(payload: Bytes) -> Record {
    Record {
        key: None,
        value: Some(payload.to_vec()),
        headers: BTreeMap::new(),
        timestamp: Utc::now(),
    }
}

async fn produce_batch(
    partition_client: &Arc<rskafka::client::partition::PartitionClient>,
    payloads: &mut Vec<Bytes>,
) {
    if payloads.is_empty() {
        return;
    }
    let n = payloads.len();
    let batch: Vec<Record> = payloads.drain(..).map(bytes_to_record).collect();
    if let Err(e) = partition_client
        .produce(batch, Compression::default())
        .await
    {
        log::warn!("Kafka produce failed ({n} records): {e}");
    }
}

async fn fill_batch(rx: &mut mpsc::Receiver<Bytes>, payloads: &mut Vec<Bytes>, first: Bytes) {
    payloads.clear();
    payloads.push(first);

    let linger = tokio::time::sleep(BATCH_LINGER);
    tokio::pin!(linger);

    while payloads.len() < BATCH_MAX_RECORDS {
        let room = BATCH_MAX_RECORDS - payloads.len();
        tokio::select! {
            biased;
            _ = &mut linger => break,
            n = rx.recv_many(payloads, room) => {
                if n == 0 {
                    break;
                }
            }
        }
    }
}

async fn run_producer_loop(
    brokers: String,
    mut rx: mpsc::Receiver<Bytes>,
) -> anyhow::Result<()> {
    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let partition_client = Arc::new(
        client
            .partition_client(TOPIC.to_owned(), 0, UnknownTopicHandling::Retry)
            .await?,
    );

    log::info!(
        "Kafka producer ready (topic={TOPIC}, partition=0, batch_max={BATCH_MAX_RECORDS}, linger_ms={})",
        BATCH_LINGER.as_millis()
    );

    let mut payloads = Vec::with_capacity(BATCH_MAX_RECORDS);

    loop {
        match rx.recv().await {
            Some(first) => {
                fill_batch(&mut rx, &mut payloads, first).await;
                produce_batch(&partition_client, &mut payloads).await;
            }
            None => {
                // All senders dropped (graceful shutdown): drain channel into `payloads` (no scratch vec).
                while {
                    let room = BATCH_MAX_RECORDS - payloads.len();
                    rx.recv_many(&mut payloads, room).await > 0
                } {
                    if payloads.len() >= BATCH_MAX_RECORDS {
                        produce_batch(&partition_client, &mut payloads).await;
                    }
                }
                produce_batch(&partition_client, &mut payloads).await;
                log::info!("Kafka producer drained channel and flushed final batch");
                break;
            }
        }
    }

    Ok(())
}

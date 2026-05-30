//! Bounded channel + background Kafka producer — HTTP handlers never await Kafka.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use rskafka::client::partition::{Compression, UnknownTopicHandling};
use rskafka::client::ClientBuilder;
use rskafka::record::Record;
use tokio::sync::mpsc;

pub const CHANNEL_SIZE: usize = 1024;
pub const TOPIC: &str = "finops-telemetry";

/// Non-blocking sender for the ingest handler; Kafka I/O runs in a spawned task.
pub fn build_producer(brokers: String) -> mpsc::Sender<Bytes> {
    let (tx, mut rx) = mpsc::channel(CHANNEL_SIZE);

    tokio::spawn(async move {
        if let Err(e) = run_producer_loop(brokers, &mut rx).await {
            log::error!("Kafka producer task exited: {e:#}");
        }
    });

    tx
}

async fn run_producer_loop(brokers: String, rx: &mut mpsc::Receiver<Bytes>) -> anyhow::Result<()> {
    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let partition_client = Arc::new(
        client
            .partition_client(
                TOPIC.to_owned(),
                0,
                UnknownTopicHandling::Retry,
            )
            .await?,
    );

    log::info!("Kafka producer ready (topic={TOPIC}, partition=0)");

    while let Some(payload) = rx.recv().await {
        let record = Record {
            key: None,
            value: Some(payload.to_vec()),
            headers: BTreeMap::new(),
            timestamp: Utc::now(),
        };
        if let Err(e) = partition_client
            .produce(vec![record], Compression::default())
            .await
        {
            log::warn!("Kafka produce failed: {e}");
        }
    }

    Ok(())
}

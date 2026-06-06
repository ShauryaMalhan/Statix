//! Typed gateway errors for startup, Kafka producer setup, and HTTP read-path failures.

use axum::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("Prometheus metrics recorder install failed: {0}")]
    PrometheusInstall(String),

    #[error("TCP bind or HTTP serve failed: {0}")]
    Http(#[from] std::io::Error),

    #[error("Kafka client error: {0}")]
    Kafka(#[from] rskafka::client::error::Error),

    #[error("Kafka topic {topic} has no partitions in broker metadata")]
    NoTopicPartitions { topic: &'static str },

    #[error("Kafka producer task join failed: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),

    #[error("Kafka producer drain timed out after {secs}s")]
    DrainTimeout { secs: u64 },

    #[error("ClickHouse query failed: {0}")]
    ClickHouse(String),
}

impl GatewayError {
    /// Map to an HTTP status for Axum handlers (startup errors are logged, not returned).
    pub fn status_code(&self) -> StatusCode {
        match self {
            GatewayError::ClickHouse(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

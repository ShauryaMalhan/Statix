//! Typed gateway errors for startup, ClickHouse writer drain, and HTTP read-path failures.

use axum::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("Prometheus metrics recorder install failed: {0}")]
    PrometheusInstall(String),

    #[error("TCP bind or HTTP serve failed: {0}")]
    Http(#[from] std::io::Error),

    #[error("ClickHouse writer task join failed: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),

    #[error("ClickHouse writer drain timed out after {secs}s")]
    DrainTimeout { secs: u64 },

    #[error("ClickHouse query failed: {0}")]
    ClickHouse(String),
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

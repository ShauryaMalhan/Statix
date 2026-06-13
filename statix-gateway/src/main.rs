//! statix-gateway — ingest gateway: POST /ingest → mpsc → Kafka; read-path → ClickHouse.
//! Phases 3–4 + 6 shipped; Target 3: GET `/api/v1/workloads/summary`.

mod config;
mod error;
mod kafka;
mod routes;

use error::GatewayError;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::sync::mpsc;

/// Ingest mpsc considered under backpressure when more than this percent of slots are in use.
const READY_CHANNEL_FULL_THRESHOLD_PCT: u8 = 80;

#[derive(Clone)]
pub struct AppState {
    pub kafka_tx: mpsc::Sender<kafka::KafkaQueueItem>,
    /// Configured `mpsc` capacity (`STATIX_KAFKA_CHANNEL_SIZE` at startup).
    pub kafka_channel_capacity: usize,
    /// `true` after Kafka broker connect + partition metadata load.
    pub kafka_ready: Arc<AtomicBool>,
    /// When set, full `Authorization` header value (`Bearer <token>`) for `POST /ingest`.
    pub expected_bearer: Option<String>,
    pub ch_client: clickhouse::Client,
}

#[tokio::main]
async fn main() -> Result<(), GatewayError> {
    let config = config::Config::from_env();
    env_logger::init();

    match &config.api_token {
        Some(_) => log::info!("API Token authentication: ENABLED (STATIX_API_TOKEN)"),
        None => log::warn!(
            "API Token authentication: DISABLED — set STATIX_API_TOKEN before production"
        ),
    }
    log::info!("ClickHouse read-path: {}", config.clickhouse_url);

    // Global recorder for `metrics::counter!` / `histogram!` / `gauge!`; Axum serves `/metrics`.
    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| GatewayError::PrometheusInstall(e.to_string()))?;

    spawn_prometheus_upkeep(prometheus_handle.clone());

    let ch_client = config.clickhouse_client();
    let producer = kafka::spawn_producer(config.kafka_brokers.clone());
    let kafka_channel_capacity = producer.channel_capacity;
    log::info!(
        "Ingest readiness: /ready fails when mpsc > {READY_CHANNEL_FULL_THRESHOLD_PCT}% full (capacity={kafka_channel_capacity})"
    );
    let state = AppState {
        kafka_tx: producer.tx.clone(),
        kafka_channel_capacity,
        kafka_ready: producer.is_ready.clone(),
        expected_bearer: config.expected_bearer().map(str::to_string),
        ch_client,
    };

    let metrics_handle = prometheus_handle.clone();
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(move || metrics_endpoint(metrics_handle.clone())))
        .route(
            "/ingest",
            post(routes::ingest::handler).layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        .route(
            "/api/v1/workloads/summary",
            get(routes::query::workloads_summary),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.api_port));
    log::info!(
        "statix-gateway: http://{addr} — /health, /ready, /ingest, /api/v1/workloads/summary; brokers={}",
        config.kafka_brokers
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log::info!("HTTP server stopped; draining Kafka producer");
    match tokio::time::timeout(Duration::from_secs(10), producer.shutdown()).await {
        Ok(_) => log::info!("Kafka producer drained successfully"),
        Err(_) => {
            let err = GatewayError::DrainTimeout { secs: 10 };
            log::error!("{err}");
        }
    }

    log::info!("statix-gateway shutdown complete");

    Ok(())
}

fn spawn_prometheus_upkeep(handle: PrometheusHandle) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            handle.run_upkeep();
        }
    });
}

async fn metrics_endpoint(handle: PrometheusHandle) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        handle.render(),
    )
}

/// Liveness: HTTP server up and producer task has not dropped the ingest channel.
async fn health_check(State(state): State<AppState>) -> StatusCode {
    if state.kafka_tx.is_closed() {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    }
}

/// Readiness: Kafka connected, ingest channel open, and mpsc not under backpressure (&gt;80% full).
async fn readiness_check(State(state): State<AppState>) -> StatusCode {
    if state.kafka_tx.is_closed() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    if !state.kafka_ready.load(Ordering::Acquire) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    let remaining = state.kafka_tx.capacity();
    let total = state.kafka_channel_capacity;
    if ingest_channel_over_threshold(remaining, total, READY_CHANNEL_FULL_THRESHOLD_PCT) {
        let used = total.saturating_sub(remaining);
        let used_pct = if total > 0 {
            (used * 100) / total
        } else {
            0
        };
        log::warn!(
            "Ingest mpsc backpressure: channel {used_pct}% full ({used}/{total} slots used, {remaining} remaining); /ready -> 503"
        );
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    StatusCode::OK
}

/// `true` when fewer than `(100 - threshold_pct)%` of `total` slots remain (channel over threshold full).
fn ingest_channel_over_threshold(remaining: usize, total: usize, threshold_pct: u8) -> bool {
    if total == 0 {
        return false;
    }
    let free_pct = remaining.saturating_mul(100) / total;
    free_pct < 100usize.saturating_sub(threshold_pct as usize)
}

#[cfg(test)]
mod readiness_tests {
    use super::ingest_channel_over_threshold;

    #[test]
    fn over_threshold_when_more_than_80_percent_full() {
        assert!(ingest_channel_over_threshold(1_000, 8_192, 80));
    }

    #[test]
    fn under_threshold_when_half_full() {
        assert!(!ingest_channel_over_threshold(4_096, 8_192, 80));
    }
}

/// SIGINT (local) and SIGTERM (ECS/K8s deploy) — stop accept, then drain in-flight ingest.
async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_ok() {
            log::info!("SIGINT received");
        }
    };

    #[cfg(unix)]
    let sigterm = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
                log::info!("SIGTERM received");
            }
            Err(e) => log::warn!("SIGTERM handler not installed: {e}"),
        }
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = sigterm => {},
    }
}

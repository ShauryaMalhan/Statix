//! statix-gateway — ingest gateway: POST /ingest → mpsc → ClickHouse; read-path → ClickHouse.
//! Phases 3–4 + 6 shipped; Target 3: GET `/api/v1/workloads/summary`.

mod clickhouse_writer;
mod config;
mod error;
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

const READY_CHANNEL_FULL_THRESHOLD_PCT: u8 = 80;

#[derive(Clone)]
pub struct AppState {
    pub ingest_tx: mpsc::Sender<clickhouse_writer::MetricRow>,
    pub ingest_channel_capacity: usize,
    pub ch_healthy: Arc<AtomicBool>,
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

    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| GatewayError::PrometheusInstall(e.to_string()))?;

    spawn_prometheus_upkeep(prometheus_handle.clone());

    let ch_client = config.clickhouse_client();
    let writer = clickhouse_writer::spawn_writer(
        ch_client.clone(),
        clickhouse_writer::ingest_channel_capacity(),
    );
    let ingest_channel_capacity = writer.channel_capacity;
    log::info!(
        "Ingest readiness: /ready fails when mpsc > {READY_CHANNEL_FULL_THRESHOLD_PCT}% full (capacity={ingest_channel_capacity})"
    );
    let state = AppState {
        ingest_tx: writer.tx.clone(),
        ingest_channel_capacity,
        ch_healthy: writer.ch_healthy.clone(),
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
        "statix-gateway: http://{addr} — /health, /ready, /ingest, /api/v1/workloads/summary; clickhouse={}",
        config.clickhouse_url
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log::info!("HTTP server stopped; draining ClickHouse writer");
    match tokio::time::timeout(Duration::from_secs(10), writer.shutdown()).await {
        Ok(_) => log::info!("ClickHouse writer drained successfully"),
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

async fn health_check(State(state): State<AppState>) -> StatusCode {
    if state.ingest_tx.is_closed() {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    }
}

async fn readiness_check(State(state): State<AppState>) -> StatusCode {
    if state.ingest_tx.is_closed() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    if !state.ch_healthy.load(Ordering::Acquire) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    let remaining = state.ingest_tx.capacity();
    let total = state.ingest_channel_capacity;
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

//! finops-api — ingest gateway: POST /ingest → mpsc → Kafka (non-blocking handler).
//! Phases 3–4 + 6 shipped; Phase 5 adds TLS/auth on `/ingest` (`FINOPS_API_TOKEN`).

mod kafka;
mod routes;

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub kafka_tx: mpsc::Sender<kafka::KafkaQueueItem>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Global recorder for `metrics::counter!` / `histogram!` / `gauge!`; Axum serves `/metrics`.
    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("failed to install Prometheus metrics recorder: {e}"))?;

    spawn_prometheus_upkeep(prometheus_handle.clone());

    let brokers =
        std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".to_string());
    let port: u16 = std::env::var("FINOPS_API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let producer = kafka::spawn_producer(brokers.clone());
    let state = AppState {
        kafka_tx: producer.tx.clone(),
    };

    let metrics_handle = prometheus_handle.clone();
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(move || metrics_endpoint(metrics_handle.clone())))
        .route("/ingest", post(routes::ingest::handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!(
        "finops-api (Phase 5): http://{addr}/ingest — brokers={brokers}; /metrics ready; \
         production requires FINOPS_API_TOKEN on /ingest (planned)"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log::info!("HTTP server stopped; draining Kafka producer");
    match tokio::time::timeout(Duration::from_secs(10), producer.shutdown()).await {
        Ok(_) => log::info!("Kafka producer drained successfully"),
        Err(_) => log::error!("Kafka drain timed out — abandoning in-flight messages"),
    }

    log::info!("finops-api shutdown complete");

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

/// Readiness: if the background producer task exited, `rx` was dropped and `tx.is_closed()`.
async fn health_check(State(state): State<AppState>) -> StatusCode {
    if state.kafka_tx.is_closed() {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
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

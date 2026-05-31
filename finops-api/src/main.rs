//! finops-api — Phase 3 ingest: POST /ingest → mpsc → Kafka (non-blocking handler).

mod kafka;
mod routes;

use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct AppState {
    pub kafka_tx: mpsc::Sender<bytes::Bytes>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

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

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ingest", post(routes::ingest::handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!(
        "finops-api listening on http://{addr}/ingest (brokers={brokers})"
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

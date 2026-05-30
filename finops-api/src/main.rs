//! finops-api — Phase 3 ingest: POST /ingest → mpsc → Kafka (non-blocking handler).

mod kafka;
mod routes;

use std::net::SocketAddr;

use axum::routing::post;
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

    let kafka_tx = kafka::build_producer(brokers.clone());
    let state = AppState { kafka_tx };

    let app = Router::new()
        .route("/ingest", post(routes::ingest::handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log::info!(
        "finops-api listening on http://{addr}/ingest (brokers={brokers})"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

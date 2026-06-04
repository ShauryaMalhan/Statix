//! POST /ingest — denormalize batch envelope to one JSONEachRow message per workload.

use std::time::Instant;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use finops_wire::{FlatRow, IngestBatch};
use tokio::sync::mpsc;

use crate::kafka;
use crate::AppState;

pub async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<IngestBatch>,
) -> Response {
    let started = Instant::now();

    if let Some(expected_bearer) = state.expected_bearer.as_ref() {
        let authorized = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some(expected_bearer.as_str());

        if !authorized {
            log::warn!("Rejected /ingest: missing or invalid Authorization bearer token");
            let response = StatusCode::UNAUTHORIZED.into_response();
            record_ingest_metrics(response.status(), started.elapsed());
            return response;
        }
    }

    let response = ingest_inner(state, batch).await;
    record_ingest_metrics(response.status(), started.elapsed());
    response
}

/// Inclusive schema versions accepted during rolling agent/gateway upgrades (N and N+1).
const MIN_SCHEMA_VERSION: u32 = 2;
const MAX_SCHEMA_VERSION: u32 = 3;

async fn ingest_inner(state: AppState, batch: IngestBatch) -> Response {
    if batch.schema_version < MIN_SCHEMA_VERSION || batch.schema_version > MAX_SCHEMA_VERSION {
        log::warn!(
            "Rejected batch with unsupported schema_version={} (accepted {MIN_SCHEMA_VERSION}..={MAX_SCHEMA_VERSION})",
            batch.schema_version
        );
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Unsupported schema_version={}. Expected 2 or 3.",
                batch.schema_version
            ),
        )
            .into_response();
    }

    let node_vec = batch.node.as_bytes().to_vec();

    for row in &batch.workloads {
        let flat = FlatRow::from_ingest(&batch, row);

        let bytes = match serde_json::to_vec(&flat) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("flat row JSON encode failed: {e}");
                continue;
            }
        };

        match state.kafka_tx.try_send((node_vec.clone(), bytes)) {
            Ok(()) => kafka::on_kafka_enqueued(),
            Err(mpsc::error::TrySendError::Full(_)) => {
                metrics::counter!("finops_api_kafka_channel_full_total").increment(1);
                log::warn!("Kafka channel full (backpressure), rejecting batch");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Ingest channel full. Broker backpressure active.",
                )
                    .into_response();
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                log::warn!("Kafka channel closed, rejecting batch");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Ingest channel closed. Producer unavailable.",
                )
                    .into_response();
            }
        }
    }

    StatusCode::OK.into_response()
}

fn record_ingest_metrics(status: StatusCode, elapsed: std::time::Duration) {
    let status_label = status.as_u16().to_string();
    metrics::counter!("finops_api_ingest_requests_total", "status" => status_label).increment(1);
    metrics::histogram!("finops_api_ingest_duration_seconds").record(elapsed.as_secs_f64());
}

//! POST /ingest — denormalize batch envelope to one JSONEachRow message per workload.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use finops_wire::IngestBatch;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::kafka;
use crate::AppState;

/// Zero-copy serialization view of a denormalized Kafka/ClickHouse row (schema v2).
#[derive(Serialize)]
struct FlatRowRef<'a> {
    window_start_ns: u64,
    window_end_ns: u64,
    node: &'a str,
    batch_id: &'a str,
    agent_version: &'a str,
    cgroup_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pod: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: &'a Option<String>,
    k8s_resolved: bool,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    exec_count: u32,
    sample_count: u32,
}

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
    let batch_window_end_ns = batch.window_end_ns;

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

    let required_slots = batch.workloads.len();
    let available_slots = state.kafka_tx.capacity();
    if available_slots < required_slots {
        metrics::counter!("finops_api_kafka_channel_full_total").increment(1);
        log::warn!(
            "Kafka channel has insufficient capacity ({available_slots}/{required_slots} needed); rejecting entire batch"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Ingest channel capacity insufficient for batch. Retry later.",
        )
            .into_response();
    }

    let node_key: Arc<[u8]> = Arc::from(batch.node.as_bytes());

    for row in &batch.workloads {
        let flat = FlatRowRef {
            window_start_ns: batch.window_start_ns,
            window_end_ns: batch.window_end_ns,
            node: &batch.node,
            batch_id: &batch.batch_id,
            agent_version: &batch.agent_version,
            cgroup_id: row.cgroup_id,
            namespace: &row.namespace,
            pod: &row.pod,
            container: &row.container,
            k8s_resolved: row.k8s_resolved,
            memory_bytes_max: row.memory_bytes_max,
            memory_bytes_last: row.memory_bytes_last,
            exec_count: row.exec_count,
            sample_count: row.sample_count,
        };

        let bytes = match serde_json::to_vec(&flat) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("flat row JSON encode failed: {e}");
                continue;
            }
        };

        match state.kafka_tx.try_send((Arc::clone(&node_key), bytes)) {
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

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let lag_secs = now_ns.saturating_sub(batch_window_end_ns) as f64 / 1_000_000_000.0;
    metrics::histogram!("finops_api_ingest_lag_seconds").record(lag_secs);

    StatusCode::OK.into_response()
}

fn record_ingest_metrics(status: StatusCode, elapsed: std::time::Duration) {
    let status_label = status.as_u16().to_string();
    metrics::counter!("finops_api_ingest_requests_total", "status" => status_label).increment(1);
    metrics::histogram!("finops_api_ingest_duration_seconds").record(elapsed.as_secs_f64());
}

//! POST /ingest — denormalize batch envelope to one JSONEachRow message per workload.

use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::kafka;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct IngestBatch {
    pub schema_version: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub batch_id: String,
    pub agent_version: String,
    pub workloads: Vec<WorkloadRow>,
}

#[derive(Debug, Deserialize)]
pub struct WorkloadRow {
    pub cgroup_id: u64,
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub container: Option<String>,
    pub k8s_resolved: bool,
    pub memory_bytes_max: u64,
    pub memory_bytes_last: u64,
    pub exec_count: u32,
    pub sample_count: u32,
}

/// Flat row for Kafka / ClickHouse JSONEachRow — borrows from `batch` (no per-row string clones).
#[derive(Serialize)]
struct FlatRow<'a> {
    window_start_ns: u64,
    window_end_ns: u64,
    node: &'a str,
    batch_id: &'a str,
    agent_version: &'a str,
    cgroup_id: u64,
    namespace: Option<&'a str>,
    pod: Option<&'a str>,
    container: Option<&'a str>,
    k8s_resolved: bool,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    exec_count: u32,
    sample_count: u32,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(batch): Json<IngestBatch>,
) -> Response {
    let started = Instant::now();
    let response = ingest_inner(state, batch).await;
    record_ingest_metrics(response.status(), started.elapsed());
    response
}

async fn ingest_inner(state: AppState, batch: IngestBatch) -> Response {
    if batch.schema_version != 2 {
        log::warn!(
            "Rejected batch with invalid schema_version={}",
            batch.schema_version
        );
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Unsupported schema_version={}. Expected 2.",
                batch.schema_version
            ),
        )
            .into_response();
    }

    // One `Vec<u8>` key per HTTP batch; per-row `clone` copies key bytes (no `Bytes` → `to_vec` at produce).
    let node_vec = batch.node.as_bytes().to_vec();
    let node_str = batch.node.as_str();

    for row in &batch.workloads {
        let flat = FlatRow {
            window_start_ns: batch.window_start_ns,
            window_end_ns: batch.window_end_ns,
            node: node_str,
            batch_id: batch.batch_id.as_str(),
            agent_version: batch.agent_version.as_str(),
            cgroup_id: row.cgroup_id,
            namespace: row.namespace.as_deref(),
            pod: row.pod.as_deref(),
            container: row.container.as_deref(),
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

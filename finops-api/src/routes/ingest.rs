//! POST /ingest — denormalize batch envelope to one JSONEachRow message per workload.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct IngestBatch {
    pub schema_version: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
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
) -> impl IntoResponse {
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

    for row in &batch.workloads {
        let flat = FlatRow {
            window_start_ns: batch.window_start_ns,
            window_end_ns: batch.window_end_ns,
            node: batch.node.as_str(),
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

        // One heap alloc from `to_vec`; `Bytes::from(vec)` reuses that buffer (no JSON memcpy).
        let bytes = match serde_json::to_vec(&flat) {
            Ok(b) => Bytes::from(b),
            Err(e) => {
                log::warn!("flat row JSON encode failed: {e}");
                continue;
            }
        };

        if state
            .kafka_tx
            .try_send((batch.node.clone(), bytes))
            .is_err()
        {
            log::warn!("Kafka channel full (backpressure), rejecting batch");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Ingest channel full. Broker backpressure active.",
            )
                .into_response();
        }
    }

    StatusCode::OK.into_response()
}

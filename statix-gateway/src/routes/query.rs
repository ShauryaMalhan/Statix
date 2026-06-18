//! GET /api/v1/workloads/summary — operational read over `statix.workload_metrics` (no `FINAL`).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::error::GatewayError;
use crate::AppState;

const SUMMARY_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       argMax(memory_bytes_max, window_start_ns) AS peak_memory,
       sum(exec_count) AS total_execs,
       sum(cpu_usage_usec) AS total_cpu_usec
FROM statix.workload_metrics
WHERE window_start_ns >= {cutoff_ns:UInt64}
GROUP BY cgroup_id, namespace, pod, container
ORDER BY peak_memory DESC
LIMIT 100
"#;

#[derive(Debug, Deserialize)]
pub struct SummaryParams {
    /// Lookback window in hours (default 24).
    pub hours: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct WorkloadSummaryRow {
    pub cgroup_id: u64,
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub container: Option<String>,
    pub peak_memory: u64,
    pub total_execs: u64,
    pub total_cpu_usec: u64,
}

pub async fn workloads_summary(
    State(state): State<AppState>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<Vec<WorkloadSummaryRow>>, StatusCode> {
    let hours = params.hours.unwrap_or(24);
    let cutoff_ns = cutoff_ns_from_hours(hours);

    match workloads_summary_inner(&state, cutoff_ns, hours).await {
        Ok(rows) => Ok(Json(rows)),
        Err(e) => {
            log::error!("{e}");
            Err(e.status_code())
        }
    }
}

async fn workloads_summary_inner(
    state: &AppState,
    cutoff_ns: u64,
    hours: u64,
) -> Result<Vec<WorkloadSummaryRow>, GatewayError> {
    state
        .ch_client
        .query(SUMMARY_SQL)
        .param("cutoff_ns", cutoff_ns)
        .fetch_all::<WorkloadSummaryRow>()
        .await
        .map_err(|e| GatewayError::ClickHouse(format!("hours={hours}: {e}")))
}

fn cutoff_ns_from_hours(hours: u64) -> u64 {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    now_ns.saturating_sub(hours.saturating_mul(3_600) * 1_000_000_000)
}

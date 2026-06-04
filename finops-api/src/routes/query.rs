//! GET /api/v1/workloads/summary — read path over `finops.workload_metrics FINAL`.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::AppState;

const SUMMARY_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       MAX(memory_bytes_max) AS peak_memory,
       SUM(exec_count) AS total_execs
FROM finops.workload_metrics FINAL
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
}

pub async fn workloads_summary(
    State(state): State<AppState>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<Vec<WorkloadSummaryRow>>, StatusCode> {
    let hours = params.hours.unwrap_or(24);
    let cutoff_ns = cutoff_ns_from_hours(hours);

    let rows = state
        .ch_client
        .query(SUMMARY_SQL)
        .param("cutoff_ns", cutoff_ns)
        .fetch_all::<WorkloadSummaryRow>()
        .await
        .map_err(|e| {
            log::error!("ClickHouse workloads summary query failed (hours={hours}): {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(rows))
}

fn cutoff_ns_from_hours(hours: u64) -> u64 {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    now_ns.saturating_sub(hours.saturating_mul(3_600) * 1_000_000_000)
}

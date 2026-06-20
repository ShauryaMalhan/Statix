//! POST /ingest — denormalize batch envelope to coalesced ClickHouse RowBinary rows.

use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use statix_wire::IngestBatch;
use tokio::sync::mpsc;

use crate::clickhouse_writer::MetricRow;
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

    if !state.ch_healthy.load(Ordering::Acquire) {
        metrics::counter!("statix_api_ch_unhealthy_reject_total").increment(1);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "ClickHouse unavailable; retry later.",
        )
            .into_response();
    }

    if batch.workloads.is_empty() {
        return StatusCode::OK.into_response();
    }

    let mut permits = match state.ingest_tx.try_reserve_many(batch.workloads.len()) {
        Ok(p) => p,
        Err(mpsc::error::TrySendError::Full(_)) => {
            metrics::counter!("statix_api_ingest_channel_full_total").increment(1);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Ingest buffer full. Retry later.",
            )
                .into_response();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Writer unavailable.",
            )
                .into_response();
        }
    };

    for w in &batch.workloads {
        permits
            .next()
            .expect("try_reserve_many exact capacity")
            .send(MetricRow::from_ingest(&batch, w));
    }

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let lag_secs = now_ns.saturating_sub(batch_window_end_ns) as f64 / 1_000_000_000.0;
    metrics::histogram!("statix_api_ingest_lag_seconds").record(lag_secs);

    StatusCode::OK.into_response()
}

fn record_ingest_metrics(status: StatusCode, elapsed: std::time::Duration) {
    let status_label = status.as_u16().to_string();
    metrics::counter!("statix_api_ingest_requests_total", "status" => status_label).increment(1);
    if status == StatusCode::SERVICE_UNAVAILABLE {
        metrics::counter!("statix_api_ingest_503_total").increment(1);
    }
    metrics::histogram!("statix_api_ingest_duration_seconds").record(elapsed.as_secs_f64());
}

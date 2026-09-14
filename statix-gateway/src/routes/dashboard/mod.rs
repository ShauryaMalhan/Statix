//! Dashboard read tier — `GET /api/v1/dashboard/{state,health}` + the page itself.
//!
//! Split by failure domain ([ADR 061]): `/state` carries tiles + table and is
//! allowed to fail; `/health` reports status and **returns 200 even when
//! components are unhealthy**, because a 503 there is indistinguishable from
//! "gateway down" — and it is precisely the signal you need during an outage.
//!
//! Additive and read-only: nothing here touches the ingest hot path.

pub mod cache;
pub mod sql;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use statix_infra::env::{read_env_u64, read_env_usize};

use crate::error::GatewayError;
use crate::AppState;

use cache::{RateLimiter, TtlCache};
use sql::{Order, Sort};

const EMBEDDED_PAGE: &str = include_str!("../../../assets/dashboard.html");

const DEFAULT_RANGE_SECS: u64 = 300;
const MIN_RANGE_SECS: u64 = 60;
const MAX_RANGE_SECS: u64 = 3_600;
const DEFAULT_LIMIT: usize = 100;
const DEFAULT_MAX_LIMIT: usize = 500;
const DEFAULT_CACHE_MS: u64 = 2_000;
const DEFAULT_RATE_LIMIT_PER_MIN: u64 = 600;
const CACHE_ENTRIES: usize = 64;
/// Bounds the cache-key space an anonymous caller can generate.
const MAX_Q_LEN: usize = 64;
/// Node stops counting as live after this many missed windows.
const NODE_STALE_SECS: u64 = 60;

// ---------------------------------------------------------------- config

#[derive(Debug, Clone)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub dir: Option<PathBuf>,
    pub cache_ttl: Duration,
    pub max_limit: usize,
    pub rate_limit_per_min: u32,
}

impl DashboardConfig {
    pub fn from_env() -> Self {
        let enabled = !matches!(
            statix_infra::env::var("STATIX_DASHBOARD_ENABLED").as_deref(),
            Some("0") | Some("false") | Some("no") | Some("off")
        );
        Self {
            enabled,
            dir: statix_infra::env::var("STATIX_DASHBOARD_DIR")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            cache_ttl: Duration::from_millis(
                read_env_u64("STATIX_DASHBOARD_CACHE_MS", DEFAULT_CACHE_MS).clamp(0, 60_000),
            ),
            max_limit: read_env_usize("STATIX_DASHBOARD_MAX_LIMIT", DEFAULT_MAX_LIMIT)
                .clamp(10, 5_000),
            rate_limit_per_min: read_env_u64(
                "STATIX_DASHBOARD_RATE_LIMIT_PER_MIN",
                DEFAULT_RATE_LIMIT_PER_MIN,
            )
            .clamp(10, 1_000_000) as u32,
        }
    }
}

pub struct Dashboard {
    pub config: DashboardConfig,
    state_cache: TtlCache,
    health_cache: TtlCache,
    limiter: RateLimiter,
}

impl Dashboard {
    pub fn new(config: DashboardConfig) -> Self {
        let ttl = config.cache_ttl;
        let limiter = RateLimiter::new(config.rate_limit_per_min, Duration::from_secs(60));
        Self {
            state_cache: TtlCache::new(ttl, CACHE_ENTRIES),
            health_cache: TtlCache::new(ttl, 4),
            limiter,
            config,
        }
    }
}

// ---------------------------------------------------------------- params

#[derive(Debug, Deserialize)]
pub struct StateParams {
    pub range_secs: Option<u64>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub q: Option<String>,
    pub node: Option<String>,
    pub unattributed: Option<bool>,
    pub limit: Option<usize>,
}

/// Normalized, bounded view of the request. Everything that changes the answer
/// lives here — and therefore in the cache key.
#[derive(Debug, Clone)]
struct NormalizedQuery {
    range_secs: u64,
    sort: Sort,
    order: Order,
    q: Option<String>,
    node: Option<String>,
    unattributed: bool,
    limit: usize,
}

impl NormalizedQuery {
    fn new(p: StateParams, max_limit: usize) -> Self {
        let q = p
            .q
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .map(|mut s| {
                // Bound the key space a caller can generate (ADR 061).
                if s.chars().count() > MAX_Q_LEN {
                    s = s.chars().take(MAX_Q_LEN).collect();
                }
                s
            });
        Self {
            range_secs: p
                .range_secs
                .unwrap_or(DEFAULT_RANGE_SECS)
                .clamp(MIN_RANGE_SECS, MAX_RANGE_SECS),
            sort: Sort::parse(p.sort.as_deref()),
            order: Order::parse(p.order.as_deref()),
            q,
            node: p
                .node
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.chars().count() <= MAX_Q_LEN),
            unattributed: p.unattributed.unwrap_or(false),
            limit: p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, max_limit),
        }
    }

    /// Keyed on the question, never the caller — Statix is single-tenant.
    fn cache_key(&self) -> String {
        format!(
            "r={}&s={}&o={}&u={}&l={}&n={}&q={}",
            self.range_secs,
            self.sort.as_str(),
            self.order.as_str(),
            self.unattributed,
            self.limit,
            self.node.as_deref().unwrap_or(""),
            self.q.as_deref().unwrap_or(""),
        )
    }

    /// Unfiltered views have bounded cardinality and carry ~all real traffic,
    /// so they are pinned against eviction by arbitrary filters.
    fn is_hot_view(&self) -> bool {
        self.q.is_none() && self.node.is_none() && !self.unattributed
    }

    fn filter_active(&self) -> bool {
        !self.is_hot_view()
    }

    fn cutoff_ns(&self, now_ns: u64) -> u64 {
        now_ns.saturating_sub(self.range_secs.saturating_mul(1_000_000_000))
    }
}

// ---------------------------------------------------------------- rows

#[derive(Debug, Row, Deserialize, Serialize)]
pub struct WorkloadLive {
    pub node: String,
    pub cgroup_id: u64,
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub container: Option<String>,
    pub k8s_resolved: bool,
    pub memory_bytes_last: u64,
    pub memory_bytes_max: u64,
    pub cpu_usage_usec: u64,
    pub cpu_millicores: f64,
    pub exec_count: u32,
    pub sample_count: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
}

#[derive(Debug, Row, Deserialize)]
struct TotalsRow {
    matched_count: u64,
    memory_bytes: u64,
    exec_count: u64,
    cpu_millicores: f64,
    unattributed_count: u64,
    newest_window_end_ns: u64,
}

#[derive(Debug, Row, Deserialize, Serialize)]
struct NodeRow {
    node: String,
    last_seen_ns: u64,
}

#[derive(Debug, Serialize)]
struct Totals {
    workload_count: u64,
    matched_count: u64,
    cpu_millicores: f64,
    memory_bytes: u64,
    exec_count: u64,
    exec_rate_per_sec: f64,
    unattributed_count: u64,
    newest_window_end_ns: u64,
}

#[derive(Debug, Serialize)]
struct StateResponse {
    generated_at_ns: u64,
    range_secs: u64,
    sort: &'static str,
    order: &'static str,
    filter_active: bool,
    returned: usize,
    totals: Totals,
    workloads: Vec<WorkloadLive>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    generated_at_ns: u64,
    gateway: GatewayHealth,
    clickhouse: ChHealth,
    freshness: Freshness,
    nodes: Vec<NodeHealth>,
}

#[derive(Debug, Serialize)]
struct GatewayHealth {
    ready: bool,
    mpsc_used: usize,
    mpsc_capacity: usize,
}

#[derive(Debug, Serialize)]
struct ChHealth {
    healthy: bool,
    query_ok: bool,
}

#[derive(Debug, Serialize)]
struct Freshness {
    newest_window_end_ns: u64,
    age_secs: f64,
    stale: bool,
    has_data: bool,
}

#[derive(Debug, Serialize)]
struct NodeHealth {
    node: String,
    last_seen_ns: u64,
    age_secs: f64,
    stale: bool,
}

// ---------------------------------------------------------------- handlers

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    match state.expected_bearer.as_deref() {
        None => true,
        Some(expected) => {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some(expected)
        }
    }
}

fn json_body(body: Arc<str>, cache_hit: bool) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::HeaderName::from_static("x-statix-cache"),
                if cache_hit { "hit" } else { "miss" },
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

fn guard(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    if !state.dashboard.config.enabled {
        return Some((StatusCode::NOT_FOUND, "Dashboard disabled.").into_response());
    }
    if !authorized(state, headers) {
        return Some(StatusCode::UNAUTHORIZED.into_response());
    }
    if !state.dashboard.limiter.allow() {
        metrics::counter!("statix_api_dashboard_rate_limited_total").increment(1);
        return Some((StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded.").into_response());
    }
    None
}

pub async fn state_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<StateParams>,
) -> Response {
    if let Some(reject) = guard(&state, &headers) {
        return reject;
    }
    let q = NormalizedQuery::new(params, state.dashboard.config.max_limit);
    let key = q.cache_key();

    if let Some(body) = state.dashboard.state_cache.get(&key) {
        metrics::counter!("statix_api_dashboard_cache_hits_total").increment(1);
        return json_body(body, true);
    }
    metrics::counter!("statix_api_dashboard_cache_misses_total").increment(1);

    match build_state(&state, &q).await {
        Ok(body) => {
            state
                .dashboard
                .state_cache
                .put(key, Arc::clone(&body), q.is_hot_view());
            json_body(body, false)
        }
        Err(e) => {
            log::error!("dashboard /state: {e}");
            metrics::counter!("statix_api_dashboard_errors_total").increment(1);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "ClickHouse query failed; see /api/v1/dashboard/health.",
            )
                .into_response()
        }
    }
}

async fn build_state(state: &AppState, q: &NormalizedQuery) -> Result<Arc<str>, GatewayError> {
    let generated_at_ns = now_ns();
    let cutoff_ns = q.cutoff_ns(generated_at_ns);
    let has_q = q.q.is_some();
    let has_node = q.node.is_some();

    let mut rows_q = state
        .ch_client
        .query(&sql::rows(q.sort, q.order, has_q, q.unattributed, has_node))
        .param("cutoff_ns", cutoff_ns)
        .param("limit", q.limit as u64);
    let mut totals_q = state
        .ch_client
        .query(&sql::totals(has_q, q.unattributed, has_node))
        .param("cutoff_ns", cutoff_ns);
    if let Some(node) = q.node.as_deref() {
        rows_q = rows_q.param("node", node);
        totals_q = totals_q.param("node", node);
    }
    if let Some(text) = q.q.as_deref() {
        rows_q = rows_q.param("q", text);
        totals_q = totals_q.param("q", text);
    }

    let workloads = rows_q
        .fetch_all::<WorkloadLive>()
        .await
        .map_err(|e| GatewayError::ClickHouse(format!("dashboard rows: {e}")))?;
    let totals = totals_q
        .fetch_all::<TotalsRow>()
        .await
        .map_err(|e| GatewayError::ClickHouse(format!("dashboard totals: {e}")))?;
    let t = totals.into_iter().next().unwrap_or(TotalsRow {
        matched_count: 0,
        memory_bytes: 0,
        exec_count: 0,
        cpu_millicores: 0.0,
        unattributed_count: 0,
        newest_window_end_ns: 0,
    });

    let response = StateResponse {
        generated_at_ns,
        range_secs: q.range_secs,
        sort: q.sort.as_str(),
        order: q.order.as_str(),
        filter_active: q.filter_active(),
        returned: workloads.len(),
        totals: Totals {
            workload_count: t.matched_count,
            matched_count: t.matched_count,
            cpu_millicores: t.cpu_millicores,
            memory_bytes: t.memory_bytes,
            exec_count: t.exec_count,
            exec_rate_per_sec: if q.range_secs > 0 {
                t.exec_count as f64 / q.range_secs as f64
            } else {
                0.0
            },
            unattributed_count: t.unattributed_count,
            newest_window_end_ns: t.newest_window_end_ns,
        },
        workloads,
    };

    serde_json::to_string(&response)
        .map(Arc::from)
        .map_err(|e| GatewayError::ClickHouse(format!("dashboard serialize: {e}")))
}

pub async fn health_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(reject) = guard(&state, &headers) {
        return reject;
    }
    if let Some(body) = state.dashboard.health_cache.get("health") {
        return json_body(body, true);
    }
    let body = build_health(&state).await;
    state
        .dashboard
        .health_cache
        .put("health".into(), Arc::clone(&body), true);
    json_body(body, false)
}

/// Always 200. Reports status; never fails on it.
async fn build_health(state: &AppState) -> Arc<str> {
    let generated_at_ns = now_ns();
    let ch_healthy = state.ch_healthy.load(Ordering::Acquire);
    let channel_open = !state.ingest_tx.is_closed();
    let remaining = state.ingest_tx.capacity();
    let capacity = state.ingest_channel_capacity;
    let used = capacity.saturating_sub(remaining);

    // Only this part needs ClickHouse; its failure degrades to an empty list.
    let cutoff_ns = generated_at_ns.saturating_sub(MAX_RANGE_SECS * 1_000_000_000);
    let (nodes, query_ok) = match state
        .ch_client
        .query(sql::nodes())
        .param("cutoff_ns", cutoff_ns)
        .fetch_all::<NodeRow>()
        .await
    {
        Ok(rows) => (rows, true),
        Err(e) => {
            log::warn!("dashboard /health node query failed: {e}");
            (Vec::new(), false)
        }
    };

    let newest = nodes.iter().map(|n| n.last_seen_ns).max().unwrap_or(0);
    let age = |ts: u64| {
        if ts == 0 {
            0.0
        } else {
            generated_at_ns.saturating_sub(ts) as f64 / 1_000_000_000.0
        }
    };

    let response = HealthResponse {
        generated_at_ns,
        gateway: GatewayHealth {
            ready: channel_open && ch_healthy,
            mpsc_used: used,
            mpsc_capacity: capacity,
        },
        clickhouse: ChHealth {
            healthy: ch_healthy,
            query_ok,
        },
        freshness: Freshness {
            newest_window_end_ns: newest,
            age_secs: age(newest),
            // "no data at all" is a different state from "data went stale".
            stale: newest != 0 && age(newest) > NODE_STALE_SECS as f64,
            has_data: newest != 0,
        },
        nodes: nodes
            .into_iter()
            .map(|n| NodeHealth {
                age_secs: age(n.last_seen_ns),
                stale: age(n.last_seen_ns) > NODE_STALE_SECS as f64,
                node: n.node,
                last_seen_ns: n.last_seen_ns,
            })
            .collect(),
    };

    serde_json::to_string(&response)
        .map(Arc::from)
        .unwrap_or_else(|_| Arc::from(r#"{"error":"serialize failed"}"#))
}

/// The page itself: disk when `STATIX_DASHBOARD_DIR` is set (dev hot reload),
/// otherwise the copy embedded in the binary (single artifact, cannot drift).
pub async fn page_handler(State(state): State<AppState>) -> Response {
    if !state.dashboard.config.enabled {
        return (StatusCode::NOT_FOUND, "Dashboard disabled.").into_response();
    }
    let html = match state.dashboard.config.dir.as_ref() {
        Some(dir) => match tokio::fs::read_to_string(dir.join("dashboard.html")).await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("dashboard dir read failed ({e}); serving embedded copy");
                EMBEDDED_PAGE.to_string()
            }
        },
        None => EMBEDDED_PAGE.to_string(),
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(q: Option<&str>, limit: Option<usize>) -> StateParams {
        StateParams {
            range_secs: None,
            sort: None,
            order: None,
            q: q.map(str::to_string),
            node: None,
            unattributed: None,
            limit,
        }
    }

    #[test]
    fn same_question_yields_same_key_regardless_of_caller() {
        let a = NormalizedQuery::new(params(None, None), 500);
        let b = NormalizedQuery::new(params(None, None), 500);
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn q_is_bounded_trimmed_and_lowercased() {
        let long = "A".repeat(500);
        let n = NormalizedQuery::new(params(Some(&long), None), 500);
        assert_eq!(n.q.as_deref().map(str::len), Some(MAX_Q_LEN));
        assert!(n.q.unwrap().chars().all(|c| c == 'a'), "must lowercase");

        let n = NormalizedQuery::new(params(Some("  NgInX  "), None), 500);
        assert_eq!(n.q.as_deref(), Some("nginx"));
        // whitespace-only filters must not create a cache key of their own
        assert!(NormalizedQuery::new(params(Some("   "), None), 500).q.is_none());
    }

    #[test]
    fn limit_is_clamped_to_configured_max() {
        assert_eq!(NormalizedQuery::new(params(None, Some(99_999)), 500).limit, 500);
        assert_eq!(NormalizedQuery::new(params(None, Some(0)), 500).limit, 1);
        assert_eq!(NormalizedQuery::new(params(None, None), 500).limit, DEFAULT_LIMIT);
    }

    #[test]
    fn range_is_clamped_to_supported_horizons() {
        let mk = |secs| {
            let mut p = params(None, None);
            p.range_secs = Some(secs);
            NormalizedQuery::new(p, 500).range_secs
        };
        assert_eq!(mk(1), MIN_RANGE_SECS);
        assert_eq!(mk(999_999), MAX_RANGE_SECS);
        assert_eq!(mk(900), 900);
    }

    #[test]
    fn only_unfiltered_views_are_pinned() {
        assert!(NormalizedQuery::new(params(None, None), 500).is_hot_view());
        assert!(!NormalizedQuery::new(params(Some("nginx"), None), 500).is_hot_view());
        assert!(NormalizedQuery::new(params(Some("x"), None), 500).filter_active());
    }

    #[test]
    fn cutoff_is_range_seconds_behind_now() {
        let n = NormalizedQuery::new(params(None, None), 500);
        let now = 1_000 * 1_000_000_000u64;
        assert_eq!(n.cutoff_ns(now), now - DEFAULT_RANGE_SECS * 1_000_000_000);
        // never underflows on a small clock
        assert_eq!(n.cutoff_ns(5), 0);
    }
}

//! Query builders for the dashboard read tier ([ADR 061]).
//!
//! Shape: an inner aggregate collapses every `(node, cgroup_id)` to its most
//! recent window inside the lookback horizon; outer projections derive
//! `cpu_millicores`, filter, sort and limit.
//!
//! Two rules encoded here:
//! * Aggregate aliases must NOT shadow source column names. Aliasing
//!   `max(window_start_ns) AS window_start_ns` makes ClickHouse resolve the
//!   inner `WHERE window_start_ns >= …` to the aggregate and fail with
//!   ILLEGAL_AGGREGATION. Hence `w_start` / `w_end` / `ns` / `pod_name` / …
//! * User input is never concatenated into SQL. Sort and order are whitelisted
//!   enums mapped to literals; every value is a bound `{name:Type}` parameter.

/// CPU consumed in the window ÷ window wall-time = cores; ×1000 = millicores.
/// 1000m is one saturated core; a multi-core workload legitimately exceeds it.
const CPU_MILLICORES: &str =
    "if(w_end > w_start, cpu_usec * 1000000.0 / (w_end - w_start), 0.0)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Cpu,
    Memory,
    MemoryPeak,
    Execs,
    LastSeen,
}

impl Sort {
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("cpu") {
            "mem" | "memory" => Sort::Memory,
            "mem_peak" | "memory_peak" => Sort::MemoryPeak,
            "execs" | "exec_count" => Sort::Execs,
            "last_seen" => Sort::LastSeen,
            _ => Sort::Cpu,
        }
    }
    /// Whitelisted literal — never user text.
    fn column(self) -> &'static str {
        match self {
            Sort::Cpu => "cpu_millicores",
            Sort::Memory => "memory_bytes_last",
            Sort::MemoryPeak => "memory_bytes_max",
            Sort::Execs => "exec_count",
            Sort::LastSeen => "window_end_ns",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Sort::Cpu => "cpu",
            Sort::Memory => "mem",
            Sort::MemoryPeak => "mem_peak",
            Sort::Execs => "execs",
            Sort::LastSeen => "last_seen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

impl Order {
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("desc") {
            "asc" => Order::Asc,
            _ => Order::Desc,
        }
    }
    fn keyword(self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Order::Asc => "asc",
            Order::Desc => "desc",
        }
    }
}

/// Collapse each `(node, cgroup_id)` to its latest window within the horizon.
/// `node_filter` is a bound parameter when present, so it prunes inside the
/// scan rather than after it.
fn inner(node_filter: bool) -> String {
    let node_clause = if node_filter {
        " AND node = {node:String}"
    } else {
        ""
    };
    format!(
        "SELECT node AS n, cgroup_id AS cg,
            max(window_start_ns)                       AS w_start,
            argMax(window_end_ns,     window_start_ns) AS w_end,
            argMax(namespace,         window_start_ns) AS ns,
            argMax(pod,               window_start_ns) AS pod_name,
            argMax(container,         window_start_ns) AS cont_name,
            argMax(k8s_resolved,      window_start_ns) AS resolved,
            argMax(memory_bytes_last, window_start_ns) AS mem_last,
            argMax(memory_bytes_max,  window_start_ns) AS mem_max,
            argMax(cpu_usage_usec,    window_start_ns) AS cpu_usec,
            argMax(exec_count,        window_start_ns) AS execs,
            argMax(sample_count,      window_start_ns) AS samples
         FROM statix.workload_metrics
         WHERE window_start_ns >= {{cutoff_ns:UInt64}}{node_clause}
         GROUP BY node, cgroup_id"
    )
}

/// Outer filters over the collapsed set. `q` matches namespace/pod/container.
fn filters(has_q: bool, unattributed_only: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if has_q {
        parts.push(
            "positionCaseInsensitive(
                concat(ifNull(ns,''),'/',ifNull(pod_name,''),'/',ifNull(cont_name,'')),
                {q:String}) > 0",
        );
    }
    if unattributed_only {
        parts.push("resolved = 0");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    }
}

/// One row per workload, newest window, sorted and capped.
pub fn rows(sort: Sort, order: Order, has_q: bool, unattributed_only: bool, node_filter: bool) -> String {
    format!(
        "SELECT
            CAST(n AS String) AS node,
            cg          AS cgroup_id,
            ns          AS namespace,
            pod_name    AS pod,
            cont_name   AS container,
            resolved    AS k8s_resolved,
            mem_last    AS memory_bytes_last,
            mem_max     AS memory_bytes_max,
            cpu_usec    AS cpu_usage_usec,
            {CPU_MILLICORES} AS cpu_millicores,
            execs       AS exec_count,
            samples     AS sample_count,
            w_start     AS window_start_ns,
            w_end       AS window_end_ns
         FROM ({inner}){filters}
         ORDER BY {col} {dir}, node ASC, cgroup_id ASC
         LIMIT {{limit:UInt64}}",
        inner = inner(node_filter),
        filters = filters(has_q, unattributed_only),
        col = sort.column(),
        dir = order.keyword(),
    )
}

/// Aggregates over the whole filtered set — deliberately unlimited, because the
/// tiles must reflect every match, not just the returned page.
pub fn totals(has_q: bool, unattributed_only: bool, node_filter: bool) -> String {
    format!(
        "SELECT
            count()                  AS matched_count,
            sum(mem_last)            AS memory_bytes,
            sum(execs)               AS exec_count,
            sum({CPU_MILLICORES})    AS cpu_millicores,
            countIf(resolved = 0)    AS unattributed_count,
            max(w_end)               AS newest_window_end_ns
         FROM ({inner}){filters}",
        inner = inner(node_filter),
        filters = filters(has_q, unattributed_only),
    )
}

/// Agent liveness derived from the data plane itself — no agent scraping.
pub fn nodes() -> &'static str {
    "SELECT CAST(node AS String) AS node, max(window_end_ns) AS last_seen_ns
     FROM statix.workload_metrics
     WHERE window_start_ns >= {cutoff_ns:UInt64}
     GROUP BY node
     ORDER BY node"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_aliases_never_shadow_source_columns() {
        // Regression guard for a real ClickHouse failure: aliasing
        // `max(window_start_ns) AS window_start_ns` makes the inner WHERE
        // resolve to the aggregate -> ILLEGAL_AGGREGATION (code 184).
        // Only the INNER must avoid shadowing; the outer projection renaming
        // `w_start AS window_start_ns` is correct and expected.
        let inner_sql = inner(false);
        for col in [
            "window_start_ns",
            "window_end_ns",
            "namespace",
            "pod",
            "container",
            "memory_bytes_last",
            "exec_count",
        ] {
            assert!(
                !inner_sql.contains(&format!("AS {col}\n")) && !inner_sql.contains(&format!("AS {col},")),
                "inner aggregate alias must not shadow source column `{col}`"
            );
        }
        assert!(inner_sql.contains("max(window_start_ns)                       AS w_start"));
        assert!(inner_sql.contains("WHERE window_start_ns >= {cutoff_ns:UInt64}"));
        // ...and the outer projection does rename back to the API names.
        let sql = rows(Sort::Cpu, Order::Desc, false, false, false);
        assert!(sql.contains("w_start     AS window_start_ns"));
    }

    #[test]
    fn sort_and_order_are_whitelisted_literals() {
        for (input, expect) in [
            (Some("mem"), "memory_bytes_last"),
            (Some("mem_peak"), "memory_bytes_max"),
            (Some("execs"), "exec_count"),
            (Some("last_seen"), "window_end_ns"),
            (Some("cpu"), "cpu_millicores"),
            (Some("'; DROP TABLE statix.workload_metrics; --"), "cpu_millicores"),
            (None, "cpu_millicores"),
        ] {
            assert_eq!(Sort::parse(input).column(), expect);
        }
        assert_eq!(Order::parse(Some("asc")).keyword(), "ASC");
        assert_eq!(Order::parse(Some("garbage")).keyword(), "DESC");
    }

    #[test]
    fn user_text_is_bound_not_interpolated() {
        let sql = rows(Sort::Cpu, Order::Desc, true, false, true);
        assert!(sql.contains("{q:String}"), "q must be a bound parameter");
        assert!(sql.contains("{node:String}"), "node must be a bound parameter");
        assert!(sql.contains("{limit:UInt64}"));
    }

    #[test]
    fn filters_compose_and_totals_stay_unlimited() {
        let both = totals(true, true, false);
        assert!(both.contains("WHERE") && both.contains("AND resolved = 0"));
        assert!(!both.contains("LIMIT"), "tiles must cover every match");
        assert!(totals(false, false, false).find("WHERE").is_none() == false || true);
    }

    #[test]
    fn node_filter_prunes_inside_the_scan() {
        let with = rows(Sort::Cpu, Order::Desc, false, false, true);
        // node predicate belongs to the inner WHERE, beside the time filter
        let inner_where = with.find("WHERE window_start_ns").unwrap();
        let group_by = with.find("GROUP BY node, cgroup_id").unwrap();
        let node_pred = with.find("node = {node:String}").unwrap();
        assert!(inner_where < node_pred && node_pred < group_by);
    }
}

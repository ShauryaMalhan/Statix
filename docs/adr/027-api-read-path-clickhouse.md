# ADR 027: API read-path — ClickHouse workload summary

**Status:** Accepted  
**Date:** 2026-06-04  
**Context:** Target 3 — expose billing-style rollups from `finops.workload_metrics` without a separate query service ([ADR 011](011-replacingmergetree-dedupe-identity.md), [ADR 026](026-clickhouse-finops-database-init.md)).

## Decision

- **`clickhouse` crate** (`0.13`) on `finops-api`; `AppState.ch_client` built at startup.
- **Env:** `CLICKHOUSE_URL` (default `http://localhost:8123`); `CLICKHOUSE_USER` (default `default`); `CLICKHOUSE_PASSWORD` (default empty — Compose uses `finops_dev`).
- **Route:** `GET /api/v1/workloads/summary?hours=<u64>` — optional lookback hours (default **24**).
- **Query:** Aggregated read over `finops.workload_metrics` with `window_start_ns >= {cutoff_ns:UInt64}`; top 100 by `peak_memory`; server-side `.param("cutoff_ns", …)`. Originally used `FINAL`; operational path now uses `argMax` ([ADR 033](033-phase55-l8-p1-week-gateway-fixes.md)).
- **Handler:** `finops-api/src/routes/query.rs` — `WorkloadSummaryRow` derives `clickhouse::Row` + serde.

Read path is **not** on the hot ingest path; no change to `POST /ingest` `try_send` contract ([ADR 005](005-non-blocking-ingest-pipeline.md)).

## Rationale

- Reuses gateway deployment; same credentials as ops already use for CH HTTP.
- `FINAL` + `GROUP BY` cgroup/pod/container matches FinOps billing questions (peak memory, exec totals).
- Parameterized `cutoff_ns` avoids string interpolation SQL injection.

## Consequences

- **Positive:** Single service for write + read in dev (`make compose-up`).
- **Negative:** Read load shares gateway process; scale reads separately later if needed.
- **Negative:** `/ready` does not yet probe ClickHouse — ingest readiness unchanged ([ADR 021](021-ingest-ready-probe.md)).
- **Code:** `finops-api/src/main.rs`, `routes/query.rs`, `docker-compose.yml` (`CLICKHOUSE_*` on `finops-api`).

## References

- `deploy/clickhouse/01_init.sql`
- [ADR 018](018-phase-roadmap-status.md) — Target 3

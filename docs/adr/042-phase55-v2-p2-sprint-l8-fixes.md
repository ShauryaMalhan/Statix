# ADR 042: Phase 5.5 V2 P2-SPRINT fixes (GA observability + thundering herd)

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** Final L8 Audit V2 items before FinOps Agent v1.0 GA — spread post-outage retry load and expose pipeline/CH health metrics.

## Decision

| ID | Area | Fix |
|----|------|-----|
| V2-15 | `statix/src/output.rs` | After successful retry when `backoff_secs > initial_backoff`, sleep `rand(0..5)` seconds before dequeuing next batch |
| V2-18 | `statix-gateway/src/routes/ingest.rs` | Capture `batch_window_end_ns`; on `200 OK` record `statix_api_ingest_lag_seconds` histogram |
| V2-16 | `deploy/grafana/clickhouse_monitoring.sql` | Grafana/alert SQL for `system.parts` active count + `system.merges` queue depth |

## Rationale

- **V2-15:** Gateway recovery must not trigger 5000-agent simultaneous retry flush (thundering herd).
- **V2-18:** Agent window-close → gateway accept lag is the first hop in end-to-end pipeline SLO diagnosis.
- **V2-16:** Merge backlog visibility prevents silent `TOO_MANY_PARTS` degradation on `statix.workload_metrics`.

## Consequences

- **Metric:** `statix_api_ingest_lag_seconds` on successful ingest only (not 4xx/5xx/503).
- **Alerting:** Wire `clickhouse_monitoring.sql` queries into Grafana; P1 at 300 parts, P0 at 1000.

## References

- [ADR 006](006-shared-http-client-for-ingest.md) — retry worker
- [ADR 007](007-clickhouse-mergetree-tuning.md) — MergeTree tuning
- [ADR 031](031-grafana-clickhouse-compose.md) — Grafana in Compose

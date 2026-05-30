# ADR 007: ClickHouse MergeTree partition, sort key, and TTL

**Status:** Accepted  
**Date:** 2026-05-30  
**Context:** `finops_telemetry` storage layout for Phase 3 Kafka ingest.

## Decision

`infra/clickhouse/init.sql` for `finops_telemetry`:

- **Partition:** `toYYYYMMDD(...)` on `window_start_ns` (daily parts, not monthly).
- **ORDER BY:** `(namespace, pod, node, window_start_ns)` — not `(node, pod, ...)`.
- **TTL:** 30 days from event time — automatic part drops on dev/docker hosts.

Kafka engine table stays plain `String` columns (JSONEachRow wire format). `LowCardinality` only on MergeTree target.

## Rationale

- **Daily partitions:** Exec/memory storms create many rows per window; smaller parts reduce merge pressure and pair cleanly with TTL.
- **Sort key:** Phase 4+ billing and dashboards filter by `namespace` / `pod` first; primary index should match that access path.
- **TTL:** Prevents unbounded growth on local `clickhouse-data` volumes during iterative testing.

## Consequences

- **Positive:** Faster namespace-scoped queries; predictable disk on dev stacks.
- **Negative:** More part files than monthly partitioning at very low volume (negligible for our scale).
- **Negative:** `IF NOT EXISTS` does not alter existing tables — dev must `docker compose down -v` after schema changes.
- **Deferred:** Production retention (30d) may move to env-specific SQL or tiered storage.

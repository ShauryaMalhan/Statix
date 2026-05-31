# ADR 007: ClickHouse MergeTree partition, sort key, and TTL

**Status:** Accepted  
**Date:** 2026-05-30  
**Context:** `finops_telemetry` storage layout for Phase 3 Kafka ingest.

## Decision

`infra/clickhouse/init.sql` for `finops_telemetry`:

- **Partition:** `toYYYYMMDD(...)` on `window_start_ns` (daily parts, not monthly).
- **ORDER BY:** `(node, namespace, window_start_ns, cgroup_id)` — `node` first; `SETTINGS allow_nullable_key = 1` (nullable `namespace` in key).
- **LowCardinality:** `node`, `namespace` only — **not** `pod` / `container` (replica hashes and CRI IDs are high-cardinality; LC dictionaries OOM at scale).
- **TTL:** 30 days from event time — automatic part drops on dev/docker hosts.

Kafka engine table stays plain `String` columns (JSONEachRow wire format). Kafka consumer resilience — [ADR 008](008-clickhouse-kafka-engine-resilience.md).

## Rationale

- **Daily partitions:** Exec/memory storms create many rows per window; smaller parts reduce merge pressure and pair cleanly with TTL.
- **Sort key:** Per-node dashboards and daemonset billing filter by `node` first; `namespace` second; time + `cgroup_id` for tie-break. Leading with Nullable `namespace`/`pod` groups NULL workloads into one sparse-index block and hurts skip efficiency.
- **LowCardinality:** Safe for tens of namespaces and node names; unsafe for unbounded pod/container strings.
- **TTL:** Prevents unbounded growth on local `clickhouse-data` volumes during iterative testing.

## Consequences

- **Positive:** Predictable CH RAM; better index skip for node-scoped queries; predictable disk on dev stacks.
- **Negative:** Namespace-first-only queries may scan more granules than namespace-leading key (mitigated by partition + `node` filter).
- **Negative:** `IF NOT EXISTS` does not alter existing tables — dev must `docker compose down -v` after schema changes.
- **Deferred:** Production retention (30d) may move to env-specific SQL or tiered storage.

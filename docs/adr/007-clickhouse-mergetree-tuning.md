# ADR 007: ClickHouse storage layout (partition, sort key, TTL)

**Status:** Accepted (amended by [ADR 011](011-replacingmergetree-dedupe-identity.md))  
**Date:** 2026-05-30  
**Context:** ClickHouse storage layout for Phase 3 Kafka ingest.

## Decision

`deploy/clickhouse/01_init.sql` for `finops.workload_metrics`:

- **Engine:** `ReplacingMergeTree()` — dedupe on merge; billing queries use `FINAL` ([ADR 011](011-replacingmergetree-dedupe-identity.md)).
- **Partition:** `toYYYYMMDD(...)` on `window_start_ns` (daily parts, not monthly).
- **ORDER BY:** `(node, window_start_ns, cgroup_id)` — billing identity only; **not** `namespace` (mutable K8s metadata).
- **LowCardinality:** `node`, `namespace` only — **not** `pod` / `container` (replica hashes and CRI IDs are high-cardinality; LC dictionaries OOM at scale).
- **TTL:** 30 days from event time — automatic part drops on dev/docker hosts.

Kafka engine table stays plain `String` columns (JSONEachRow wire format). Kafka consumer resilience — [ADR 008](008-clickhouse-kafka-engine-resilience.md).

## Rationale

- **Daily partitions:** Exec/memory storms create many rows per window; smaller parts reduce merge pressure and pair cleanly with TTL.
- **Sort key:** Per-node dashboards filter by `node` first; `window_start_ns` + `cgroup_id` identify one workload window for dedupe and skip efficiency.
- **LowCardinality:** Safe for tens of namespaces and node names; unsafe for unbounded pod/container strings.
- **TTL:** Prevents unbounded growth on local `clickhouse-data` volumes during iterative testing.

## Consequences

- **Positive:** Predictable CH RAM; better index skip for node-scoped queries; predictable disk on dev stacks.
- **Negative:** `FINAL` required for exact billing counts before background merge ([ADR 011](011-replacingmergetree-dedupe-identity.md)).
- **Negative:** `IF NOT EXISTS` does not alter existing tables — dev must `docker compose down -v` after schema changes.
- **Deferred:** Production retention (30d) may move to env-specific SQL or tiered storage.

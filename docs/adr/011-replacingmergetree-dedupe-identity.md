# ADR 011: ReplacingMergeTree dedupe identity (no `namespace` in sort key)

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Agent ingest retry ([output.rs](../../finops-user/src/output.rs)) can deliver the same billing window twice. K8s labels may change between attempts (e.g. `namespace` NULL on first POST, `"default"` after resolver catches up). `MergeTree` kept every row; an `ORDER BY` that included `namespace` made ReplacingMergeTree treat retries as different identities → double billing.

## Decision

`finops.workload_metrics` in `deploy/clickhouse/01_init.sql`:

- **Engine:** `ReplacingMergeTree()` (default row selection on merge — later insert typically wins).
- **ORDER BY:** `(node, window_start_ns, cgroup_id)` — stable workload identity per aggregation window.
- **Removed:** `namespace` from sort key; `SETTINGS allow_nullable_key = 1` (no nullable keys in ORDER BY).
- **Unchanged:** Kafka engine table, materialized view, daily partition, 30d TTL, LowCardinality on `node`/`namespace` only ([ADR 007](007-clickhouse-mergetree-tuning.md) LC rules).

## Read path (billing)

ReplacingMergeTree deduplicates **asynchronously** during background merges. For billing aggregates and dashboards, queries **must** use `FINAL`:

```sql
SELECT sum(memory_bytes_max) FROM finops.workload_metrics FINAL WHERE node = 'node-1';
```

Without `FINAL`, duplicate rows may appear until merges complete.

## Rationale

- **Mutable metadata:** `namespace`, `pod`, `container`, `k8s_resolved` are enrichment fields, not billing identity.
- **Retry safety:** Same `(node, window_start_ns, cgroup_id)` collapses on merge regardless of label changes between agent retries.
- **Pairs with:** Agent bounded retry queue (ADR 006 / Phase 4 TODO 3.2 shipped).

## Consequences

- **Positive:** Idempotent storage for retried windows without `batch_id` on wire (yet).
- **Negative:** `FINAL` adds read cost — acceptable for billing batch jobs; avoid on hot exploratory scans without need for exact counts.
- **Negative:** `CREATE TABLE IF NOT EXISTS` does not migrate existing volumes — `docker compose down -v && make compose-up` after schema change.
- **Deferred:** Explicit version column (e.g. `window_end_ns`) if merge tie-break must be deterministic; `batch_id` on wire for audit ([TODO 4.6](../../.cursor/skills/finops-ebpf-agent/TODO.md)).

## References

- `deploy/clickhouse/01_init.sql`
- [ADR 007](007-clickhouse-mergetree-tuning.md) — partitions, TTL, LowCardinality

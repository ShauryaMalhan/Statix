# ADR 059: Phase 10 — ClickHouse `cgroup_id` minmax skip index

**Status:** Accepted  
**Date:** 2026-06-18  
**Context:** Phase 10 observability/cost — FinOps read queries filter by `cgroup_id` (summary API, billing drill-down). `ORDER BY (node, window_start_ns, cgroup_id)` already clusters data, but a skip index reduces granule reads when predicates target specific workloads.

## Decision

Add a secondary skip index on `statix.workload_metrics`:

```sql
INDEX cgroup_idx cgroup_id TYPE minmax GRANULARITY 4
```

- **Type `minmax`:** stores min/max `cgroup_id` per granule; queries with `cgroup_id = ?` or bounded ranges can skip non-overlapping blocks.
- **`GRANULARITY 4`:** one index entry per four granules — balances index size vs. selectivity for hour-partitioned FinOps windows.
- Placed in `deploy/clickhouse/01_init.sql` after column definitions, before `ENGINE`.

## Consequences

- **Positive:** Faster cgroup-scoped reads at scale; no application or RowBinary path changes.
- **Negative:** Slightly more storage and merge work on the index; new volumes only via `CREATE TABLE`; existing volumes need `ALTER TABLE ... ADD INDEX`.
- **Operational:** Dev reset: `docker compose down -v && make compose-up`. Prod: run ALTER from init script comment block.

## References

- [ADR 007](../007-clickhouse-mergetree-tuning.md) — MergeTree tuning
- [ADR 011](../011-replacingmergetree-dedupe-identity.md) — sort key / billing `FINAL`
- [ADR 027](../027-api-read-path-clickhouse.md) — read API over `workload_metrics`

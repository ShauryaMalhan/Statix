# ADR 026: ClickHouse `finops` database init (Target 2)

**Status:** Accepted  
**Date:** 2026-06-04  
**Context:** Target 2 data engineering. Former duplicate `infra/clickhouse/init.sql` was removed; `deploy/clickhouse/01_init.sql` is the only init script ([ADR 007](007-clickhouse-mergetree-tuning.md), [ADR 011](011-replacingmergetree-dedupe-identity.md), [ADR 018](018-phase-roadmap-status.md)).

## Decision

**Single canonical script:** `deploy/clickhouse/01_init.sql` (merged former infra + deploy definitions).

1. `CREATE DATABASE finops`
2. **`statix.workload_metrics`** — `ReplacingMergeTree()`, `PARTITION BY` day, 30d `TTL`, `ORDER BY (node, window_start_ns, cgroup_id)`, columns aligned with `FlatRow` in `ingest.rs` (`k8s_resolved Bool` for JSONEachRow).
3. **`statix.kafka_telemetry_queue`** — Kafka engine; `kafka_skip_broken_messages = 1000`; `kafka_num_consumers = 1` (raise in prod).
4. **`finops.telemetry_mv`** — `SELECT *` into `workload_metrics`.

- **docker-compose** mounts this file (not `infra/clickhouse/init.sql`).
- **Removed** duplicate `infra/clickhouse/init.sql`; pointer in `infra/clickhouse/README.md`.

## Rationale

- One schema for dev and prod; no drift between Compose and K8s.
- Retains ADR 007/008 tuning (partition, TTL, skip broken messages).

## Consequences

- **Positive:** Billing `SELECT … FROM statix.workload_metrics FINAL` everywhere.
- **Negative:** Existing volumes with old `default.finops_telemetry` tables need `docker compose down -v` or manual migration.

## References

- `finops-api/src/routes/ingest.rs` (`FlatRow`)
- [ADR 008](008-clickhouse-kafka-engine-resilience.md)

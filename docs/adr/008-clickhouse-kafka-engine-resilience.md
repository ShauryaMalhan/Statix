# ADR 008: ClickHouse Kafka engine resilience and throughput

**Status:** Accepted  
**Date:** 2026-05-30  
**Context:** `finops.kafka_telemetry_queue` in `deploy/clickhouse/01_init.sql` consumes `JSONEachRow` from topic `finops-telemetry`. Cloud networks can drop packets (black-hole routes); bad rows can poison the consumer.

## Decision

Kafka engine `SETTINGS` on `finops.kafka_telemetry_queue`:

- **`kafka_skip_broken_messages = 1000`** — skip malformed JSON per block instead of halting the consumer on the first bad row.
- **`kafka_num_consumers = 1`** — local Docker (single partition). **Production:** set to match Kafka topic partition count (e.g. `8`) so ClickHouse consumes partitions in parallel.

Storage target (`finops.workload_metrics`) — `ReplacingMergeTree` + sort key — [ADR 007](007-clickhouse-mergetree-tuning.md), [ADR 011](011-replacingmergetree-dedupe-identity.md).

## Rationale

- **Poison pill:** ClickHouse Kafka consumers stop on parse errors by default; one bad API row freezes dashboards.
- **Single-thread default:** Without `kafka_num_consumers`, only one partition is consumed at a time under multi-partition topics.

## Consequences

- **Positive:** Ingest pipeline survives occasional wire-format bugs; production can scale with partition count.
- **Negative:** Skipped rows are lost silently (up to threshold per block) — monitor CH logs / row counts vs Kafka lag.
- **Ops:** `IF NOT EXISTS` does not alter existing tables — `docker compose down -v` after `init.sql` changes.
- **Code:** `deploy/clickhouse/01_init.sql`

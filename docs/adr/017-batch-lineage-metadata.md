# ADR 017: Batch lineage metadata (`batch_id`, `agent_version`)

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Financial audits require tracing ClickHouse rows to a specific agent flush ([TODO 4.6](../../.cursor/skills/finops-ebpf-agent/TODO.md)). Retries and `ReplacingMergeTree` dedupe by `(node, window_start_ns, cgroup_id)` — lineage fields are audit metadata, not billing identity.

## Decision

Every aggregator flush assigns:

- `batch_id`: `uuid::Uuid::new_v4()` (one ID per HTTP batch / flush)
- `agent_version`: `env!("CARGO_PKG_VERSION")` from `finops-user`

Wire path: `BatchPayload` → `BatchJson` → `POST /ingest` `IngestBatch` → `FlatRow` → Kafka JSONEachRow → ClickHouse `finops_telemetry`.

## Rationale

- Unique `batch_id` links all workload rows from one flush for support and audit queries.
- `agent_version` correlates schema/behavior with a deployed agent binary.
- Fields are **not** in `ORDER BY` — duplicate retries for the same window still collapse on merge ([ADR 011](011-replacingmergetree-dedupe-identity.md)).

## Consequences

- **Positive:** `SELECT * FROM finops_telemetry FINAL WHERE batch_id = '…'` traces a single agent emission.
- **Negative:** Schema change — existing ClickHouse volumes need `docker compose down -v && make compose-up`.
- **Negative:** Manual curl tests must include `batch_id` and `agent_version` in the ingest body.

## References

- `finops-user/src/aggregator.rs`, `output.rs`
- `finops-api/src/routes/ingest.rs`
- `infra/clickhouse/init.sql`

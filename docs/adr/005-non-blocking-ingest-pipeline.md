# ADR 005: Non-blocking HTTP → Kafka ingest pipeline

**Status:** Accepted (amended 2026-06-01)  
**Date:** 2026-05-28  
**Context:** Phase 3 must persist telemetry without stalling the eBPF ring buffer drain or the ingest HTTP handler.

## Decision

Three-tier non-blocking pipeline:

1. **Agent (`finops-user`)** — `emit_batch` serializes JSON, enqueues to retry worker (`try_send`, bounded 60) — [ADR 006](006-shared-http-client-for-ingest.md). Caller never awaits HTTP. K8s refresh and cgroupfs memory reads must not block the main `select!` loop (`tokio::spawn` + `spawn_blocking`).
2. **API (`finops-api`)** — `POST /ingest` hard-gates `schema_version == 2` (`400` otherwise); denormalizes each workload row, `mpsc::try_send((Vec<u8>, Vec<u8>))` to bounded channel (env, default 8192 — [ADR 014](014-kafka-producer-env-tuning.md)); node key once per batch ([ADR 010](010-kafka-partition-key-by-node.md)). `200 OK` when all rows enqueue; first `try_send` failure → `503`. `GET /metrics` — [ADR 012](012-finops-api-prometheus-metrics.md).
3. **Kafka** — micro-batch (`recv_many`, env linger/batch_max); group by partition; `produce()` per partition sub-batch — [ADR 014](014-kafka-producer-env-tuning.md).

Infrastructure: Kafka KRaft → ClickHouse `Kafka` engine table → materialized view → **`ReplacingMergeTree`** ([ADR 007](007-clickhouse-mergetree-tuning.md), [ADR 011](011-replacingmergetree-dedupe-identity.md)). No Rust consumer.

## Rationale

- FinOps correctness: agent stall = missed exec/memory signals during storms.
- API stall under slow Kafka would backpressure agents if we used blocking `send` or awaited produce in handlers.
- Raw `JSONEachRow` per message matches ClickHouse schema; no ORM layer.

## Consequences

- **Positive:** Sub-millisecond handler path; predictable agent CPU; agent retries + CH dedupe reduce double-billing.
- **Negative:** Under sustained overload, API returns `503`; partial enqueue before `503` possible until `batch_id` on wire ([TODO](../../../.cursor/skills/finops-ebpf-agent/TODO.md) 4.6).
- **Ops:** `make compose-up` — [ADR 009](009-finops-api-docker-compose.md). Agent: `FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest`.
- **Graceful shutdown:** `with_graceful_shutdown` → drop ingest `tx` → Kafka task drains mpsc → flush; `producer.shutdown()` capped at **10s**.
- **Health:** `GET /health` → `200` if `kafka_tx` open; `503` if producer task died.
- **Deferred:** TLS, ingest auth — see `TODO.md`.

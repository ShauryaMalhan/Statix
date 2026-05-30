# ADR 005: Non-blocking HTTP → Kafka ingest pipeline

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 3 must persist telemetry without stalling the eBPF ring buffer drain or the ingest HTTP handler.

## Decision

Three-tier non-blocking pipeline:

1. **Agent (`finops-user`)** — `emit_batch` serializes JSON, `tokio::spawn` + shared `reqwest::Client` (`OnceLock`). Caller never awaits HTTP.
2. **API (`finops-api`)** — `POST /ingest` denormalizes each workload row, `mpsc::try_send(Bytes)` to bounded channel (1024). Always `200 OK` on accept; channel full → warn + drop row.
3. **Kafka** — micro-batch (`recv_many`, 5ms linger); hoisted `payloads` buffer; one `Vec<Record>` alloc per batch via `collect()` then `produce()` (ownership cannot be recycled).

Infrastructure: Kafka KRaft → ClickHouse `Kafka` engine table → materialized view → `MergeTree`. No Rust consumer.

## Rationale

- FinOps correctness: agent stall = missed exec/memory signals during storms.
- API stall under slow Kafka would backpressure agents if we used blocking `send` or awaited produce in handlers.
- Raw `JSONEachRow` per message matches ClickHouse schema; no ORM layer.

## Consequences

- **Positive:** Sub-millisecond handler path; predictable agent CPU.
- **Negative:** Under sustained overload, rows may drop at API channel (logged); no at-least-once guarantee to agent without retry (deferred).
- **Ops:** Requires Docker stack or equivalent Kafka + ClickHouse for Phase 3 local dev.
- **Graceful shutdown:** `with_graceful_shutdown` (SIGTERM/SIGINT) → drop ingest `tx` → Kafka task `recv_many` drains mpsc directly into hoisted `payloads` (no scratch vec / append copy) → `produce_batch` for full and partial batches (ECS deploy).
- **Deferred:** TLS, ingest auth, agent retry queue — see `TODO.md`.

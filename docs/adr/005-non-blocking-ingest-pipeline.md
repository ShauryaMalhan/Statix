# ADR 005: Non-blocking HTTP → Kafka ingest pipeline

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 3 must persist telemetry without stalling the eBPF ring buffer drain or the ingest HTTP handler.

## Decision

Three-tier non-blocking pipeline:

1. **Agent (`finops-user`)** — `emit_batch` serializes JSON, `tokio::spawn` + shared `reqwest::Client` (`OnceLock`). Caller never awaits HTTP.
2. **API (`finops-api`)** — `POST /ingest` denormalizes each workload row, `mpsc::try_send(Bytes)` to bounded channel (1024). Always `200 OK` on accept; channel full → warn + drop row.
3. **Kafka** — dedicated `tokio` task owns `rskafka::PartitionClient`, `recv()` loop + `produce().await`.

Infrastructure: Kafka KRaft → ClickHouse `Kafka` engine table → materialized view → `MergeTree`. No Rust consumer.

## Rationale

- FinOps correctness: agent stall = missed exec/memory signals during storms.
- API stall under slow Kafka would backpressure agents if we used blocking `send` or awaited produce in handlers.
- Raw `JSONEachRow` per message matches ClickHouse schema; no ORM layer.

## Consequences

- **Positive:** Sub-millisecond handler path; predictable agent CPU.
- **Negative:** Under sustained overload, rows may drop at API channel (logged); no at-least-once guarantee to agent without retry (deferred).
- **Ops:** Requires Docker stack or equivalent Kafka + ClickHouse for Phase 3 local dev.
- **Deferred:** TLS, ingest auth, agent retry queue — see `TODO.md`.

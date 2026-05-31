# ADR 005: Non-blocking HTTP → Kafka ingest pipeline

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 3 must persist telemetry without stalling the eBPF ring buffer drain or the ingest HTTP handler.

## Decision

Three-tier non-blocking pipeline:

1. **Agent (`finops-user`)** — `emit_batch` serializes JSON, `tokio::spawn` + shared `reqwest::Client` (`OnceLock`, 3s timeout — [ADR 006](006-shared-http-client-for-ingest.md)). Caller never awaits HTTP. K8s refresh and cgroupfs memory reads must not block the main `select!` loop (`tokio::spawn` + `spawn_blocking`).
2. **API (`finops-api`)** — `POST /ingest` hard-gates `schema_version == 2` (`400` otherwise); denormalizes each workload row, `mpsc::try_send(Bytes)` to bounded channel (1024). `200 OK` when all rows enqueue; first `try_send` failure → `503` with plain-text body (handler never awaits Kafka).
3. **Kafka** — micro-batch (`recv_many`, 5ms linger); hoisted `payloads` buffer; one `Vec<Record>` alloc per batch via `collect()` then `produce()` (ownership cannot be recycled).

Infrastructure: Kafka KRaft → ClickHouse `Kafka` engine table → materialized view → `MergeTree`. No Rust consumer.

## Rationale

- FinOps correctness: agent stall = missed exec/memory signals during storms.
- API stall under slow Kafka would backpressure agents if we used blocking `send` or awaited produce in handlers.
- Raw `JSONEachRow` per message matches ClickHouse schema; no ORM layer.

## Consequences

- **Positive:** Sub-millisecond handler path; predictable agent CPU.
- **Negative:** Under sustained overload, API returns `503` (no silent accept); partial enqueue before `503` possible until `batch_id` dedupe ([TODO](../../../.cursor/skills/finops-ebpf-agent/TODO.md) Phase 4). Agent retry/backoff deferred.
- **Ops:** `make compose-up` ships Kafka, ClickHouse, and `finops-api` — [ADR 009](009-finops-api-docker-compose.md). Agent on host: `FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest`.
- **Graceful shutdown:** `with_graceful_shutdown` (SIGTERM/SIGINT) → drop ingest `tx` → Kafka task drains mpsc → `produce_batch` flush; `producer.shutdown()` capped at **10s** (deploy must not hang on dead broker).
- **Health:** `GET /health` → `200` if `kafka_tx` open; `503` if producer task died (`mpsc` receiver dropped).
- **Deferred:** TLS, ingest auth, agent retry queue — see `TODO.md`.

# ADR 055: Phase 13 Part 1 — Kafka removal, direct ClickHouse RowBinary ingest

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** [PHASE_13_PART1_PLAYBOOK.md](../../.cursor/skills/statix-ebpf-agent/PHASE_13_PART1_PLAYBOOK.md) — queue-less architecture pivot. Kafka was the shock absorber between gateway and ClickHouse; with Phase 11 agent WAL + circuit breaker, the gateway becomes the terminal buffer with honest backpressure.

## Decision

### Schema (`deploy/clickhouse/01_init.sql`)

- Drop `statix.telemetry_mv` then `statix.kafka_telemetry_queue` (consumer before source).
- `statix.workload_metrics` unchanged — `ReplacingMergeTree(window_end_ns)` absorbs batched RowBinary inserts; no `async_insert`.

### Gateway writer (`statix-gateway/src/clickhouse_writer.rs`)

- Replace `kafka.rs` with mpsc coalescer (`STATIX_CH_LINGER_MS`, `STATIX_CH_BATCH_MAX`) drained by one worker.
- RowBinary insert via `clickhouse::Client::insert("statix.workload_metrics")` with gateway-local `MetricRow` (`#[derive(Row)]`).
- Synchronous `insert.end()` wrapped in `STATIX_CH_INSERT_TIMEOUT_SECS` (default 3s, < agent 5s HTTP timeout) — flips `ch_healthy`.
- Reuses read-path `ch_client` connection pool.

### Gateway state + ingest (`main.rs`, `routes/ingest.rs`)

- `AppState`: `ingest_tx: mpsc::Sender<FlatRow>`, `ch_healthy: Arc<AtomicBool>`.
- Tier 1: `!ch_healthy` → instant `503` (`statix_api_ch_unhealthy_reject_total`).
- Tier 2: `try_reserve_many` → `Full` → `503`.
- `/ready`: `ch_healthy` + mpsc &lt;80% gate.

### Dependencies

- Delete `kafka.rs`; remove `rskafka`, `chrono`, `rustc-hash` from `statix-gateway/Cargo.toml`.
- Env: `STATIX_INGEST_CHANNEL_SIZE` (replaces `STATIX_KAFKA_CHANNEL_SIZE`).

## Rationale

- ClickHouse degrades on many small parts — micro-batch coalescing is mandatory.
- `async_insert` would ACK before durability and hide stall signal needed to trip agent circuit breaker → WAL.
- Without a broker, buffering doomed rows in RAM converts CH stalls into agent data loss on mpsc overflow.

## Consequences

- **Positive:** Simpler stack; synchronous stall detection; agent WAL absorbs edge shock; shared CH client pool.
- **Negative:** Gateway is terminal buffer — must fast-fail `503`; compose/K8s still reference Kafka until Part 2.
- **Cancelled ops:** Phase 5 `kafka_num_consumers`, Kafka retention, CH Kafka-engine lag alerting.

## References

- [ADR 005](../005-non-blocking-ingest-pipeline.md) — superseded ingest path (historical)
- [ADR 054](../phase11/054-phase11-wal-spillway.md) — agent WAL + circuit breaker
- [TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md) — Phase 13

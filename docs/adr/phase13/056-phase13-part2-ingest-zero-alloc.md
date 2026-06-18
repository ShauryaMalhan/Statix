# ADR 056: Phase 13 Part 2 — Ingest zero-alloc collapse (single `MetricRow`)

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** [PHASE_13_PART2_PLAYBOOK.md](../../.cursor/skills/statix-ebpf-agent/PHASE_13_PART2_PLAYBOOK.md) — Part 1 ([ADR 055](055-phase13-part1-kafka-removal-rowbinary.md)) shipped the mpsc coalescer + RowBinary writer but retained a double-buffer: `FlatRow` in `statix-wire` and `MetricRow` in the gateway, with a transient `Vec<FlatRow>` in the ingest handler and a second `Vec<MetricRow>` at flush time.

## Decision

### Collapse to one gateway-local type

- **`MetricRow`** (`statix-gateway/src/clickhouse_writer.rs`) is the sole insert-ready row type. It derives `clickhouse::Row` and must not live in `statix-wire` (would drag `clickhouse` into the agent).
- **`MetricRow::from_ingest(&IngestBatch, &WorkloadRow)`** builds rows inline in the ingest handler.
- **Remove `FlatRow`**, `FlatRow::from_ingest`, and `IngestBatch::into_flat_rows` from `statix-wire` — zero callers after collapse.

### Ingest handler (`routes/ingest.rs`)

- Reserve `try_reserve_many(batch.workloads.len())` first; build each `MetricRow` directly into a permit.
- No transient `Vec<FlatRow>` between reserve and send.
- 3-tier 503 backpressure, schema gate, lag histogram unchanged.

### Writer pipeline (`clickhouse_writer.rs`)

- Channel element type: `MetricRow` end-to-end (`ChWriter.tx`, `fill_batch`, `drain_final`, `flush_with_retry`).
- `flush_with_retry` inserts the coalescer batch directly — no `From<FlatRow>` conversion pass.

### AppState (`main.rs`)

- `ingest_tx: mpsc::Sender<clickhouse_writer::MetricRow>`.

### Coalescer retained (non-negotiable)

The mpsc is **not** replaced with a per-request `clickhouse::inserter`. One insert per HTTP request would create one ClickHouse part per agent batch → merge storms at scale. The coalescer fuses rows across requests into large parts ([ADR 055](055-phase13-part1-kafka-removal-rowbinary.md)).

### Idempotency invariant (unchanged)

Sort key `(node, window_start_ns, cgroup_id)` + version column `window_end_ns` on `ReplacingMergeTree`. Collapsing allocation paths does not alter any key/version field; WAL replays and 503 retries still dedupe via `FINAL`.

### Deferred stretch

Per-row `String` clone of `node` / `batch_id` / `agent_version` remains (inherent to RowBinary per-row columns). Optional future: `Arc<str>` envelope fields on `MetricRow` after verifying `clickhouse` 0.13 RowBinary serializes `Arc<str>` correctly.

## Consequences

- **Positive:** Materializations per batch drop from 3 → 1 (coalescer batch only); no redundant struct pair; `statix-wire` surface reduced to live protocol types (`IngestBatch`, `WorkloadRow`).
- **Negative:** Envelope strings still cloned once per workload row at ingest (acceptable; `Arc<str>` stretch documented below).
- **Neutral:** Wire JSON protocol, channel semantics, `/ready` gates, and agent build graph unchanged.
- **Follow-up:** Infra strip completed in [ADR 057](057-phase13-part2-infra-kafka-strip.md).

## References

- [ADR 055](055-phase13-part1-kafka-removal-rowbinary.md) — queue-less RowBinary ingest
- [ADR 054](../phase11/054-phase11-wal-spillway.md) — at-least-once + WAL replay
- [ADR 011](../011-replacingmergetree-dedupe-identity.md) — dedup identity

# Phase 11 — Local Disk WAL Spillway — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Each item: **What** / **Why** / **How**. Run `cargo check --workspace` after each.
> One ADR for the wave: [docs/adr/phase11/054-phase11-wal-spillway.md](../../../docs/adr/phase11/054-phase11-wal-spillway.md).
> Priority: P0 = data loss, P1 = resource exhaustion, P2 = perf/edge-case.

**Status:** Shipped ✅ (ADR 054).

## Problem

The agent's only durability buffer was an in-memory Tokio mpsc capped at
`RETRY_QUEUE_CAPACITY = 60` windows (`statix/src/output.rs`). On a 429/5xx or a
full queue, `enqueue_batch_json` synchronously **dropped the oldest batch**.
During a prolonged gateway outage / network partition this silently discarded
billing telemetry, violating the FinOps zero-data-loss requirement.

## Design

A bounded **segmented append-only WAL** sits between the in-memory retry queue
and permanent loss. Disk is the overflow spillway; loss only occurs at the disk
hard cap, is FIFO-ordered and metered. Delivery is **at-least-once** — ClickHouse
`ReplacingMergeTree` (`node, window_start_ns, cgroup_id`) de-duplicates replays.

```
emit_batch → retry mpsc (try_send)
                └─ Full / circuit Open → WAL writer channel (try_send, non-blocking)
                                            └─ dedicated OS thread: append + fdatasync
WAL drainer (gateway healthy) → retry mpsc → POST /ingest
```

---

## P11-1 (P0) Segmented WAL on-disk format — `statix/src/wal/segment.rs`
What: CRC32-framed, length-prefixed append-only segments; 16B magic header.
Why: at-least-once durability under outage without OOM; torn-tail detectable.
How: frame `[u32 len][u32 crc32][u64 batch_seq][payload]`; `seg-<seq>.wal`;
     rotate at `STATIX_WAL_SEGMENT_BYTES`; stack 16B header buffer; the
     `bytes::Bytes` JSON is written verbatim (zero-copy).

## P11-2 (P0) Dedicated writer thread + group commit — `statix/src/wal/writer.rs`
What: single `std::thread` owns the active segment; `fdatasync` group-commit.
Why: keep ALL disk I/O off the ring-buffer hot path; avoid `spawn_blocking`
     pool starvation; one owner of the active fd.
How: bounded `mpsc` → thread via `blocking_recv` + drain `try_recv`; `fdatasync`
     (not `fsync`) per `STATIX_WAL_FSYNC_FRAMES` / `STATIX_WAL_FSYNC_INTERVAL_MS`
     and always at end of each drain batch (durability); `posix_fadvise(DONTNEED)`
     on consumed/rotated segments; on write error truncate to last good frame.

## P11-3 (P0) Hot-path overflow routing — `statix/src/output.rs`
What: `enqueue_batch_json` Full branch → `WAL.try_append` (try_send), not drop.
Why: eliminate silent billing-data loss on queue saturation / circuit Open.
How: `try_send` to the WAL writer channel; only if WAL is disabled or its
     channel is also full, fall back to the legacy sync drop-oldest.

## P11-4 (P0) Bootstrap recovery / self-heal — `statix/src/wal/recovery.rs`
What: validate segments, truncate torn tail, drop corrupt segments, rebuild
     head/tail purely from the surviving segment set.
Why: survive SIGKILL/reboot/corruption without crash loops or replaying garbage.
How: scan `seg-*.wal` by seq; validate magic/version; CRC+len walk; `set_len`
     at first invalid frame; superblock is advisory (not trusted). Runs in
     `main.rs` before the writer/drainer start.

## P11-5 (P1) Circuit breaker + drainer — `statix/src/wal/drainer.rs`, `mod.rs`
What: `Closed/HalfOpen/Open` (AtomicU8) driven by retry-worker POST outcomes;
     `tokio::spawn` drainer replays WAL → retry queue.
Why: low-overhead health detection (no steady-state polling); ordered backlog
     flush that never overruns the in-memory buffer.
How: `record_post_success/failure` from `output.rs` POST arms; Open routes
     overflow straight to WAL; drainer probes `try_half_open` staggered by a
     node-hash spread; replays only while the retry queue has capacity.

## P11-6 (P1) Hard cap + ENOSPC handling — `statix/src/wal/segment.rs`, `mod.rs`
What: `STATIX_WAL_MAX_BYTES` bound; drop-oldest-segment on cap; ENOSPC caught.
Why: bounded disk on ephemeral volumes; never fill the volume, never panic.
How: `enforce_cap` deletes oldest segment + `statix_wal_dropped_{batches,bytes}_total`;
     write `io::Error` (ENOSPC) → `statix_wal_write_errors_total`, truncate to
     last good frame, drop the batch (bounded loss).

## P11-7 (P1) Metrics + boot/shutdown — `statix/src/main.rs`, `output.rs`
What: register `statix_wal_*` metrics on `:9091`; `init_wal` at boot.
Why: observability of spillway depth/loss; per-batch fdatasync guarantees
     anything spilled is durable across an orderly stop.
How: gauges/counters/histogram per design; `output::init_wal(&node)` after
     `init_retry_worker`, before ingestion.

## Metrics (`:9091`, `statix_*`)

| Metric | Type | Meaning |
|--------|------|---------|
| `statix_wal_bytes_current` | gauge | bytes currently on disk |
| `statix_wal_segments_current` | gauge | segment count |
| `statix_wal_frames_written_total` | counter | frames appended |
| `statix_wal_frames_replayed_total` | counter | frames replayed to gateway |
| `statix_wal_dropped_batches_total` | counter | frames lost at hard cap |
| `statix_wal_dropped_bytes_total` | counter | bytes lost at hard cap |
| `statix_wal_corrupt_frames_total` | counter | corrupt frames/segments at recovery |
| `statix_wal_write_errors_total` | counter | append/fdatasync errors (e.g. ENOSPC) |
| `statix_wal_fsync_seconds` | histogram | fdatasync latency |
| `statix_wal_circuit_state` | gauge | 0=Closed, 1=HalfOpen, 2=Open |

## Env vars

| Var | Default | Meaning |
|-----|---------|---------|
| `STATIX_WAL_ENABLED` | `true` | disable with `0/false/no/off` (→ legacy drop-oldest) |
| `STATIX_WAL_DIR` | `/var/lib/statix/wal` | segment directory |
| `STATIX_WAL_MAX_BYTES` | `536870912` (512 MiB) | hard disk cap (≥ one segment) |
| `STATIX_WAL_SEGMENT_BYTES` | `8388608` (8 MiB) | segment rotation size |
| `STATIX_WAL_FSYNC_FRAMES` | `64` | max frames between fdatasync in a burst |
| `STATIX_WAL_FSYNC_INTERVAL_MS` | `200` | max time between fdatasync in a burst |

## Verification

```bash
make check       # cargo check across workspace (incl. wal module)
make wal-test    # cargo test -p statix wal  (frame CRC, torn-tail, rotation,
                 #   hard-cap, crash replay count_in==count_out, circuit breaker)
make wal-faultfs # root: tmpfs ENOSPC degradation test (no panic, stays usable)
```

## Execution order

P11-1 → P11-2 → P11-4 → P11-3 → P11-5 → P11-6 → P11-7

# ADR 054: Phase 11 — Local disk WAL spillway for the agent

**Status:** Accepted  
**Date:** 2026-06-14  
**Context:** Hardening the agent for prolonged gateway outages / network partitions ([PHASE_11_WAL_PLAYBOOK.md](../../../.cursor/skills/statix-ebpf-agent/PHASE_11_WAL_PLAYBOOK.md)). The only durability buffer was the in-memory retry mpsc (60 windows); on saturation `output::enqueue_batch_json` dropped the oldest batch, silently losing billing telemetry (FinOps zero-data-loss violation).

## Decision

### P11-1 — Segmented append-only WAL (`statix/src/wal/segment.rs`)

- On-disk format: `seg-<seq>.wal` files with a 16-byte magic/version header, then
  CRC32-framed records `[u32 len][u32 crc32(payload)][u64 batch_seq][payload]`.
- The pre-serialized `bytes::Bytes` JSON payload is written verbatim (zero-copy);
  both headers are built on the stack (no per-frame heap allocation).
- **Chosen over** SQLite (malloc + btree/WAL/checkpoint write amplification +
  blocking exclusive-lock model) and an mmap ring (msync latency spikes; `ENOSPC`
  surfaces as uncatchable SIGBUS; torn-page detection is hard). A sequential log
  gives ~1× write amplification, FIFO semantics matching the mpsc, and catchable
  `io::Error` on disk-full.

### P11-2 — Dedicated writer thread + group commit (`statix/src/wal/writer.rs`)

- A single `std::thread` (`statix-wal-writer`) owns the active segment fd and is
  the only writer. It drains a bounded `tokio::mpsc` (`blocking_recv` + `try_recv`)
  and group-commits with `fdatasync` (not `fsync`) bounded by
  `STATIX_WAL_FSYNC_FRAMES` / `STATIX_WAL_FSYNC_INTERVAL_MS`, always syncing at the
  end of a drain batch. `posix_fadvise(DONTNEED)` releases consumed/rotated pages.
- Dedicated thread (not `spawn_blocking`) avoids Tokio blocking-pool starvation.

### P11-3 — Hot-path overflow routing (`statix/src/output.rs`)

- `enqueue_batch_json`'s `Full` branch now `try_send`s to the WAL writer channel
  (`Wal::try_append`); only if the WAL is disabled or its channel is also full does
  it fall back to the legacy synchronous drop-oldest. No disk I/O on the hot path.

### P11-4 — Bootstrap recovery / self-heal (`statix/src/wal/recovery.rs`)

- At boot (before writer/drainer start), enumerate segments by seq, validate the
  header (drop segments with a corrupt header), walk frames validating length +
  CRC, and `set_len`-truncate a torn tail (expected after SIGKILL/power-loss) or a
  corrupt remainder. Head/tail cursors are rebuilt from the surviving segment set;
  the advisory superblock is intentionally not trusted.

### P11-5 — Circuit breaker + drainer (`statix/src/wal/mod.rs`, `drainer.rs`)

- `CircuitState` (`Closed`/`HalfOpen`/`Open`, `AtomicU8`) is driven by the retry
  worker's POST outcomes (`record_post_success` / `record_post_failure`) — no
  steady-state health polling. A `tokio::spawn` drainer replays WAL → retry queue
  oldest-first while the queue has headroom; recovery probes are staggered by a
  node-hash spread to avoid a fleet-wide thundering herd.

### P11-6 — Hard cap + ENOSPC (`statix/src/wal/segment.rs`, `mod.rs`)

- `STATIX_WAL_MAX_BYTES` (≥ one segment) bounds disk; on cap, the oldest segment is
  deleted (`statix_wal_dropped_{batches,bytes}_total`). A write `io::Error` (ENOSPC)
  increments `statix_wal_write_errors_total`, truncates back to the last good frame,
  and drops the batch — bounded loss, never a panic.

### P11-7 — Metrics + boot wiring (`statix/src/main.rs`, `output.rs`)

- `output::init_wal(&node)` runs after `init_retry_worker` and before ingestion.
  Ten `statix_wal_*` series are exported on `:9091` (see playbook table).

## Rationale

- Delivery is **at-least-once**: a frame durable on disk but already delivered
  pre-crash is replayed and de-duplicated by ClickHouse `ReplacingMergeTree`
  (`node, window_start_ns, cgroup_id`), so crash-replay correctness needs no
  agent-side dedup.
- The hot-path latency contract is preserved: appends are a non-blocking
  `try_send`; all disk I/O is on the dedicated writer thread.

## Consequences

- **Positive:** No silent telemetry loss on outages up to the disk cap; bounded,
  metered, FIFO loss only at the hard cap; self-healing across crashes; disk usage
  hard-bounded on ephemeral volumes.
- **Negative:** New `crc32fast` dependency; at-least-once means occasional
  duplicate batches on the wire (absorbed by `ReplacingMergeTree FINAL`); a
  pathological single oversized frame can exceed `segment_bytes` by design (forward
  progress > strict cap).
- **Unchanged:** Wire format / `schema_version`; ring-buffer drain still has no
  `.await`; legacy drop-oldest remains as the last-resort bound and when
  `STATIX_WAL_ENABLED=0`.

## References

- [PHASE_11_WAL_PLAYBOOK.md](../../../.cursor/skills/statix-ebpf-agent/PHASE_11_WAL_PLAYBOOK.md) — implementation playbook + verification
- [ADR 051](../phase55/v3/051-phase55-v3-wave3-distributed-state.md) — V3-15 recovery node-hash spread (reused for WAL probe staggering)
- [enterprise-latency.md](../../guides/enterprise-latency.md) — durability & back-pressure principles
- `statix/src/wal/` — `segment.rs`, `writer.rs`, `recovery.rs`, `drainer.rs`, `mod.rs`

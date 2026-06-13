# ADR 053: Phase 5.5 V3 Wave 5 — micro-architecture polish

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** L8/L9 Post-GA audit Wave 5 ([L8_POST_GA_FIXES.md](../../../../.cursor/skills/statix-ebpf-agent/L8_POST_GA_FIXES.md)) — final P2 polish: BPF readability, ring-buffer safety, poll cadence, flush allocations.

## Decision

### V3-16 — Named `BPF_RB_NO_WAKEUP` constant (`statix-ebpf/src/main.rs`)

- `const BPF_RB_NO_WAKEUP: u64 = 1` replaces magic `1` in ring submit wakeup suppression.

### V3-17 — `StatixEvent` alignment assertion (`statix/src/main.rs`)

- Compile-time `assert!(align_of::<StatixEvent>() <= 8)` before ring-buffer pointer cast.

### V3-18 — Ring poll interval 5ms (`statix/src/main.rs`)

- `poll_interval` increased from 1ms to 5ms; wakeup suppression still fires every 64th BPF event.

### V3-3 — `BatchPayload.node` as `Arc<str>` (`statix/src/aggregator.rs`, `output.rs`)

- `flush()` uses `Arc::from(node)`; JSON boundary in `emit_batch` performs single `String` conversion for `IngestBatch`.

## Rationale

- Magic submit flags obscure kernel ABI contract and complicate review.
- Misaligned ring records would be UB on cast; assertion catches struct layout drift at compile time.
- 1ms poll burns CPU on idle nodes; 5ms still bounds worst-case 64-event gap latency.
- Per-flush `node.to_string()` duplicated heap work; `Arc<str>` defers copy until serialize boundary.

## Consequences

- **Positive:** Clearer BPF; safer agent drain; lower idle poll overhead; one node string shape through flush path.
- **Negative:** `Arc::from(&str)` still allocates per flush unless caller caches shared `Arc` — acceptable P2 step; `V3-5-extra` fd pool remains deferred.
- **Unchanged:** Hot-path ring drain still no `.await`; ingest wire format still `String` node at HTTP boundary.

## References

- [ADR 052](052-phase55-v3-wave4-perf-observability.md) — Wave 4
- [ADR 038](../v2/038-phase55-v2-wave1-l8-fixes.md) — V2-9 BPF wakeup suppression
- [TODO.md](../../../../.cursor/skills/statix-ebpf-agent/TODO.md) — V3-16, V3-17, V3-18, V3-3

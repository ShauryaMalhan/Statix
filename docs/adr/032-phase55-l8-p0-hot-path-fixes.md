# ADR 032: Phase 5.5 L8 audit — P0-SHIP agent hot-path fixes

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** L8 audit — seven P0-SHIP bottlenecks on the agent ingest hot path before production. Original fix IDs F1–F5, F7, F8; removed from [L8-AUDIT-FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8-AUDIT-FIXES.md) after ship (playbook retains only open work, renumbered F1–F7).

## Decision

| Fix | File | Change |
|-----|------|--------|
| F1 | `output.rs` | `IS_HTTP_INGEST` `OnceLock<bool>` — no `env::var` per flush |
| F2 | `aggregator.rs` | Thread-local `SmallRng` + `fast_batch_id()` — no `Uuid::new_v4()` syscall |
| F3 | `aggregator.rs` | `BatchPayload::agent_version: &'static str` — single `.to_string()` at JSON boundary |
| F4 | `aggregator.rs` + `attribution.rs` | `pub DEFAULT_LABELS`; `WorkloadStats::default` clones shared `Arc` |
| F5 | `output.rs` + `main.rs` | `emit_batch(BatchPayload)` move semantics — no field/workload clones |
| F7 | `memory_sampler.rs` | One `spawn_blocking` per tick for all cgroup reads |
| F8 | `main.rs` | `DRAIN_BUDGET = 256` on ring-buffer drain — yield to `select!` |

**Dependency:** `rand` feature `small_rng` in `statix/Cargo.toml`.

## Rationale

- Eliminates ~500 allocations per flush (F5), env lock per flush (F1), getrandom per flush (F2), and N blocking tasks per sample tick (F7).
- Ring budget prevents flush/memory-sample starvation during exec storms (F8).

## Consequences

- **Positive:** Agent hot path aligned with [enterprise-latency.md](../enterprise-latency.md) mechanical sympathy targets.
- **Negative:** `batch_id` uses non-crypto RNG after one-time OS seed — acceptable for correlation ([ADR 017](017-batch-lineage-metadata.md)).
- **Deferred (now shipped):** P1-WEEK in [ADR 033](033-phase55-l8-p1-week-gateway-fixes.md). L8 playbook retains F1 (`Arc<[u8]>` node key) only.

## References

- [L8-AUDIT-FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8-AUDIT-FIXES.md)
- [ADR 023](023-phase5-hot-path-fixes.md)

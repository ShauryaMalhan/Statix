# ADR 022: BPF ring buffer drop counter (`RING_DROPS`)

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 5 P0 — `EVENTS.reserve()` failure dropped events with no visibility ([phase5-production-readiness.md](../phase5-production-readiness.md)).

## Decision

1. **Kernel (`finops-ebpf`):** `RING_DROPS` — `BPF_MAP_TYPE_PERCPU_ARRAY`, key `u32`, value `u64`, `max_entries: 1`. On `EVENTS.reserve` failure, increment key `0` on the current CPU via `get_ptr_mut(0)`.
2. **Agent (`finops-user`):** `take_map("RING_DROPS")` after load; `tokio` task every 10s sums per-CPU values. If total &gt; 0: `log::error!(...)` and `metrics::counter!("finops_agent_ring_drops_total").absolute(total)`.

## Consequences

- **Positive:** Silent drops become observable; per-CPU counter avoids map lock contention on the hot path.
- **Negative:** Cumulative counter (not rate); rebuild all three eBPF bundle variants after BPF changes.
- **Ops:** Re-run `make build-ebpf` before `make run`; alert on the error log or wire Phase 10 Prometheus recorder to the metrics counter.

## References

- [TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md) Phase 5 P0
- `finops-ebpf/src/main.rs`, `finops-user/src/loader.rs`

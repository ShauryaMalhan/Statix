# ADR 022: BPF ring buffer drop counter (`RING_DROPS`)

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 5 P0 — `EVENTS.reserve()` failure dropped events with no visibility ([phase5-production-readiness.md](../phase5-production-readiness.md)).

## Decision

1. **Kernel (`finops-ebpf`):** `RING_DROPS` — `BPF_MAP_TYPE_PERCPU_ARRAY`, key `u32`, value `u64`, `max_entries: 1`. On `EVENTS.reserve` failure, increment key `0` on the current CPU via `get_ptr_mut(0)`.
2. **Agent (`finops-user`):** `take_map("RING_DROPS")` after load; `tokio` task every 10s sums per-CPU values. If total &gt; 0: `log::error!(...)` and `metrics::counter!("finops_agent_ring_drops_total").absolute(total)`.
3. **Prometheus export:** `metrics-exporter-prometheus` HTTP listener on `0.0.0.0:9091` ([ADR 023](023-phase5-hot-path-fixes.md)).

## Consequences

- **Positive:** Silent drops become observable; per-CPU counter avoids map lock contention on the hot path.
- **Negative:** Cumulative counter (not rate); rebuild all three eBPF bundle variants after BPF changes.
- **Ops:** Re-run `make build-ebpf` before `make run`; scrape `http://<node>:9091/metrics` or alert on error log.

## References

- [TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md) Phase 5 P0
- [ADR 023](023-phase5-hot-path-fixes.md)
- `finops-ebpf/src/main.rs`, `finops-user/src/loader.rs`, `finops-user/src/main.rs`

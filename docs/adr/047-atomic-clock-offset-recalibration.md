# ADR 047: Atomic background clock-offset recalibration (NTP drift)

**Status:** Accepted  
**Date:** 2026-06-07  
**Context:** [ADR 016](016-clock-domain-offset.md) calibrated `clock_offset_ns` once at `Aggregator::new`. Long-running bare-metal / AI nodes see NTP steps and VM suspend/resume; a static offset drifts from true wall time and skews billing windows.

## Decision

1. **Global state** — `static CLOCK_OFFSET_NS: AtomicU64` in `statix-infra/src/clock.rs`.
2. **Startup** — `init_clock_offset()` stores `wall_unix_ns - mono_now_ns()` before the hot path runs.
3. **Hot path** — `clock_offset_ns()` loads the atomic with `Ordering::Relaxed`; `mono_to_wall(mono)` = `mono + clock_offset_ns()`. No `SystemTime::now()` on ring-buffer drain.
4. **Background** — Tokio task in `statix/src/main.rs` calls `recalibrate_clock_offset()` every `STATIX_CLOCK_RECALIBRATE_SECS` (default **3600**).
5. **Aggregator** — removed per-instance `clock_offset_ns` field; always reads global atomic.

## Rationale

- **Relaxed atomic load** is sufficient: offset changes at most once per hour; a slightly stale load during recalibration is sub-second billing noise.
- **Background-only syscalls** keep the eBPF drain loop allocation-free and lock-free.
- **Hourly default** balances NTP correction vs idle CPU; tunable via env without recompile.

## Alternatives considered

| Approach | Why not |
|----------|---------|
| Re-calibrate on every flush | Extra `clock_gettime` + wall syscall per window — unnecessary on flush path |
| `SystemTime::now()` per event | Unacceptable on ring-buffer hot path |
| `seqlock` / `RwLock` | Atomic load is faster and drift tolerance does not need strict ordering |
| Put Tokio task in `statix-infra` | Couples infra lib to async runtime; agent already owns Tokio |

## Consequences

- **Positive:** Billing timestamps track NTP-adjusted wall clock on multi-month uptimes.
- **Positive:** Hot path cost: one relaxed atomic load per timestamp conversion (unchanged vs field load).
- **Negative:** Sub-second discontinuity possible at recalibration instant (logged when offset changes).

## References

- [ADR 016](016-clock-domain-offset.md) — original offset model
- `statix-infra/src/clock.rs`, `statix/src/aggregator.rs`, `statix/src/main.rs`

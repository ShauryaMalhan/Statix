# ADR 069: Prime the CPU baseline at startup; first window is a full window

**Status:** Accepted  
**Date:** 2026-09-25  
**Amends:** [ADR 058](058-phase14-cpu-usage-tracking.md) — "first read per cgroup sets baseline only (delta 0)" still holds for cgroups that appear later, but no longer for the first window after startup. [ADR 067](067-sample-inside-flush.md) — its side-effect note that tokio's first `tick()` fires immediately no longer applies. Both otherwise unedited.

## Context

`cpu.stat`'s `usage_usec` is a running total since the cgroup was created, like an odometer. CPU used in a window is the difference between two readings, so the first reading of any cgroup only sets a baseline (ADR 058).

At startup that meant the first window **always reported 0 CPU**. `time::interval` fires its first tick immediately, so the agent sampled (baseline only) and closed the first window a few milliseconds after starting:

```
agent starts ─┬─ tick fires at once: reading 1 (baseline) → window 1 closes, CPU = 0, ~0 s long
              └─ +10 s: reading 2 → window 2 has real CPU
```

## Decision

Two changes, which only work together:

1. **`Sampler::prime`** reads every leaf cgroup's `cpu.stat` once at startup, after `bootstrap_existing_cgroups` registers them, and stores it as the baseline. It adds nothing to the window. Same leaf check and `spawn_blocking` as `tick`; a cgroup that can't be read simply primes on its first normal tick, as before.
2. **The flush timer's first tick is one full window after startup**: `time::interval_at(Instant::now() + window, window)` instead of `time::interval(window)`.

**Why priming alone is not enough:** with an immediate first tick, window 1 would carry a small but real CPU delta over a window only milliseconds long. The dashboard computes millicores as CPU ÷ window length, so that divides by almost nothing and produces absurd spikes. The window has to be full-length for its CPU number to mean anything.

### Verified

Dev VM, agent started 23:07:47 UTC (binary built 28 s earlier). First window after start: **10 s long, 3,053,090 µs CPU, 1 sample**, in line with the windows after it (2.93–3.05 M µs, about 300m). Before: about 0.0 s and 0 CPU.

## Consequences

- **Positive:** every window the agent writes is full-length and carries memory and CPU, including the first. No special case for readers.
- **Negative:** after a restart, the first data arrives **one window later** (10 s by default) instead of immediately. The dashboard keeps showing the previous data, or its existing "No workloads reporting" empty state on a fresh machine. `dev-up.sh` waits up to 30 s for data, so it is unaffected.
- **Cost:** one extra `cpu.stat` read per leaf cgroup, once, at startup, on the blocking pool.
- **Unchanged:** cgroups that first appear *after* startup (new containers, new sessions) still report 0 CPU in their first window. They are primed by their first normal tick, exactly as ADR 058 describes. Fixing that would need an out-of-band read when a cgroup is registered, which is not worth it for one window per new workload.

## References

- [ADR 058](058-phase14-cpu-usage-tracking.md) — CPU delta and priming
- [ADR 066](066-sample-leaf-cgroups-only.md) — leaf-only sampling (`prime` uses the same check)
- [ADR 067](067-sample-inside-flush.md) — one sample per window, taken inside the flush step
- tokio docs — `time::interval` ("the first tick completes immediately") vs `time::interval_at`

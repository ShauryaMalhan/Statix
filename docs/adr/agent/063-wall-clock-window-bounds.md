# ADR 063: Read the wall clock for window bounds; remove the cached offset

**Status:** Accepted  
**Date:** 2026-09-22  
**Supersedes:** [ADR 016](016-clock-domain-offset.md) (clock domain offset), [ADR 047](047-atomic-clock-offset-recalibration.md) (atomic background recalibration). Both remain as the record of what was decided then and are deliberately unedited.

## Context

BPF stamps every event with `bpf_ktime_get_ns()`, which is `CLOCK_MONOTONIC`. ClickHouse needs wall-clock time. ADR 016 bridged the two by calibrating `offset = wall − monotonic` once at startup, caching it in an `AtomicU64`, and converting with `monotonic + offset`. ADR 047 added an hourly task to recalibrate for NTP drift.

**The premise was sound.** A clock read per event, at thousands of events per second, is real repeated work on a path whose contract is never to block. Caching a difference and adding it is the obvious answer.

**The premise stopped matching the code.** Reading `aggregator.rs` showed the per-event conversion had exactly one consumer:

```rust
let wall_timestamp = self.mono_to_wall(event.timestamp);
...
log::trace!("event kind={} ... wall_timestamp_ns={wall_timestamp}");
```

A `trace!` that is off in normal operation. Nothing per-event reached ClickHouse. The timestamps that did — `window_start_ns` and `window_end_ns` — were computed inside `flush()`, **twice per window**, off the hot path.

So the optimisation was paying for itself on a path that no longer existed, while being load-bearing on one where it saved nothing.

## The failure it caused

A cached offset is only valid while both clocks advance together. When the host **pauses** — a laptop lid, a hypervisor pause, a live migration — `CLOCK_MONOTONIC` freezes with the machine. On resume NTP steps the wall clock forward; monotonic is never corrected. The cached offset is then too small by exactly the pause duration.

Observed three times on 2026-09-22, most clearly in k3s:

- agent `Running`, 0 restarts, writing **63 rows / 12 s**
- every row stamped **3492 s (58 minutes)** in the past
- every "last 5 minutes" query returned nothing; the dashboard showed 0 workloads on a completely healthy pipeline

The guest never observes a suspend event under a hypervisor pause — measured `CLOCK_MONOTONIC == CLOCK_BOOTTIME`, so `bpf_ktime_get_boot_ns()` would not have helped either. Independently corroborated by VM wall-age 26.7 h against monotonic uptime 19.35 h: **7.3 hours of wall time the monotonic clock never counted.**

ADR 047's hourly recalibration does eventually correct it, but rows written in the meantime stay **permanently mis-stamped in ClickHouse**, and nothing emits a signal that it happened. Cost attribution reads those timestamps.

## Decision

### Window bounds read the system clock directly

`Aggregator::new`, `flush()` and `reset_window` call `wall_unix_ns()` (`SystemTime::now()`). No derived value, therefore nothing that can go stale.

Measured on the dev VM (aarch64, `-O2`, 5M iterations):

| Operation | Cost |
|---|---|
| atomic load (the cached offset) | **1.31 ns** |
| `CLOCK_REALTIME` read (wall clock) | **14.02 ns** |

The clock read is ~11x the atomic and still 14 nanoseconds. Linux serves it through the vDSO, so it is not a syscall. Against the work `flush()` already does in the same call — walking up to 4096 workloads, cloning label strings, allocating a `Vec`, serialising to JSON, sending over HTTP — it is roughly **0.001% of one flush**.

### One clock reading per flush, not two

`flush()` takes a single reading and passes it to `reset_window`, so one window ends exactly where the next begins. Previously two readings a few microseconds apart left a small hole between consecutive ranges. Windows now tile with no gap and no overlap. The empty-buffer early return takes its own reading, since it produces no batch and therefore has no end bound to share.

### The offset subsystem is removed, not merely bypassed

Deleted: the `CLOCK_OFFSET_NS` static, `calibrate_clock_offset_ns`, `init_clock_offset`, `clock_offset_ns`, `recalibrate_clock_offset`, `mono_now_ns`, `Aggregator::mono_to_wall`, `Aggregator::wall_now_ns`, `spawn_clock_recalibration_task`, and the `log::trace!` that was its last consumer. `STATIX_CLOCK_RECALIBRATE_SECS` no longer exists. `statix-infra` loses its now-unused `libc` dependency.

Leaving it dead would not have been neutral. `clock_offset_ns()` *looks* like the correct way to turn a BPF timestamp into wall time, and it had two ADRs vouching for it — a future reader would reasonably reach for it and reintroduce the bug. `statix-infra/src/clock.rs` keeps a module comment explaining why the offset is absent.

## Consequences

- **Positive:** the failure mode is eliminated rather than reduced — there is no cached value left to go stale, so pause duration no longer matters. `clock.rs` drops from 63 lines to 16; 93 lines removed across three files. One fewer background task. Windows tile exactly.
- **Negative — and this is the real cost:** per-event wall timestamps are no longer available. Nothing needs them today, and the only thing that did was a disabled trace log. **If that changes, this decision must be revisited** — at 10,000 events/s a clock read per event is ~140 µs/s of pure clock reading. Probably still acceptable, but it is a different calculation than the one made here, and it should be made deliberately rather than by assuming this ADR covers it.
- **Diagnostic loss:** the `wall_timestamp_ns` trace line is gone. If BPF timestamp debugging is needed again, log the raw monotonic value — it is what the kernel actually provided, and is more honest for ordering questions.
- **Verified:** 41 tests pass; clean build with zero warnings. Real-world test — laptop shut down overnight, VM paused ~12 hours, agent reported `newest 1s ago` on resume. Before the fix the same pause produced a 58-minute skew that persisted until the hourly tick.

## The general lesson

> An optimisation is scoped to the path it was written for. When the code moves, the optimisation does not move with it — it stays, still paying its costs, no longer earning them.

Caching here was correct for a per-event path and wrong for one that runs twice per ten seconds. The same cached value served both, so the wrong half decided the behaviour.

## References

- [ADR 016](016-clock-domain-offset.md) — original clock domain offset (superseded)
- [ADR 047](047-atomic-clock-offset-recalibration.md) — hourly recalibration (superseded)
- [ADR 003](003-early-flush-instead-of-cap-eviction.md) — early flush at `max_keys`, which is why a window is not always `window_secs` long and why every row carries its own bounds
- Commits `fbe1e0b` (fix), this change (removal)

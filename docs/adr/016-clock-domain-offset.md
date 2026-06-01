# ADR 016: Clock domain offset (BPF monotonic → wall)

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Identity events stamp `timestamp` with `bpf_ktime_get_ns()` (monotonic/boot-time domain). The aggregator used `SystemTime` for `window_start_ns` / `window_end_ns`, so event times and billing windows lived in different domains ([TODO 4.1](../../.cursor/skills/finops-ebpf-agent/TODO.md)).

## Decision

At `Aggregator::new`, calibrate once:

```text
clock_offset_ns = wall_unix_ns - CLOCK_MONOTONIC_ns
```

- Store `clock_offset_ns` on `Aggregator`.
- For each ring-buffer event: `wall_timestamp = event.timestamp + clock_offset_ns` before any timestamp math.
- Window boundaries (`window_start_ns`, `window_end_ns`, `reset_window`) use `mono_now_ns() + clock_offset_ns` so flush intervals stay in the same domain as converted BPF timestamps.

User-space memory samples already pass wall time from `memory_sampler`; they do **not** add the offset again.

## Rationale

- ClickHouse / billing expect Unix-epoch nanoseconds on batch windows.
- A single offset at agent start avoids comparing raw BPF monotonic values to wall-only flush timestamps.
- Monotonic + fixed offset is stable across NTP steps during a long-lived agent run (wall-only `SystemTime` for window ends would jump after NTP).

## Consequences

- **Positive:** BPF events and aggregator windows share one calibrated wall domain.
- **Negative:** Offset is not refreshed after NTP adjustments; long uptimes may drift from true UTC (acceptable until periodic recalibration is added).
- **Negative:** `CLOCK_MONOTONIC` is an approximation of `bpf_ktime_get_ns()` (boot-time); sub-second skew possible on some kernels.

## References

- `finops-user/src/aggregator.rs`, `finops-ebpf/src/main.rs`

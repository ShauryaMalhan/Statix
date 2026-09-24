# ADR 067: Sample inside the flush step — one reading per window

**Status:** Accepted  
**Date:** 2026-09-24  
**Amends:** [ADR 058](058-phase14-cpu-usage-tracking.md) — its operational note recommending `STATIX_SAMPLE_INTERVAL_SECS <= STATIX_WINDOW_SECS`. That variable no longer exists. ADR 058 is otherwise unedited.

## Context

The agent's main loop ran two independent timers:

```rust
_ = flush_interval.tick()  => { close the window, send it }
_ = sample_interval.tick() => { read memory.current + cpu.stat into the window }
```

Both defaulted to 10 s and were created together, so they became ready **at the same instant**. When more than one branch is ready, `tokio::select!` picks one **at random**. So each period, the sample landed before the window closed or after it, by coin flip:

```
window 1:  [ sample ]          normal
window 2:  [        ]          sample fell into the next window
window 3:  [ sample sample ]   two samples: 20 s of CPU in a 10 s window
```

- **A window with no sample** still holds rows for cgroups that ran a program (exec events), but with 0 memory and 0 CPU. The dashboard shows each cgroup's *latest* window, so busy cgroups (k3s, Docker, the user's shell) read zero for one window and totals visibly dropped. Seen 2026-09-24: memory 5.5 → 1.8 GiB and CPU 300m → 100m, then back.
- **A window with two samples** carries two CPU deltas, so millicores (CPU ÷ window length) read **double**.

Nothing was lost overall, since CPU deltas still summed correctly over time. But every per-window number was unreliable, and per-window numbers are what the dashboard and any billing query read.

## Decision

**Sample inside the flush step: read, then close the window**, in one `select!` arm. The separate sample timer and `STATIX_SAMPLE_INTERVAL_SECS` are removed; the agent samples exactly once per window, just before it closes.

```rust
_ = flush_interval.tick() => {
    for batch in sampler.tick(&cache, &mut agg, &node).await { output::emit_batch(batch); }
    if let Some(batch) = agg.flush(&node, &cache) { output::emit_batch(batch); }
}
```

Two steps in one arm run in a fixed order, so there is no longer anything for `select!` to choose between. Early batches from the sampler (a window that hit `max_keys` mid-sample) are emitted before the regular flush, so ordering is preserved.

**Side effect, wanted:** tokio's first `tick()` fires immediately, so the first window after startup now carries a real memory reading instead of the zeros `bootstrap_existing_cgroups` seeds.

### Verified

On the dev VM after the change: every window since restart had `max(sample_count) = 1` — none with 0 or 2 — and CPU stayed in a 228–309m band across consecutive windows.

## Alternatives considered

- **Keep both timers, add `biased;` to `select!`** so flush always wins ties. Rejected: fixes the tie only while the two timers stay in phase. They drift apart under `MissedTickBehavior::Delay` whenever one arm runs long, and the race comes back as "sometimes 0, sometimes 2" at a different phase.
- **Keep `STATIX_SAMPLE_INTERVAL_SECS` but require it to equal the window.** Rejected: more code to protect a setting that can only have one valid value.
- **Sample faster than the window** (e.g. every 2 s) for a truer per-window memory peak. Not needed today; the defaults already sampled once per window, so `memory_bytes_max` already equalled `memory_bytes_last`. If a real peak is wanted, it belongs in a separate decision about sub-window sampling, not a second free-running timer.

## Consequences

- **Positive:** exactly one reading per window. CPU millicores no longer double or zero out, and memory tiles stop dropping and recovering.
- **Negative:** sampling rate is tied to the window. Anyone who set `STATIX_SAMPLE_INTERVAL_SECS` will find it **silently ignored**; it was undocumented outside the README env table and dev guides, and those are updated.
- **Hot path unaffected.** Sampling already ran in the main `select!` loop and already awaited `spawn_blocking`; it now runs in the flush arm instead of its own arm.
- **Not addressed here** (tracked in TODO.md):
  - **The shutdown flush still skips the sample.** The Ctrl-C / SIGTERM arms call `agg.flush` directly, so the last partial window is written unsampled, and exec'ing cgroups end on a 0-memory row until the agent restarts. Fix: share one "sample, then flush" function across all three arms.
  - **CPU is still 0 in the first window after start.** A rate needs two readings; the first one only primes the baseline (ADR 058).

## Follow-up (2026-09-24)

The shutdown gap listed above is closed. "Sample, then flush" now lives in one function, `sample_and_flush` in `statix/src/main.rs`, called by the flush timer **and** both shutdown arms. One function rather than three copies, because three copies is how the gap happened: the timer arm was fixed and the shutdown arms were not. Verified: after `dev-down.sh`, the final window shows `max(sample_count) = 1` and no unsampled rows.

## References

- [ADR 058](058-phase14-cpu-usage-tracking.md) — CPU delta and baseline priming
- [ADR 066](066-sample-leaf-cgroups-only.md) — leaf-only sampling (the fix that made this race visible)
- `tokio::select!` docs — "if multiple futures are ready, one is selected at random" unless `biased;`

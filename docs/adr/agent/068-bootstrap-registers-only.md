# ADR 068: Bootstrap registers cgroups only — no synthetic exec events

**Status:** Accepted  
**Date:** 2026-09-25  
**Amends:** [ADR 015](015-cgroup-v2-bootstrap-on-startup.md). Steps 4 and 5 are removed (inject a synthetic identity event per cgroup; emit early-flush batches). Steps 1–3 (walk, inode as `cgroup_id`, register paths) stand. ADR 015 is otherwise unedited.

## Context

eBPF only sees a workload when a process **execs**, so anything already running when the agent starts is invisible until it execs again. ADR 015 fixed that with a startup walk of `/sys/fs/cgroup` which, for every directory, did two things:

1. **registered** the cgroup's path, so the sampler knows where to read `memory.current` / `cpu.stat`
2. **injected a synthetic `StatixEvent`** (`EVENT_KIND_WORKLOAD_IDENTITY`) into the aggregator, so the workload had a row in the first window

In 2026-06 step 2 was needed. Sampling ran on its own timer, so the first window could close before any sample, and the synthetic event was the only thing that put pre-existing workloads in it.

Two later decisions changed that, and turned step 2 into a bug:

- **[ADR 066](066-sample-leaf-cgroups-only.md)** samples leaf cgroups only. Parent cgroups still got a synthetic row but never a sample, so they showed as **zero-value workloads** for the whole dashboard lookback.
- **[ADR 067](067-sample-inside-flush.md)** samples every window just before it closes, so every leaf gets a real row from its first sample and no longer needs the synthetic one.

And one thing was wrong from the start: the aggregator cannot tell a synthetic identity event from a real one, so it did `exec_count += 1` for each. **Every restart reported one phantom exec per cgroup** in its first window.

### Measured (dev VM, 2026-09-24)

First window after two restarts under the old code:

| window | `sum(exec_count)` | rows with `sample_count = 0` |
|---|---|---|
| 15:28:22 | **62** | **18** |
| 15:29:03 | **62** | **18** |

62 = 44 leaf cgroups (the dashboard's workload count at the time) + 18 parents: exactly one phantom exec per cgroup, and one zero row per parent. Normal windows show 0–26 real execs. After the fix, the first window after restart shows `execs 0`, `unsampled 0`.

## Decision

**`bootstrap_existing_cgroups` registers cgroups and does nothing else.** It no longer touches the aggregator, so its signature drops `agg` and `node`, and it returns `()` instead of early-flush batches.

The synthetic event had no other effect worth keeping. Its only other call, `cache.on_identity_event`, returns immediately, because `register_cgroup_directory` registered the same path one line earlier.

## Consequences

- **Positive:** no zero-value parent rows and no phantom execs after a restart. Simpler code: the synthetic `StatixEvent` literal and the early-flush plumbing are gone.
- **Neutral:** a pre-existing workload first appears in the first *sampled* window rather than the first window. Since ADR 067 those are the same window: tokio's first `tick()` fires immediately and samples before flushing.
- **Invariant:** only real exec events increment `exec_count`, so `exec_count` now means what its name says.
- **Not addressed here** (tracked in TODO.md):
  - **CPU is 0 in the first window after start.** A rate needs two readings; the first only primes the baseline (ADR 058).
  - **Occasional single unsampled row.** Seen three times in mid-run windows (1 row each, not at a restart): a cgroup that exec'd but was not sampled at window close. Unverified; most likely a short-lived cgroup (e.g. from `docker exec`) removed before the window closed.

## References

- [ADR 015](015-cgroup-v2-bootstrap-on-startup.md) — the bootstrap walk (amended)
- [ADR 066](066-sample-leaf-cgroups-only.md) — leaf-only sampling, which left parent rows unsampled
- [ADR 067](067-sample-inside-flush.md) — sample inside flush, which made the synthetic row redundant

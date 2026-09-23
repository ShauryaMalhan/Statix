# ADR 066: Sample leaf cgroups only

**Status:** Accepted  
**Date:** 2026-09-24  
**Amends:** [ADR 015](015-cgroup-v2-bootstrap-on-startup.md) (bootstrap registers every cgroup directory) and [ADR 058](058-phase14-cpu-usage-tracking.md) (CPU baseline lifetime). Both remain as written.

## Context

cgroups form a tree, and a parent's `memory.current` and `cpu.stat` **already include every descendant**. The sampler read every registered cgroup, and `bootstrap_existing_cgroups` registers every directory under `/sys/fs/cgroup`, parents included. So the same bytes were counted once per level of the tree, and every fleet-wide `sum()` was inflated by a factor that depends on tree depth.

It surfaced the moment K8s attribution started working: the dashboard reported **15 GiB** on a VM with **7.7 GiB** of RAM. Measured on the dev VM (2026-09-24):

```
sum of memory.current, every cgroup    15419.8 MiB   ← more than the machine has
sum of memory.current, leaves only      6800.4 MiB
RAM installed                           7921   MiB
```

The same bug made each Kubernetes pod appear 2–3 times: the pod-level cgroup (a parent) and its container cgroups (leaves) all resolved to the same labels.

No query could fix this. ClickHouse stores `cgroup_id` (an inode) and nothing else — no parent, no depth, no path — so the read side cannot tell a parent from a leaf.

## Decision

**The sampler reads only leaf cgroups** — ones with no child cgroups. Parents are skipped.

This is exact, not an approximation. cgroup v2's "no internal processes" rule means processes can only live in leaves, so a parent owns no memory or CPU of its own; its numbers are purely the sum of what is below it. Summing leaves counts every byte once.

### How "leaf" is detected

The cgroup filesystem (kernfs) sets a directory's link count to **2 + its number of subdirectories**. So `nlink == 2` means "no child cgroups": one `stat`, no directory listing, no allocation. Verified on the dev VM before relying on it: `user.slice` → 3, `session-4.scope` → 2, and `find -links 2` reproduced the leaf total above.

`is_leaf_cgroup` in `statix/src/memory_sampler.rs` runs inside the existing `spawn_blocking`, alongside the reads it gates. If the `stat` fails (the cgroup was deleted between snapshot and read) the cgroup is skipped for that tick.

### Checked every tick, not once at bootstrap

Leaf-ness changes. A slice that is empty at boot — e.g. `kubepods-besteffort.slice` before any best-effort pod is scheduled — is a leaf; the moment a pod lands inside it, it is a parent. A check only at bootstrap would bring the double-counting back for exactly the workloads that start after the agent.

### CPU baseline follows what was actually sampled

The CPU baseline map (`Sampler.cpu_baseline`, ADR 058) is now pruned against the cgroups **read this tick**, not the cgroups registered. Otherwise a cgroup that becomes a parent keeps its old baseline, and if it later becomes a leaf again its first delta spans the whole gap — one large, fake CPU spike. Pruning makes it re-prime, like a new cgroup. The prune happens only when the blocking task succeeds, so a failed tick does not wipe every baseline.

## Alternatives considered

- **Emit `parent_cgroup_id` / depth and filter at read time.** Keeps per-slice rollups available. Rejected for now: a wire + schema + gateway + query change, and every query must remember the filter or silently double-count again. Can be layered on later if slice rollups are needed.
- **Skip non-leaves in `bootstrap_existing_cgroups` only.** Rejected: misses cgroups that gain children later (above).
- **K8s-specific de-duplication** (drop pod rows when container rows exist). Rejected: fixes K8s only; `user.slice`, `system.slice` and Docker trees double-count the same way.
- **Hide fleet-wide totals.** Rejected as a fix; it hides the symptom and leaves the stored data wrong.

## Consequences

- **Positive:** totals are physically possible again. Each pod counts once. No wire, schema or gateway change.
- **Negative:** per-slice rollups (e.g. "all of `system.slice`") are gone from the stored data. They were never trustworthy summed alongside their children anyway.
- **Cost:** one extra `stat` per registered cgroup per tick, on the blocking pool, off the hot path.
- **Still registered:** bootstrap still registers parent cgroups and emits one identity row each at startup, with zero memory and CPU. Harmless to totals, but those rows count as workloads in the dashboard's lookback window. Tracked with the startup-zero issue in TODO.md.
- **Not addressed here:**
  - **Page cache.** `memory.current` includes file cache. On the dev VM, leaves summed to 1709 MiB `anon` + 4547 MiB `file`, against 2226 MiB `used` in `free`. That is a question of which definition of memory to bill, separate from double-counting, and tracked in TODO.md.
  - **The pause-container label.** The sandbox cgroup is its own leaf and its (small) memory is real, so it is no longer double-counted. But it is still labelled with the application container's name.

## References

- [ADR 015](015-cgroup-v2-bootstrap-on-startup.md) — bootstrap walk that registers every directory
- [ADR 058](058-phase14-cpu-usage-tracking.md) — CPU delta and baseline priming
- Linux `Documentation/admin-guide/cgroup-v2.rst` — "No Internal Process Constraint"
- Linux `fs/kernfs/inode.c` — `kernfs_refresh_inode`: `set_nlink(inode, kn->dir.subdirs + 2)`

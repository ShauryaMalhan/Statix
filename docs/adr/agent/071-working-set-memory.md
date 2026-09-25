# ADR 071: Report working-set memory, not `memory.current`

**Status:** Accepted  
**Date:** 2026-09-26  
**Serves:** Goal 1 — right-size pods by need ([docs/PRODUCT.md](../../PRODUCT.md))

## Context

The agent reported each leaf cgroup's `memory.current`. That file counts **everything** charged to the cgroup — including page cache: file contents Linux keeps in RAM only to make re-reads fast, and reclaims the moment a program needs the memory.

On the dev VM (2026-09-24, leaves only, from `memory.stat`): 1709 MiB `anon` (program memory) + 4547 MiB `file` (page cache) + ~540 MiB kernel = 6800 MiB `memory.current`, while `free` reported 2226 MiB `used`.

For a waste report that is the worst kind of wrong: a pod that needs 300 MiB but has read a lot of files looks like it "uses 2 GiB", so Statix would tell a company **not** to shrink it. The dashboard tile already said "current working set" — the label was right, the number was not.

## Decision

**Report working set = `memory.current − inactive_file`** — the definition the kubelet uses for `kubectl top` and for eviction decisions.

`inactive_file` is page cache nobody has used recently — the first thing the kernel reclaims. Working set keeps *active* file cache (files a program is using right now) on purpose: taking it away would slow the program down.

**Stored in the existing columns.** `memory_bytes_last` / `memory_bytes_max` now mean working set. Agent-only change: no wire, gateway or ClickHouse schema change ([ADR 070](../meta/070-product-scope-self-hosted-waste-report.md): stay light).

### Implementation

- `memory.stat`'s path is precomputed per cgroup and stored in `memory_stat_paths`, alongside `memory.current` and `cpu.stat` — never built per tick. Registered in both `on_identity_event` and `register_cgroup_directory`, removed in `evict_stale_cgroups`.
- `read_inactive_file_at` reads `memory.stat` into a 4 KB stack buffer and returns `Option<u64>`. A missing file or field is a **soft miss**: the sampler falls back to `memory.current` alone (subtracts 0), as the kubelet does.
- `current.saturating_sub(inactive)`: the two files are read at slightly different moments, so `inactive_file` can briefly exceed `memory.current`.

### Verified

Dev VM, 2026-09-25 22:23 UTC:

| | total, leaf cgroups |
|---|---|
| before (`memory.current`), last windows of the old agent | 5.70–5.73 GiB |
| after (working set), first window of the new agent | **3.66 GiB** |
| computed directly from cgroupfs a few seconds later | **3.69 GiB** |
| `free -m` `used`, for reference | 2.48 GiB |

The agent matches the kernel. Per workload the drop differs as expected: a mostly-idle-cache cgroup went 1.7 GiB → 967 MiB, while one actively using its files barely moved (801 → 753 MiB). Working set stays above `free`'s `used` because it keeps active cache, by design.

## Alternatives considered

- **`anon` only** (program memory, no cache at all). Rejected: undercounts workloads that genuinely depend on their files being in RAM (databases, search indexes), and no Kubernetes tool reports it, so Statix would disagree with `kubectl top`.
- **Add new `working_set_bytes_*` columns, keep `memory.current`.** Rejected for now: wire schema v4, gateway, ClickHouse `ALTER`, and dashboard queries, to keep a number the report does not use. Revisit if cache pressure ever becomes a report feature.
- **Keep `memory.current`.** Rejected: see Context.

## Consequences

- **Positive:** memory numbers mean "what this workload needs" and agree with `kubectl top`. The right-sizing goal can now compare memory against requests/limits honestly.
- **Negative:** raw `memory.current` is no longer stored.
- **Mixed history:** rows written before 2026-09-25 22:23 UTC on the dev stack hold `memory.current`; rows after hold working set. Both live in the same columns until the 30-day TTL ages the old ones out. Dev data only; no customer data existed.
- **Cost:** one extra file read per leaf cgroup per window, on the blocking pool, stack buffer only.

## References

- [ADR 066](066-sample-leaf-cgroups-only.md) — leaf-only sampling
- [ADR 067](067-sample-inside-flush.md) — one sample per window
- [ADR 070](../meta/070-product-scope-self-hosted-waste-report.md) — product scope and "stay light"
- Kubernetes kubelet / cAdvisor — `working_set = usage − inactive_file`

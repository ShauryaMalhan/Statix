# ADR 015: Bootstrap existing cgroup v2 workloads on agent startup

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** eBPF identity events only fire on `sched_process_exec`. Long-running pods/DBs started before the agent never appear in the aggregator or memory sampler until something new execs ([TODO 1.7](../../.cursor/skills/statix-ebpf-agent/TODO.md)).

## Decision

On startup, before the main `select!` loop, `attribution::bootstrap_existing_cgroups`:

1. Recursively walks `STATIX_CGROUP_ROOT` (default `/sys/fs/cgroup`) via `walkdir`.
2. For each **directory** (skip root), uses `metadata().ino()` as **`cgroup_id`** (cgroup v2 unified hierarchy).
3. Registers `memory.current` paths via `AttributionCache::register_cgroup_directory` (relative path `/slice/...`).
4. Injects synthetic `StatixEvent` (`EVENT_KIND_WORKLOAD_IDENTITY`, `timestamp: 0`, empty `comm`) into `Aggregator::on_finops_event`.
5. Emits any early-flush batch from `max_keys` during bootstrap.

## Rationale

- Billing must include already-running workloads, not only processes that exec after agent attach.
- Inode-as-id matches `bpf_get_current_cgroup_id()` semantics on cgroup v2.
- Path registration is required: bootstrap events have no PID, so `/proc/{pid}/cgroup` cannot be used.

## Consequences

- **Positive:** Memory sampler and rollups cover pre-existing cgroups immediately.
- **Negative:** Large hosts with deep cgroup trees add startup latency and may trigger early aggregator flushes.
- **Negative:** Leaf cgroups without `memory.current` still register (sampler skips unreadable files).

## References

- `finops-user/src/attribution.rs`, `main.rs`

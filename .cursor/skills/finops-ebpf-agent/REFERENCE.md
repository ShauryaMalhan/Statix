# FinOps eBPF Agent — Reference

## Overview (Phase 2)

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` + `cgroup_id` → `EVENTS` |
| Agent | AsyncFd drain → attribution → aggregator → batched JSON |
| Memory | Userspace sampler: `memory.current` per tracked cgroup every N seconds |

## File map

```
finops-core/
├── docs/phase2-validation.md
├── docs/phase3-ingest-interface.md
├── finops-common/src/lib.rs       # FinopsEvent, EVENT_KIND_*
├── finops-ebpf/src/main.rs        # tracepoint, EVENTS 512KiB
└── finops-user/src/
    ├── main.rs
    ├── loader.rs
    ├── attribution.rs
    ├── memory_sampler.rs
    ├── aggregator.rs
    └── output.rs
```

## Data flow

```
sched:sched_process_exec
  → finops_sched_process_exec
    → EVENTS.reserve::<FinopsEvent>
    → cgroup_id, pid, tgid, comm, timestamp
    → submit(0)
  → AsyncFd drain
    → attribution.on_identity (/proc/pid/cgroup path cache)
    → aggregator (per-window rollups)
  → memory_sampler (interval) → memory_bytes in aggregator
  → K8s refresh (30s, in-cluster) → pod/namespace labels
  → emit_batch schema_version 2
```

## Naming contract

| Artifact | Name |
|----------|------|
| BPF program | `finops_sched_process_exec` |
| Tracepoint | `sched` / `sched_process_exec` |
| Ring buffer | `EVENTS` |
| Struct | `FinopsEvent` |
| ELF path env | `FINOPS_EBF_PATH` |

## Deferred work

Track point-wise future optimizations in [TODO.md](TODO.md). Add a line there whenever perf or correctness work is intentionally postponed.

## Roadmap

| Phase | Status | Scope |
|-------|--------|-------|
| 1 | Done | execve kprobe, per-event JSON |
| 2 | **Done** | tracepoint, cgroup_id, memory samples, K8s labels, batched JSON |
| 3 | Planned | gRPC ingest → ClickHouse |
| 4–8 | Later | analyzer, GitOps, dashboard, GPU |

## Enterprise trust model

Unchanged: verifier sandbox, read-only probes, open BPF source, ring buffer telemetry without payload inspection.

## Operational notes

- cgroup v2 only; set `FINOPS_CGROUP_ROOT` if non-standard mount
- DaemonSet: privileged, host `/sys/fs/cgroup`, pod watch RBAC
- `make verify-btf` before shipping to diverse kernels
- **Memory sampler**: precomputed `memory.current` paths; stack-buffer read; one `sample_tick_ns` per interval
- **Attribution**: `parking_lot::RwLock`; cgroup v2 `0::/path` parsing; `Path::components()` for K8s UID/container extraction

---
name: finops-ebpf-agent
description: >-
  Enforces enterprise coding standards for the FinOps eBPF Agent (finops-core):
  three-crate workspace, BPF verifier rules, FinopsEvent ring buffer, Phase 2
  attribution and batched telemetry. Use when editing finops-common, finops-ebpf,
  finops-user; adding probes; or discussing K8s attribution, memory sampling,
  or stream-ready batches.
---

# FinOps eBPF Agent

Phase 2: tracepoint identity events with `cgroup_id`, cgroup memory sampling, K8s attribution, and `schema_version: 2` batched JSON.

## Quick start

```
Task progress:
- [ ] `FinopsEvent` / kind constants updated in finops-common only
- [ ] BPF map `EVENTS` name matches loader
- [ ] Tracepoint `finops_sched_process_exec` → sched:sched_process_exec
- [ ] reserve → fill all fields → submit(0) on every path
- [ ] User: attribution + aggregator + batched output
- [ ] make build && make check
```

## Workspace contract

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `finops-common` | host + bpf | `FinopsEvent`, event kind constants, `Pod` via `user` feature |
| `finops-ebpf` | `bpfel-unknown-none` | tracepoint, `bpf_get_current_cgroup_id`, ring buffer |
| `finops-user` | host | loader, attribution, memory_sampler, aggregator, output, main |

Modules: `loader.rs`, `attribution.rs`, `memory_sampler.rs`, `aggregator.rs`, `output.rs`, `main.rs`.

See [REFERENCE.md](REFERENCE.md), [PATTERNS.md](PATTERNS.md), and deferred work in [TODO.md](TODO.md).

## Shared memory contract

Ring record: **`FinopsEvent`** (64 bytes) with `kind`:

- `EVENT_KIND_WORKLOAD_IDENTITY` (1) — exec via `sched:sched_process_exec`
- `EVENT_KIND_MEMORY_SAMPLE` (2) — user-space `memory.current` sampler (and future BPF)

Legacy `ProcessEvent` remains for reference; Phase 2 uses `FinopsEvent` only.

## BPF verifier

- No `?` after `EVENTS.reserve`
- No `bpf_trace_printk`
- Always `submit(0)` or `discard(0)`
- Include `cgroup_id` from `bpf_get_current_cgroup_id()` on identity events

## User-space rules

- Batched stdout: `schema_version: 2` (see `output::emit_batch`)
- `FINOPS_RAW_EVENTS=1` for per-event debug only
- Do not block the event loop on K8s API — refresh on interval (`refresh_k8s_pods`)
- Memory: read `{FINOPS_CGROUP_ROOT}/<path>/memory.current`, not `/proc` polling
- Env: `FINOPS_WINDOW_SECS`, `FINOPS_SAMPLE_INTERVAL_SECS`, `FINOPS_NODE_NAME`, `FINOPS_CGROUP_ROOT`

### Hot-path heap discipline (`memory_sampler`, event loop)

The &lt;0.1% CPU goal applies to the **user daemon**, not only BPF. Polling loops must not allocate per iteration.

| Avoid in hot loops | Use instead |
|--------------------|-------------|
| `fs::read_to_string` on `memory.current` | `File::open` + `read` into `[u8; 32]` on the stack, then `trim` + `parse` |
| `PathBuf::join` per cgroup per tick | Precompute absolute `.../memory.current` in `AttributionCache::on_identity_event` (cold path) |
| `tracked_cgroup_ids()` → `Vec` per tick | `for_each_memory_current_path` callback over the cache map |
| Per-cgroup timestamps in one sample tick | Single `sample_tick_ns` at tick start — **intentional** for TSDB column batching |

`memory.current` is a tiny decimal file (&lt; 32 bytes). Heap churn on the sample path breaks the same overhead story as `/proc` polling in legacy APM agents.

### Aggregator (`aggregator.rs`)

| Rule | Detail |
|------|--------|
| Map | `rustc_hash::FxHashMap` keyed by `cgroup_id` (`u64`) — not `HashMap` (SipHash overkill) |
| Buffers | Two pre-sized maps; ping-pong on flush; `.clear()` keeps capacity |
| Cap | At `max_keys` → **early flush** — never `enforce_cap` / random key eviction |

### Attribution cache (`attribution.rs`)

| Rule | Detail |
|------|--------|
| Locks | `parking_lot::RwLock` for shared maps — not `std::sync::RwLock` on hot paths |
| `/proc/pid/cgroup` | Parse v2 lines as `0::/path` via `split_once("::")` — never `split_once(':')` (drops leading `/`) |
| Path parsing | `Path::components()` + `Component::Normal` — no `to_string_lossy()` over full paths |
| Precompute | `memory.current` absolute path stored on `on_identity_event` (cold path) |

## Build

```bash
make build
make run          # needs root
make verify-btf   # BTF present
```

Validation: [docs/phase2-validation.md](../../../docs/phase2-validation.md)  
ADRs (why hot paths look the way they do): [docs/adr/](../../../docs/adr/)  
Phase 3 ingest spec: [docs/phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)  
Deferred optimizations: [TODO.md](TODO.md) — add items when deferring perf/correctness work

## OOM-safe remediation (Phases 4–5)

```
requests = p99 × 1.20
limits   = requests × 1.25
```

See Pattern 7–8 in [PATTERNS.md](PATTERNS.md).

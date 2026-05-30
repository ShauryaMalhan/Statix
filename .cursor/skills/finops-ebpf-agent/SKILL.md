---
name: finops-ebpf-agent
description: >-
<<<<<<< HEAD
  Enforces enterprise coding standards for the FinOps eBPF Agent (finops-core):
  three-crate workspace, BPF verifier rules, FinopsEvent ring buffer, Phase 2
  attribution and batched telemetry. Use when editing finops-common, finops-ebpf,
  finops-user; adding probes; or discussing K8s attribution, memory sampling,
  or stream-ready batches.
=======
  Enterprise low-latency standards for the FinOps eBPF stack (finops-core):
  BPF ring buffer, Phase 2 attribution/aggregation, Phase 3 HTTP→Kafka→ClickHouse.
  Use when editing finops-common, finops-ebpf, finops-user, finops-api; adding probes;
  ingest, Docker infra, or ADRs. Always read this skill first, then build with make,
  and update docs/adr/skills in the same change.
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
---

# FinOps eBPF Agent

<<<<<<< HEAD
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
=======
**Enterprise goal:** &lt;0.1% node CPU at idle, **zero blocking** on kernel event drain, **no telemetry loss** on capacity signals.

Phases: **2 done** (batched agent) · **3 done** (ingest API + Kafka + ClickHouse)

## Mandatory workflow (every change)

1. Read [SKILL.md](SKILL.md) → [REFERENCE.md](REFERENCE.md) → [PATTERNS.md](PATTERNS.md)
2. Implement using patterns below (do not invent parallel conventions)
3. `make build && make check` (add `make verify-btf` if BPF/deploy changed)
4. **ADR** — new file in `docs/adr/` for architectural decisions ([enterprise-latency.md](../../../docs/enterprise-latency.md))
5. **Docs** — update README, phase validation, `phase3-ingest-interface.md` if contracts change
6. **Skills** — update this skill, REFERENCE, PATTERNS, TODO in the **same PR**
7. Deferred work → [TODO.md](TODO.md) only (open items; delete when shipped)

## Quick start checklist

```
- [ ] finops-common: FinopsEvent / kinds only here
- [ ] BPF: EVENTS map name matches loader; reserve → fill → submit(0)
- [ ] Agent: no await on ring-buffer path for HTTP/K8s blocking work
- [ ] Aggregator: FxHashMap, double buffer, early flush (never enforce_cap)
- [ ] Output: FINOPS_INGEST_URL → spawn + shared reqwest Client
- [ ] API: POST /ingest → try_send only; Kafka in background task
- [ ] make build && make check
- [ ] docs/adr + skills updated
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
```

## Workspace contract

| Crate | Target | Responsibility |
|-------|--------|----------------|
<<<<<<< HEAD
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
=======
| `finops-common` | host + bpf | `FinopsEvent`, kind constants, `Pod` via `user` feature |
| `finops-ebpf` | `bpfel-unknown-none` | tracepoint, `cgroup_id`, ring buffer |
| `finops-user` | host | loader, attribution, memory_sampler, aggregator, output, main |
| `finops-api` | host | `POST /ingest`, mpsc → Kafka (`rskafka`) |

**Infra:** `docker-compose.yml`, `infra/clickhouse/init.sql`

Modules: see [REFERENCE.md](REFERENCE.md).

## Latency contract (non-negotiable)

| Layer | Rule |
|-------|------|
| Ring buffer loop | No `.await` on HTTP ingest or blocking I/O |
| `emit_batch` | `tokio::spawn` + `OnceLock<reqwest::Client>` |
| `POST /ingest` | `mpsc::try_send`; always `200`; channel full → warn + drop row |
| Kafka | Background task only |
| Aggregator | Early flush at `max_keys`; flip buffer before drain |
| Memory sample | Stack `[u8; 32]` read; precomputed `memory.current` paths |

Full principles: [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## BPF verifier

- No `?` after `EVENTS.reserve`
- No `bpf_trace_printk`
- Always `submit(0)` or `discard(0)`
<<<<<<< HEAD
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
=======
- `cgroup_id` from `bpf_get_current_cgroup_id()` on identity events

## User-space (Phase 2)

- Batched JSON `schema_version: 2`
- `FINOPS_RAW_EVENTS=1` debug only
- K8s: interval refresh only — not in event loop
- Memory: precomputed `{CGROUP_ROOT}/…/memory.current`
- Env: `FINOPS_WINDOW_SECS`, `FINOPS_SAMPLE_INTERVAL_SECS`, `FINOPS_NODE_NAME`, `FINOPS_CGROUP_ROOT`

### Hot-path heap discipline

| Avoid | Use |
|-------|-----|
| `read_to_string` on `memory.current` | `File::read` into `[u8; 32]` |
| `PathBuf::join` per sample tick | Precompute path on `on_identity_event` |
| `Vec` of all cgroup IDs per tick | `for_each_memory_current_path` |
| `HashMap` for `cgroup_id` | `FxHashMap` ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md)) |

### Aggregator

| Rule | Detail |
|------|--------|
| Map | `rustc_hash::FxHashMap` |
| Buffers | Two maps; flip before drain ([ADR 004](../../../docs/adr/004-swap-buffer-before-drain.md)) |
| Cap | Early flush — never random eviction ([ADR 003](../../../docs/adr/003-early-flush-instead-of-cap-eviction.md)) |

### Attribution

| Rule | Detail |
|------|--------|
| Locks | `parking_lot::RwLock` |
| cgroup v2 | `split_once("::")` not `split_once(':')` |
| Paths | `Path::components()` — no full-path `to_string_lossy()` |

## Phase 3 ingest

| Component | Rule |
|-----------|------|
| Agent | `FINOPS_INGEST_URL` → fire-and-forget POST ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)) |
| API | Denormalize batch → one Kafka JSON row per workload |
| Stack | `make compose-up` (Docker required) |
| Storage | ClickHouse Kafka engine — no Rust consumer ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)) |
| CH schema | Daily parts, `ORDER BY (namespace, pod, node, time)`, 30d TTL ([ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md)) |

Spec: [docs/phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)  
Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md)

## Build (always via Makefile)

```bash
make deps          # first time
make build         # ebpf + finops-user + finops-api
make check
make verify-btf    # when BPF / kernel portability touched
make compose-up    # Phase 3 infra (Docker)
make run-api       # ingest :3000
sudo FINOPS_INGEST_URL=http://localhost:3000/ingest make run
```

Phase 2 validation: [docs/phase2-validation.md](../../../docs/phase2-validation.md)  
ADRs: [docs/adr/](../../../docs/adr/)  
Deferred: [TODO.md](TODO.md)
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## OOM-safe remediation (Phases 4–5)

```
requests = p99 × 1.20
limits   = requests × 1.25
```

<<<<<<< HEAD
See Pattern 7–8 in [PATTERNS.md](PATTERNS.md).
=======
See Pattern 8 in [PATTERNS.md](PATTERNS.md).
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

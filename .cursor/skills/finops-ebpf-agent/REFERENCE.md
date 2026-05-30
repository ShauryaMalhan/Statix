# FinOps eBPF Agent — Reference

<<<<<<< HEAD
## Overview (Phase 2)

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` + `cgroup_id` → `EVENTS` |
| Agent | AsyncFd drain → attribution → aggregator → batched JSON |
| Memory | Userspace sampler: `memory.current` per tracked cgroup every N seconds |
=======
Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → Kafka → ClickHouse.

**Principles:** [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role | Latency note |
|-------|------|----------------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` → `EVENTS` | μs; no blocking helpers |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` | No await on ingest |
| Ingest API | `POST /ingest` → `try_send` → background Kafka | Handler &lt; 1 ms target |
| Storage | Kafka `finops-telemetry` → CH Kafka engine → MergeTree | No Rust consumer |
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## File map

```
finops-core/
<<<<<<< HEAD
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
=======
├── README.md
├── docker-compose.yml
├── Makefile
├── infra/clickhouse/init.sql
├── docs/
│   ├── enterprise-latency.md
│   ├── phase2-validation.md
│   ├── phase3-ingest-interface.md
│   ├── phase3-validation.md
│   └── adr/
├── finops-common/src/lib.rs
├── finops-ebpf/src/main.rs
├── finops-user/src/
│   ├── main.rs, loader.rs, attribution.rs
│   ├── memory_sampler.rs, aggregator.rs, output.rs
├── finops-api/src/
│   ├── main.rs, kafka.rs
│   └── routes/ingest.rs
└── .cursor/skills/finops-ebpf-agent/
```

## Data flow (Phase 3)

```
sched:sched_process_exec
  → EVENTS ring buffer
  → AsyncFd drain → attribution → aggregator
  → emit_batch
       ├─ FINOPS_INGEST_URL unset → stdout JSON
       └─ FINOPS_INGEST_URL set  → tokio::spawn(POST /ingest)
            → finops-api try_send → Kafka produce (background)
                 → finops_telemetry_kafka → finops_mv → finops_telemetry
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
```

## Naming contract

| Artifact | Name |
|----------|------|
| BPF program | `finops_sched_process_exec` |
| Tracepoint | `sched` / `sched_process_exec` |
| Ring buffer | `EVENTS` |
| Struct | `FinopsEvent` |
<<<<<<< HEAD
| ELF path env | `FINOPS_EBF_PATH` |

## Deferred work

Track point-wise future optimizations in [TODO.md](TODO.md). Add a line there whenever perf or correctness work is intentionally postponed.
=======
| Kafka topic | `finops-telemetry` |
| Agent ELF env | `FINOPS_EBF_PATH` |
| Ingest env | `FINOPS_INGEST_URL` (e.g. `http://localhost:3000/ingest`) |
| API env | `KAFKA_BROKERS`, `FINOPS_API_PORT` |
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## Roadmap

| Phase | Status | Scope |
|-------|--------|-------|
<<<<<<< HEAD
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
=======
| 1 | Done | exec tracepoint, ring buffer, JSON |
| 2 | Done | cgroup_id, memory sampling, K8s labels, batched schema v2 |
| 3 | **Done** | HTTP ingest, Kafka, ClickHouse pipeline |
| 4–8 | Later | p99 analyzer, GitOps remediation, dashboard, GPU |

## Enterprise trust model

Verifier sandbox, read-only telemetry probes, open BPF source, ring buffer without payload inspection. Ingest path drops under overload rather than blocking agents ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)).

## Operational notes

- **Docker** required for Phase 3 local stack: `apt install docker.io docker-compose-v2`
- cgroup v2 only; `FINOPS_CGROUP_ROOT` if non-standard
- DaemonSet: privileged, host cgroup mount, pod watch RBAC
- `make verify-btf` before diverse kernel deploys
- Kafka internal listener: `kafka:29092`; host: `localhost:9092`
- ClickHouse `finops_telemetry`: daily partitions, sort `(namespace, pod, node, window_start_ns)`, 30d TTL — [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md)

## Deferred work

[TODO.md](TODO.md) — open items only.
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

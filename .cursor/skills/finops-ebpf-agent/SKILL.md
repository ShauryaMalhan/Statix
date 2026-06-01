---
name: finops-ebpf-agent
description: >-
  Enterprise low-latency standards for the FinOps eBPF stack (finops-core):
  BPF ring buffer, Phase 2 attribution/aggregation, Phase 3 HTTP→Kafka→ClickHouse.
  Use when editing finops-common, finops-ebpf, finops-user, finops-api; adding probes;
  ingest, Docker infra, or ADRs. Always read this skill first, then build with make,
  and update docs/adr/skills in the same change.
---

# FinOps eBPF Agent

**Enterprise goal:** &lt;0.1% node CPU at idle, **zero blocking** on kernel event drain, **no telemetry loss** on capacity signals.

Phases: **2 done** (batched agent) · **3 done** (ingest API + Kafka + ClickHouse)

## Mandatory workflow (every change)

1. Read [SKILL.md](SKILL.md) → [REFERENCE.md](REFERENCE.md) → [PATTERNS.md](PATTERNS.md)
2. Implement using patterns below (do not invent parallel conventions)
3. `make build && make check` (add `make verify-btf` if BPF/deploy changed)
4. **ADR** — new file in `docs/adr/` for architectural decisions ([enterprise-latency.md](../../../docs/enterprise-latency.md))
5. **Docs** — update README, phase validation, `phase3-ingest-interface.md` if contracts change
6. **Skills** — update this skill, REFERENCE, PATTERNS, TODO in the **same PR**
7. Deferred work → [TODO.md](TODO.md); mark shipped items `[x]` (keep the line)

## Quick start checklist

```
- [ ] finops-common: FinopsEvent / kinds only here
- [ ] BPF: EVENTS map name matches loader; reserve → fill → submit(0)
- [ ] Agent: no await on ring-buffer path for HTTP/K8s blocking work
- [ ] Aggregator: FxHashMap, double buffer, early flush (never enforce_cap)
- [ ] Output: FINOPS_INGEST_URL → `init_retry_worker` + shared reqwest (3s timeout, 90s pool idle)
- [ ] API: GET /health; GET /metrics; POST /ingest → 400/503/200; try_send; Kafka in background task
- [ ] make build && make check
- [ ] docs/adr + skills updated
```

## Workspace contract

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `finops-common` | host + bpf | `FinopsEvent`, kind constants, `Pod` via `user` feature |
| `finops-ebpf` | `bpfel-unknown-none` | tracepoint, `cgroup_id`, ring buffer |
| `finops-user` | host | loader, attribution, memory_sampler, aggregator, output, main |
| `finops-api` | host | `GET /health`, `GET /metrics`, `POST /ingest` → mpsc `(Bytes, Bytes)` keyed Kafka ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md), [ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md)) |

**Infra:** `docker-compose.yml`, `Dockerfile.api`, `infra/clickhouse/init.sql`

Modules: see [REFERENCE.md](REFERENCE.md).

## Shared memory contract

Ring record: **`FinopsEvent`** (64 bytes) with `kind`:

- `EVENT_KIND_WORKLOAD_IDENTITY` (1) — exec via `sched:sched_process_exec`
- `EVENT_KIND_MEMORY_SAMPLE` (2) — user-space `memory.current` sampler

## Latency contract (non-negotiable)

| Layer | Rule |
|-------|------|
| Ring buffer loop | No `.await` on HTTP ingest or blocking I/O |
| `emit_batch` | Serialize + `try_send` to retry worker queue; shared `reqwest` (3s timeout); backoff 1s→30s ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)) |
| `POST /ingest` | `schema_version == 2` or `400`; `try_send`; `200` or `503` on channel full |
| Kafka | Background task only |
| Aggregator | Early flush at `max_keys`; flip buffer before drain |
| Memory sample | Async sampler; cgroupfs via `spawn_blocking` + stack `[u8; 32]`; precomputed paths |

Full principles: [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)

## BPF verifier

- No `?` after `EVENTS.reserve`
- No `bpf_trace_printk`
- Always `submit(0)` or `discard(0)`
- `cgroup_id` from `bpf_get_current_cgroup_id()` on identity events

## User-space (Phase 2)

- Batched JSON `schema_version: 2`
- `FINOPS_RAW_EVENTS=1` debug only
- K8s: `tokio::spawn` + 30s interval — never `await` API in main `select!`
- Memory: precomputed `{CGROUP_ROOT}/…/memory.current`
- Env: `FINOPS_WINDOW_SECS`, `FINOPS_SAMPLE_INTERVAL_SECS`, `FINOPS_NODE_NAME`, `FINOPS_CGROUP_ROOT`

### Hot-path heap discipline

| Avoid | Use |
|-------|-----|
| `read_to_string` on `memory.current` or `/proc/{pid}/cgroup` | `File::read` into stack buffer (`[u8; 32]` / `[u8; 1024]`) |
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
| Agent | `init_retry_worker` — bounded queue 60, exponential backoff; shared client 3s timeout ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)) |
| API | `GET /health`, `GET /metrics`; denormalize → `try_send((node, Bytes))`; keyed Kafka; shutdown + 10s drain ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md), [ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md)) |
| Stack | `make compose-up` / `compose-down` — Kafka, ClickHouse, `finops-api` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)) |
| Storage | ClickHouse Kafka engine — no Rust consumer ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)) |
| CH Kafka | `kafka_skip_broken_messages`, `kafka_num_consumers` = partition count in prod ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)) |
| CH storage | `ReplacingMergeTree`; LC on `node`/`namespace`; `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries `FINAL`; 30d TTL ([ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)) |

Spec: [docs/phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)  
Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md)

## Build (always via Makefile)

```bash
make deps          # first time
make build         # ebpf + finops-user + finops-api
make check
make verify-btf    # when BPF / kernel portability touched
make compose-up    # Phase 3 stack (API in Docker on :3000)
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent on host (root)
make compose-down  # tear down stack
# Host-only API dev (not with compose-up): make run-api
# After API code changes in Docker: docker compose build finops-api && docker compose up -d finops-api
curl -s http://127.0.0.1:3000/metrics | grep finops_api_
```

Phase 2 validation: [docs/phase2-validation.md](../../../docs/phase2-validation.md)  
ADRs: [docs/adr/](../../../docs/adr/)  
Deferred: [TODO.md](TODO.md)

## OOM-safe remediation (Phases 4–5)

```
requests = p99 × 1.20
limits   = requests × 1.25
```

See Pattern 8 in [PATTERNS.md](PATTERNS.md).

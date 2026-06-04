---
name: finops-ebpf-agent
description: >-
  Enterprise low-latency standards for the FinOps eBPF stack (finops-core):
  BPF ring buffer, batched agent, HTTP→Kafka→ClickHouse; Phase 5 security focus.
  Use when editing finops-common, finops-ebpf, finops-user, finops-api; adding probes;
  ingest, Docker infra, or ADRs. Always read this skill first, then build with make,
  and update docs/adr/skills in the same change.
---

# FinOps eBPF Agent

**Enterprise goal:** &lt;0.1% node CPU at idle, **zero blocking** on kernel event drain, **no telemetry loss** on capacity signals.

Phases: **1–4 done** · **6 done** (L8 + [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) · **T1–3 done** (deploy/CH/read API — [ADR 024](../../../docs/adr/024-agent-production-container.md)–[027](../../../docs/adr/027-api-read-path-clickhouse.md)) · **5 active** (TLS + prod ops — [phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)) · **8 partial** (base K8s shipped; informer/drain open — [TODO](TODO.md))

## Mandatory workflow (every change)

1. Read [SKILL.md](SKILL.md) → [REFERENCE.md](REFERENCE.md) → [PATTERNS.md](PATTERNS.md)
2. Implement using patterns below (do not invent parallel conventions)
3. `make build && make check` (add `make verify-btf` if BPF/deploy changed)
4. **ADR** — new file in `docs/adr/` for architectural decisions ([enterprise-latency.md](../../../docs/enterprise-latency.md))
5. **Docs** — update README, phase validation, `phase5-production-readiness.md` if deploy gates change; `phase3-ingest-interface.md` if wire contract changes
6. **Skills** — update this skill, REFERENCE, PATTERNS, TODO in the **same PR**
7. Deferred work → [TODO.md](TODO.md); mark shipped items `[x]` (keep the line)

## Quick start checklist

```
- [ ] finops-common: FinopsEvent / kinds only here
- [ ] BPF: EVENTS map name matches loader; reserve → fill → submit(0); on reserve fail increment `RING_DROPS` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [ ] Agent: no await on ring-buffer path; Prometheus on `:9091` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)); procfs before write lock in `on_identity_event`
- [ ] Aggregator: FxHashMap, double buffer, early flush (never enforce_cap); `clock_offset_ns` ([ADR 016](../../../docs/adr/016-clock-domain-offset.md))
- [ ] Output: `FINOPS_INGEST_URL` → `init_http_client` (+ optional `FINOPS_API_TOKEN`) + `init_retry_worker` ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md), [ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md))
- [ ] API: GET /health; GET /ready; GET /metrics; POST /ingest; GET `/api/v1/workloads/summary` (`CLICKHOUSE_*` — [ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md)); try_send on ingest
- [ ] make build && make check
- [ ] docs/adr + skills updated
```

## Workspace contract

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `finops-common` | host + bpf | `FinopsEvent`, kind constants, `Pod` via `user` feature |
| `finops-ebpf` | `bpfel-unknown-none` | tracepoint, `cgroup_id`, ring buffer (`FINOPS_RING_BUF_BYTES` / [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md)) |
| `finops-user` | host | loader (`RING_DROPS` poll), attribution, aggregator, output; **`:9091/metrics`** ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| `finops-api` | host | Ingest: `POST /ingest` → mpsc; read: `GET /api/v1/workloads/summary` → ClickHouse ([ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md)); probes + `/metrics` ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md), [ADR 021](../../../docs/adr/021-ingest-ready-probe.md)) |

**Infra:** `docker-compose.yml`, `deploy/docker/`, `deploy/k8s/`, `deploy/clickhouse/01_init.sql` (single CH init)

Modules: see [REFERENCE.md](REFERENCE.md).

## Shared memory contract

Ring record: **`FinopsEvent`** (64 bytes) with `kind`:

- `EVENT_KIND_WORKLOAD_IDENTITY` (1) — exec via `sched:sched_process_exec`
- `EVENT_KIND_MEMORY_SAMPLE` (2) — user-space `memory.current` sampler

## Latency contract (non-negotiable)

| Layer | Rule |
|-------|------|
| Ring buffer loop | No `.await` on HTTP ingest or blocking I/O |
| `emit_batch` | Serialize + `try_send` to retry worker; on full queue, sync `try_lock` drop-oldest (no spawn); backoff + jitter ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)) |
| `POST /ingest` | `schema_version` 2 or 3 or `400` ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md)); `try_send`; `200` or `503` on channel full |
| Kafka | Background task only |
| Aggregator | Early flush at `max_keys`; flip buffer before drain; BPF timestamp + `clock_offset_ns` for windows ([ADR 016](../../../docs/adr/016-clock-domain-offset.md)) |
| Memory sample | Async sampler; cgroupfs via `spawn_blocking` + stack `[u8; 32]`; precomputed paths |

Full principles: [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)

## BPF verifier

- No `?` after `EVENTS.reserve`
- No `bpf_trace_printk`
- Always `submit(0)` or `discard(0)`
- `cgroup_id` from `bpf_get_current_cgroup_id()` on identity events

## User-space (Phase 2)

- Batched JSON `schema_version: 2`; `batch_id` + `agent_version` per flush ([ADR 017](../../../docs/adr/017-batch-lineage-metadata.md))
- `FINOPS_RAW_EVENTS=1` debug only
- K8s: `tokio::spawn` + 30s interval — never `await` API in main `select!`
- Startup: `bootstrap_existing_cgroups` before event loop ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md))
- Memory: precomputed `{CGROUP_ROOT}/…/memory.current`
- Env: `FINOPS_WINDOW_SECS`, `FINOPS_SAMPLE_INTERVAL_SECS`, `FINOPS_NODE_NAME`, `FINOPS_CGROUP_ROOT`

### Hot-path heap discipline

| Avoid | Use |
|-------|-----|
| `read_to_string` on `memory.current` or `/proc/{pid}/cgroup` | `File::read` into stack buffer (`[u8; 32]` / `[u8; 1024]`) |
| `PathBuf::join` / `to_path_buf` per sample tick | Precompute `Arc<PathBuf>` on identity; sampler clones `Arc` only |
| `Vec` of all cgroup IDs per tick | `for_each_memory_current_path` |
| `HashMap` for `cgroup_id` | `FxHashMap` ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md)) |

### Aggregator

| Rule | Detail |
|------|--------|
| Map | `rustc_hash::FxHashMap` |
| Buffers | Two maps; flip before drain ([ADR 004](../../../docs/adr/004-swap-buffer-before-drain.md)) |
| Cap | Early flush — never random eviction ([ADR 003](../../../docs/adr/003-early-flush-instead-of-cap-eviction.md)) |
| Clock | `clock_offset_ns` at `new`; `on_finops_event` converts BPF mono → wall ([ADR 016](../../../docs/adr/016-clock-domain-offset.md)) |

### Attribution

| Rule | Detail |
|------|--------|
| Locks | `parking_lot::RwLock`; **procfs before `write()`** on identity ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| Labels | `DEFAULT_LABELS` `LazyLock`; cache K8s/path merges in `cgroup_labels` |
| cgroup v2 | `split_once("::")` not `split_once(':')` |
| Paths | `Path::components()` — no full-path `to_string_lossy()` |

## Ingest pipeline (Phases 3–4 shipped; Phase 5 secures `/ingest`)

| Component | Rule |
|-----------|------|
| Agent | `init_http_client` (`FINOPS_API_TOKEN` → `default_headers`); `init_retry_worker` queue 60, backoff + jitter; HTTP timeouts via env ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md), [ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md)) |
| API | `GET /health`, `GET /ready` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md)); `POST /ingest` `expected_bearer` + `try_send` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md), [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md)); read `GET /api/v1/workloads/summary?hours=` ([ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md)) |
| Agent metrics | `http://0.0.0.0:9091/metrics` — ring drops + future counters ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| Stack | `make compose-up` / `compose-down` — Kafka, ClickHouse, `finops-api` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)) |
| Storage | ClickHouse Kafka engine — no Rust consumer ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)) |
| CH Kafka | `kafka_skip_broken_messages`, `kafka_num_consumers` = partition count in prod ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)) |
| CH storage | `finops.workload_metrics`; `ReplacingMergeTree`; billing `FINAL`; init [deploy/clickhouse/01_init.sql](../../../deploy/clickhouse/01_init.sql) ([ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md)) |
| Prod deploy | `deploy/docker/Dockerfile.{gateway,agent}`; `deploy/k8s/*.yaml` ([ADR 024](../../../docs/adr/024-agent-production-container.md), [ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md)) |

Spec: [docs/phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)  
Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md)

## Build (always via Makefile)

```bash
make deps          # first time
make build         # ebpf + finops-user + finops-api
make check
make verify-btf    # when BPF / kernel portability touched
make compose-up    # Dev stack (API in Docker on :3000); Phase 5: add FINOPS_API_TOKEN in prod
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent on host (root)
make compose-down  # tear down stack
# Host-only API dev (not with compose-up): make run-api
# After API code changes in Docker: docker compose build finops-api && docker compose up -d finops-api
# After CH schema change: docker compose down -v && make compose-up
# Billing check: SELECT count() FROM finops.workload_metrics FINAL
curl -s http://127.0.0.1:3000/metrics | grep finops_api_
curl -s http://127.0.0.1:9091/metrics | grep finops_agent_ring_drops   # agent (root)
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

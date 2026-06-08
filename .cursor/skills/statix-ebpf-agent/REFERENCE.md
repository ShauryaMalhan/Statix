# Statix eBPF Agent — Reference

Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → Kafka → ClickHouse.

**Principles:** [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `StatixEvent` → `EVENTS` |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` → retry worker → `POST /ingest` |
| Ingest API | `POST /ingest`; `try_send((Vec<u8>, Vec<u8>))` — [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md) |
| Read API | `GET /api/v1/workloads/summary?hours=` → `AppState.ch_client` — [ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md) |
| Agent metrics | `http://<host>:9091/metrics` — `statix_ring_drops_total` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| Storage | Kafka → CH Kafka engine → `ReplacingMergeTree` (billing: `FINAL`) |

## File map

```
Statix/
├── docker-compose.yml
├── Dockerfile.gateway
├── deploy/docker/Dockerfile.gateway
├── deploy/docker/Dockerfile.statix
├── deploy/k8s/gateway.yaml
├── deploy/k8s/statix-daemonset.yaml
├── deploy/clickhouse/01_init.sql
├── infra/clickhouse/README.md
├── statix-ebpf/, statix-common/, statix-wire/, statix-infra/, statix/, statix-gateway/ (`src/config.rs`)
├── .github/workflows/ebpf-ci.yml   # userspace + kernel verifier matrix ([ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md))
├── scripts/verify-ebpf-kernel.sh   # virtme-ng + statix-ebpf-verify per kernel
├── docs/ (enterprise-latency, phase2/3 validation, adr/)
└── .cursor/skills/statix-ebpf-agent/
```

## Data flow (ingest pipeline)

```
ring buffer → aggregator → emit_batch
  ├─ stdout (no STATIX_INGEST_URL)
  └─ POST /ingest → Kafka → statix.workload_metrics (query with FINAL)
```

## Roadmap

| Phase / target | Status |
|----------------|--------|
| 1–3 | Done (E2E ingest) |
| 4 | Done (scale, lineage, bootstrap, metrics) |
| 5 | **Partial** — TLS + P0 shipped; prod CH/Kafka ops ([phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)) |
| 5.5 V1/V2 | Done — L8 GA hardening ([ADR 032](../../../docs/adr/032-phase55-l8-p0-hot-path-fixes.md)–[043](../../../docs/adr/043-kubernetes-alb-tls-termination.md)) |
| 5.5 V3 | **Active** — post-GA audit ([L8_POST_GA_FIXES.md](L8_POST_GA_FIXES.md), [TODO.md](TODO.md)) |
| 6 | Done — mechanical sympathy / hot path ([ADR 018](../../../docs/adr/018-phase-roadmap-status.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| 7 | **Done** — wire, agent, gateway, infra, `Config`, typed errors, read-only labels ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md)–[036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md)) |
| T1–3 | Done — prod images, K8s YAML, CH init, read API ([ADR 024](../../../docs/adr/024-agent-production-container.md)–[027](../../../docs/adr/027-api-read-path-clickhouse.md)) |
| 8 | Partial — V2 K8s hardening shipped (informer, drain, digest pins); stronger cgroup→pod mapping open |
| 9 | Partial — eBPF verifier CI shipped ([ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md)); arm64 / cgroup v1 detection open |
| 10 | Extended observability (Grafana shipped; agent metrics + CH tuning open) |

## Operational notes

- Phase 3 stack: `make compose-up` / `make compose-down` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)); CH schema change → `docker compose down -v` then `make compose-up` ([ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- Prod: `deploy/docker/README.md`, `deploy/k8s/README.md`, `deploy/clickhouse/README.md`
- Kafka UI `:8080`; ClickHouse `:8123` (`default` + `CLICKHOUSE_PASSWORD` from `.env`); API `:3000` (`/health`, `/ready`, `/metrics`); Grafana `:3001` (anonymous admin, ClickHouse plugin — [ADR 031](../../../docs/adr/031-grafana-clickhouse-compose.md)); agent `:9091/metrics`
- **Gateway env:** `config::Config::from_env()` in `statix-gateway/src/config.rs` — `KAFKA_BROKERS`, `STATIX_API_PORT` (invalid → exit 1), `STATIX_API_TOKEN`, `CLICKHOUSE_*` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md)); Kafka tuning in `kafka.rs` via `statix_infra::env` ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md), [048](../../../docs/adr/048-generic-env-positive-parsing.md))
- Kafka: host `localhost:9092`, in-compose `kafka:29092` (API + ClickHouse consumer)
- Agent ingest URL: `http://127.0.0.1:3000/ingest` (not `localhost` — IPv6)
- eBPF bundle: `target/bpf/statix-ebpf-{small,large,xlarge}`; auto by `num_cpus` — [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md); override `STATIX_EBF_PATH`
- Agent event loop: `watch_k8s_pods` stream (node-scoped informer — [ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md)); `labels_for_cgroup` read-only; ring drain `DRAIN_BUDGET=256`; memory samples = one `spawn_blocking`/tick; ingest retry = `bytes::Bytes` ([ADR 032](../../../docs/adr/032-phase55-l8-p0-hot-path-fixes.md), [ADR 033](../../../docs/adr/033-phase55-l8-p1-week-gateway-fixes.md), [ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md), [enterprise-latency.md](../../../docs/enterprise-latency.md))
- Gateway ingest: `FlatRowRef` + `Arc<[u8]>` node key — no envelope string clones on HTTP thread ([ADR 034](../../../docs/adr/034-phase55-l8-p2-ingest-zero-copy.md))
- Startup cgroup bootstrap: `bootstrap_existing_cgroups` (walkdir + dir `ino()` = `cgroup_id`; `STATIX_CGROUP_ROOT`) — [ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md)
- Aggregator clock: global `AtomicU64` offset; `STATIX_CLOCK_RECALIBRATE_SECS` (default 3600) — [ADR 016](../../../docs/adr/016-clock-domain-offset.md), [047](../../../docs/adr/047-atomic-clock-offset-recalibration.md)
- Batch lineage: `batch_id` (UUID v4) + `agent_version` on every flush — [ADR 017](../../../docs/adr/017-batch-lineage-metadata.md)
- ClickHouse `ReplacingMergeTree` + `FINAL` billing reads: [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)
- ClickHouse Kafka engine: `kafka_skip_broken_messages`, `kafka_num_consumers` — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)
- Agent HTTP: `init_http_client()` + `init_retry_worker()` — env timeouts (5s / 55s defaults), backoff; queue full → sync `try_lock` drop-oldest (no spawn) — [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)
- Merge conflicts: resolve all `<<<<<<<` markers before `make run`

## Deferred work

[TODO.md](TODO.md)

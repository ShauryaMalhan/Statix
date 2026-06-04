# FinOps eBPF Agent — Reference

Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → Kafka → ClickHouse.

**Principles:** [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` → `EVENTS` |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` → retry worker → `POST /ingest` |
| Ingest API | `POST /ingest`; `try_send((Vec<u8>, Vec<u8>))` — [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md) |
| Read API | `GET /api/v1/workloads/summary?hours=` → `AppState.ch_client` — [ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md) |
| Agent metrics | `http://<host>:9091/metrics` — `finops_agent_ring_drops_total` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| Storage | Kafka → CH Kafka engine → `ReplacingMergeTree` (billing: `FINAL`) |

## File map

```
finops-core/
├── docker-compose.yml
├── Dockerfile.api
├── deploy/docker/Dockerfile.gateway
├── deploy/docker/Dockerfile.agent
├── deploy/k8s/gateway.yaml
├── deploy/k8s/agent-daemonset.yaml
├── deploy/clickhouse/01_init.sql
├── infra/clickhouse/README.md
├── finops-ebpf/, finops-common/, finops-wire/, finops-agent/, finops-api/ (`src/config.rs`)
├── docs/ (enterprise-latency, phase2/3 validation, adr/)
└── .cursor/skills/finops-ebpf-agent/
```

## Data flow (ingest pipeline)

```
ring buffer → aggregator → emit_batch
  ├─ stdout (no FINOPS_INGEST_URL)
  └─ POST /ingest → Kafka → finops.workload_metrics (query with FINAL)
```

## Roadmap

| Phase | Status |
|-------|--------|
| 1–3 | Done (E2E ingest) |
| 4 | Done (scale, lineage, bootstrap, metrics) |
| 5 | **Active** — TLS + prod CH/Kafka ([phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)) |
| 6 | Done — L8 + P0 fixes ([ADR 018](../../../docs/adr/018-phase-roadmap-status.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| T1–3 | Done — prod images, K8s YAML, CH init, read API ([ADR 024](../../../docs/adr/024-agent-production-container.md)–[027](../../../docs/adr/027-api-read-path-clickhouse.md)) |
| 8 | Partial — base manifests shipped; informer/drain/registry pins open |
| 7 | Partial — `finops-wire`, `finops-agent`, `Config` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md), [030](../../../docs/adr/030-finops-api-config-struct.md)) |
| 9–10 | Portability, extended observability |

## Operational notes

- Phase 3 stack: `make compose-up` / `make compose-down` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)); CH schema change → `docker compose down -v` then `make compose-up` ([ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- Prod: `deploy/docker/README.md`, `deploy/k8s/README.md`, `deploy/clickhouse/README.md`
- Kafka UI `:8080`; ClickHouse `:8123` (`default` / `finops_dev`); API `:3000` (`/health`, `/ready`, `/metrics`); Grafana `:3001` (anonymous admin, ClickHouse plugin — [ADR 031](../../../docs/adr/031-grafana-clickhouse-compose.md)); agent `:9091/metrics`
- **Gateway env:** `config::Config::from_env()` in `finops-api/src/config.rs` — `KAFKA_BROKERS`, `FINOPS_API_PORT` (invalid → exit 1), `FINOPS_API_TOKEN`, `CLICKHOUSE_*` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md)); Kafka tuning still in `kafka.rs` (`FINOPS_KAFKA_CHANNEL_SIZE`, …)
- Kafka: host `localhost:9092`, in-compose `kafka:29092` (API + ClickHouse consumer)
- Agent ingest URL: `http://127.0.0.1:3000/ingest` (not `localhost` — IPv6)
- eBPF bundle: `target/bpf/finops-ebpf-{small,large,xlarge}`; auto by `num_cpus` — [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md); override `FINOPS_EBF_PATH`
- Agent event loop: K8s API in `tokio::spawn`; memory samples via `spawn_blocking` ([enterprise-latency.md](../../../docs/enterprise-latency.md))
- Startup cgroup bootstrap: `bootstrap_existing_cgroups` (walkdir + dir `ino()` = `cgroup_id`; `FINOPS_CGROUP_ROOT`) — [ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md)
- Aggregator clock: `clock_offset_ns` at `new`; window bounds in wall domain aligned with BPF timestamps — [ADR 016](../../../docs/adr/016-clock-domain-offset.md)
- Batch lineage: `batch_id` (UUID v4) + `agent_version` on every flush — [ADR 017](../../../docs/adr/017-batch-lineage-metadata.md)
- ClickHouse `ReplacingMergeTree` + `FINAL` billing reads: [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)
- ClickHouse Kafka engine: `kafka_skip_broken_messages`, `kafka_num_consumers` — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)
- Agent HTTP: `init_http_client()` + `init_retry_worker()` — env timeouts (5s / 55s defaults), backoff; queue full → sync `try_lock` drop-oldest (no spawn) — [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)
- Merge conflicts: resolve all `<<<<<<<` markers before `make run`

## Deferred work

[TODO.md](TODO.md)

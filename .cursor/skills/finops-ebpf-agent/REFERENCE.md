# FinOps eBPF Agent — Reference

Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → Kafka → ClickHouse.

**Principles:** [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` → `EVENTS` |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` |
| Ingest API | `GET /health`, `POST /ingest` (`400`/`503`/`200`) → `try_send` → background Kafka |
| Storage | Kafka → CH Kafka engine → MergeTree |

## File map

```
finops-core/
├── docker-compose.yml
├── Dockerfile.api
├── infra/clickhouse/init.sql
├── finops-ebpf/, finops-common/, finops-user/, finops-api/
├── docs/ (enterprise-latency, phase2/3 validation, adr/)
└── .cursor/skills/finops-ebpf-agent/
```

## Data flow (Phase 3)

```
ring buffer → aggregator → emit_batch
  ├─ stdout (no FINOPS_INGEST_URL)
  └─ POST /ingest → Kafka → finops_telemetry
```

## Roadmap

| Phase | Status |
|-------|--------|
| 1–2 | Done |
| 3 | Done (HTTP ingest) |
| 4–8 | Analyzer, GitOps, dashboard |

## Operational notes

- Phase 3 stack: `make compose-up` / `make compose-down` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- Kafka UI `:8080`; ClickHouse `:8123` (`default` / `finops_dev`); API `http://127.0.0.1:3000/health`
- Kafka: host `localhost:9092`, in-compose `kafka:29092` (API + ClickHouse consumer)
- Agent ingest URL: `http://127.0.0.1:3000/ingest` (not `localhost` — IPv6)
- Agent event loop: K8s API in `tokio::spawn`; memory samples via `spawn_blocking` ([enterprise-latency.md](../../../docs/enterprise-latency.md))
- ClickHouse MergeTree: [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md)
- ClickHouse Kafka engine: `kafka_skip_broken_messages`, `kafka_num_consumers` — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)
- Agent HTTP: `init_http_client()` — 3s timeout, 90s pool idle — [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)
- Merge conflicts: resolve all `<<<<<<<` markers before `make run`

## Deferred work

[TODO.md](TODO.md)

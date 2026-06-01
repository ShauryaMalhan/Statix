# FinOps eBPF Agent — Reference

Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → Kafka → ClickHouse.

**Principles:** [docs/enterprise-latency.md](../../../docs/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `FinopsEvent` → `EVENTS` |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` → retry worker → `POST /ingest` |
| Ingest API | `GET /health`, `GET /metrics`, `POST /ingest` → `try_send((Bytes, Bytes))` — one `node` key alloc per batch ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md), [ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md)) |
| Storage | Kafka → CH Kafka engine → `ReplacingMergeTree` (billing: `FINAL`) |

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
  └─ POST /ingest → Kafka → finops_telemetry (query with FINAL)
```

## Roadmap

| Phase | Status |
|-------|--------|
| 1–2 | Done |
| 3 | Done (HTTP ingest) |
| 4–8 | Analyzer, GitOps, dashboard |

## Operational notes

- Phase 3 stack: `make compose-up` / `make compose-down` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- Kafka UI `:8080`; ClickHouse `:8123` (`default` / `finops_dev`); API `http://127.0.0.1:3000/health` and `/metrics`
- Kafka: host `localhost:9092`, in-compose `kafka:29092` (API + ClickHouse consumer)
- Agent ingest URL: `http://127.0.0.1:3000/ingest` (not `localhost` — IPv6)
- eBPF bundle: `target/bpf/finops-ebpf-{small,large,xlarge}`; auto by `num_cpus` — [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md); override `FINOPS_EBF_PATH`
- Agent event loop: K8s API in `tokio::spawn`; memory samples via `spawn_blocking` ([enterprise-latency.md](../../../docs/enterprise-latency.md))
- ClickHouse `ReplacingMergeTree` + `FINAL` billing reads: [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)
- ClickHouse Kafka engine: `kafka_skip_broken_messages`, `kafka_num_consumers` — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md)
- Agent HTTP: `init_http_client()` + `init_retry_worker()` — env timeouts (5s / 55s defaults), backoff — [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)
- Merge conflicts: resolve all `<<<<<<<` markers before `make run`

## Deferred work

[TODO.md](TODO.md)

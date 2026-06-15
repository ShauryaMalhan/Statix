# Statix eBPF Agent — Reference

Enterprise low-latency telemetry: kernel → agent → (stdout | HTTP) → gateway → ClickHouse RowBinary.

**Principles:** [docs/guides/enterprise-latency.md](../../../docs/guides/enterprise-latency.md)  
**Workflow:** Update ADR + docs + skills with every architectural change.

## Overview

| Layer | Role |
|-------|------|
| Kernel | `sched:sched_process_exec` → `StatixEvent` → `EVENTS` |
| Agent | AsyncFd → attribution → aggregator → `emit_batch` → retry worker → `POST /ingest` (overflow → disk WAL `statix/src/wal/`, [ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)) |
| Ingest API | `POST /ingest`; `try_reserve_many(FlatRow)` — [ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md) |
| Read API | `GET /api/v1/workloads/summary?hours=` → `AppState.ch_client` — [ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md) |
| Agent metrics | `http://<host>:9091/metrics` — ring drops, WAL, circuit ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)) |
| Storage | Gateway RowBinary → `statix.workload_metrics` (`ReplacingMergeTree`; billing: `FINAL`) — [ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md) |

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
  └─ POST /ingest → clickhouse_writer (RowBinary) → statix.workload_metrics (query with FINAL)
```

## Roadmap

| Phase / target | Status |
|----------------|--------|
| 1–3 | Done (E2E ingest) |
| 4 | Done (scale, lineage, bootstrap, metrics) |
| 5 | **Partial** — TLS + P0 shipped; prod ops ([phase5-production-readiness.md](../../../docs/guides/phase5-production-readiness.md)) |
| 5.5 V1/V2 | Done — L8 GA hardening ([ADR 032](../../../docs/adr/phase55/l8/032-phase55-l8-p0-hot-path-fixes.md)–[043](../../../docs/adr/phase55/v2/043-kubernetes-alb-tls-termination.md)) |
| 5.5 V3 | Done — post-GA audit ([ADR 049](../../../docs/adr/phase55/v3/049-phase55-v3-wave1-silent-deaths.md)–[053](../../../docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md)) |
| 11 | Done — WAL + circuit breaker ([ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)) |
| 13 | **Part 1 done** — queue-less ingest ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)); Part 2 compose strip open |
| 6 | Done — mechanical sympathy / hot path ([ADR 018](../../../docs/adr/018-phase-roadmap-status.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| 7 | **Done** — wire, agent, gateway, infra, `Config`, typed errors, read-only labels ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md)–[036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md)) |
| T1–3 | Done — prod images, K8s YAML, CH init, read API ([ADR 024](../../../docs/adr/024-agent-production-container.md)–[027](../../../docs/adr/027-api-read-path-clickhouse.md)) |
| 8 | Partial — V2 K8s hardening shipped (informer, drain, digest pins); stronger cgroup→pod mapping open |
| 9 | Partial — eBPF verifier CI shipped ([ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md)); arm64 / cgroup v1 detection open |
| 10 | Extended observability (Grafana shipped; agent metrics + CH tuning open) |

## Operational notes

- Phase 3 stack: `make compose-up` / `make compose-down` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)); CH schema change → `docker compose down -v` then `make compose-up` ([ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- Prod: `deploy/docker/README.md`, `deploy/k8s/README.md`, `deploy/clickhouse/README.md`
- Kafka UI `:8080` *(legacy compose — removed Part 2)*; ClickHouse `:8123`; API `:3000`; Grafana `:3001`; agent `:9091/metrics`
- **Gateway env:** `config::Config::from_env()` — `STATIX_API_PORT`, `STATIX_API_TOKEN`, `CLICKHOUSE_*` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md)); writer tuning in `clickhouse_writer.rs`: `STATIX_INGEST_CHANNEL_SIZE`, `STATIX_CH_BATCH_MAX`, `STATIX_CH_LINGER_MS`, `STATIX_CH_INSERT_TIMEOUT_SECS` ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md))
- Agent ingest URL: `http://127.0.0.1:3000/ingest` (not `localhost` — IPv6)
- eBPF bundle: `target/bpf/statix-ebpf-{small,large,xlarge}`; auto by `num_cpus` — [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md); override `STATIX_EBF_PATH`
- Agent event loop: `watch_k8s_pods` stream (node-scoped informer — [ADR 041](../../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md)); `labels_for_cgroup` read-only; ring drain `DRAIN_BUDGET=256`; memory samples = one `spawn_blocking`/tick; ingest retry = `bytes::Bytes` ([ADR 032](../../../docs/adr/phase55/l8/032-phase55-l8-p0-hot-path-fixes.md), [ADR 033](../../../docs/adr/phase55/l8/033-phase55-l8-p1-week-gateway-fixes.md), [ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md), [enterprise-latency.md](../../../docs/guides/enterprise-latency.md))
- Gateway ingest: owned `FlatRow` → mpsc coalescer → RowBinary ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md))
- Startup cgroup bootstrap: `bootstrap_existing_cgroups` (walkdir + dir `ino()` = `cgroup_id`; `STATIX_CGROUP_ROOT`) — [ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md)
- Aggregator clock: global `AtomicU64` offset; `STATIX_CLOCK_RECALIBRATE_SECS` (default 3600) — [ADR 016](../../../docs/adr/016-clock-domain-offset.md), [047](../../../docs/adr/047-atomic-clock-offset-recalibration.md)
- Batch lineage: `batch_id` (UUID v4) + `agent_version` on every flush — [ADR 017](../../../docs/adr/017-batch-lineage-metadata.md)
- ClickHouse `ReplacingMergeTree` + `FINAL` billing reads: [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)
- Gateway RowBinary writer + `ch_healthy` backpressure: [ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)
- Agent HTTP: `init_http_client()` + `init_retry_worker()` — env timeouts (5s / 55s defaults), backoff; queue full → sync `try_lock` drop-oldest (no spawn) — [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)
- Merge conflicts: resolve all `<<<<<<<` markers before `make run`

## Deferred work

[TODO.md](TODO.md)

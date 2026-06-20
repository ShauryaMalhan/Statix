# Phase 3 ingest interface

Phase 3 ships **HTTP ingest** (not gRPC): the agent POSTs the Phase 2 batch JSON; the gateway denormalizes rows and writes to ClickHouse via a RowBinary coalescer ([ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).

**Enterprise constraints:** [enterprise-latency.md](enterprise-latency.md) · **Validation:** [phase3-validation.md](phase3-validation.md) · **ADR:** [005](../adr/005-non-blocking-ingest-pipeline.md) (historical Kafka path), [055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md) (current)

## Flow (Phase 13)

```
statix  --POST /ingest-->  statix-gateway  --try_reserve_many-->  mpsc  --RowBinary-->  ClickHouse
                                                                                              |
                                                                                    statix.workload_metrics
```

Backpressure: `ch_healthy` false or mpsc full → `503` → agent circuit breaker → WAL ([ADR 054](../adr/phase11/054-phase11-wal-spillway.md)).

## Batch envelope (agent → API)

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | u32 | `2` or `3` ([ADR 020](../adr/020-ingest-schema-version-window.md)) |
| `window_start_ns` | u64 | Window open (Unix ns) |
| `window_end_ns` | u64 | Window close (Unix ns) |
| `node` | string | Hostname / `STATIX_NODE_NAME` |
| `batch_id` | string | UUID v4 per flush ([ADR 017](../adr/017-batch-lineage-metadata.md)) |
| `agent_version` | string | `statix` crate version at flush time |
| `workloads` | array | Rolled-up rows (below) |

## Workload row

| Field | Type | Description |
|-------|------|-------------|
| `cgroup_id` | u64 | Join key from `bpf_get_current_cgroup_id()` |
| `namespace` | string? | Kubernetes namespace when resolved |
| `pod` | string? | Pod name when resolved |
| `container` | string? | Container name when resolved |
| `k8s_resolved` | bool | `true` if namespace+pod known |
| `memory_bytes_max` | u64 | Max `memory.current` in window |
| `memory_bytes_last` | u64 | Last sample in window |
| `exec_count` | u32 | `sched_process_exec` events in window |
| `sample_count` | u32 | Memory samples in window |
| `cpu_usage_usec` | u64 | CPU microseconds consumed in window (delta of cgroup `cpu.stat` `usage_usec`; schema v3; omitted in v2 → `0`) ([ADR 058](../adr/phase14/058-phase14-cpu-usage-tracking.md)) |

## Gateway flat row (one per workload)

The ingest handler builds gateway-local `MetricRow` via `MetricRow::from_ingest` and enqueues to the coalescer mpsc ([ADR 056](../adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).

## ClickHouse storage (`statix.workload_metrics`)

| Item | Value |
|------|--------|
| Engine | `ReplacingMergeTree()` — dedupes on background merge |
| Sort key | `(node, window_start_ns, cgroup_id)` ([ADR 011](../adr/011-replacingmergetree-dedupe-identity.md)) |
| Billing queries | Always `FROM statix.workload_metrics FINAL` |

Schema change on existing volume: `docker compose down -v && make compose-up`.

## HTTP endpoints

| Route | Method | Response |
|-------|--------|----------|
| `/health` | GET | **Liveness:** `200` if ingest mpsc sender open; `503` if writer task exited |
| `/ready` | GET | **Readiness:** `200` if `ch_healthy` + mpsc &lt;80% ([ADR 029](../adr/029-ready-channel-depth-gate.md), [055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)); else `503` |
| `/metrics` | GET | Prometheus text ([ADR 012](../adr/012-finops-api-prometheus-metrics.md)) |
| `/ingest` | POST | See table below |

## HTTP responses (`POST /ingest`)

| Status | When | Body |
|--------|------|------|
| `200 OK` | Every workload row enqueued to coalescer mpsc | empty |
| `401 Unauthorized` | `STATIX_API_TOKEN` set and `Authorization` missing/wrong ([ADR 019](../adr/019-ingest-bearer-token-auth.md)) | empty |
| `400 Bad Request` | `schema_version` not in `2..=3` | plain text |
| `503 Service Unavailable` | `!ch_healthy` or `try_reserve_many` full | plain text |

Handler never awaits ClickHouse insert. On `503`, agent circuit opens and WAL captures batches ([ADR 054](../adr/phase11/054-phase11-wal-spillway.md)).

## Environment variables

### Agent (`statix`)

Wire types: `statix_wire::IngestBatch` ([ADR 028](../adr/028-finops-wire-and-agent-rename.md)).

| Variable | Default | Purpose |
|----------|---------|---------|
| `STATIX_INGEST_URL` | (unset) | If set, `POST` batch JSON here; else stdout |
| `STATIX_API_TOKEN` | (unset) | Bearer token (must match API) |
| `STATIX_HTTP_TIMEOUT_SECS` | `5` | Agent `reqwest` timeout |
| `STATIX_HTTP_POOL_IDLE_SECS` | `55` | Pool idle timeout |
| `STATIX_BACKOFF_*` | 1s→30s | Retry worker ([ADR 006](../adr/006-shared-http-client-for-ingest.md)) |
| `STATIX_EBF_PATH` | (required) | Compiled BPF ELF |
| `STATIX_WINDOW_SECS` | `10` | Aggregation window |
| `STATIX_SAMPLE_INTERVAL_SECS` | `10` | cgroupfs poll interval (`memory.current` + `cpu.stat` on same tick) |
| `STATIX_NODE_NAME` | hostname | Node id in batches |

### API (`statix-gateway`)

Loaded by `config::Config::from_env()` ([ADR 030](../adr/030-finops-api-config-struct.md)). Writer tuning in `clickhouse_writer.rs` ([ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).

| Variable | Default | Purpose |
|----------|---------|---------|
| `STATIX_API_PORT` | `3000` | HTTP listen port |
| `STATIX_API_TOKEN` | (unset) | Bearer on `POST /ingest` |
| `STATIX_INGEST_CHANNEL_SIZE` | `8192` (min 1024) | Ingest mpsc depth |
| `STATIX_CH_BATCH_MAX` | `1024` (64–16384) | RowBinary micro-batch size |
| `STATIX_CH_LINGER_MS` | `50` (1–1000) | Coalesce linger |
| `STATIX_CH_INSERT_TIMEOUT_SECS` | `3` (1–30) | Insert ACK timeout; flips `ch_healthy` |
| `STATIX_MPSC_DEPTH_SAMPLE_MS` | `1000` | Background mpsc depth gauge sampler period ([ADR 060](../adr/phase10/060-phase10-golden-signal-saturation-metrics.md)) |
| `CLICKHOUSE_URL` | `http://localhost:8123` | Read + write client |
| `CLICKHOUSE_USER` / `CLICKHOUSE_PASSWORD` | `default` / empty | Auth |

## Read API

`GET /api/v1/workloads/summary?hours=<u64>` — default **24** hours. Aggregates over `statix.workload_metrics` (no `FINAL` on summary — same WAL double-count caveat as `total_execs`; billing uses `FINAL` on the table) ([ADR 027](../adr/027-api-read-path-clickhouse.md), [058](../adr/phase14/058-phase14-cpu-usage-tracking.md)).

Response fields include `peak_memory`, `total_execs`, and `total_cpu_usec` (`sum(cpu_usage_usec)`).

```bash
curl -s 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=24' | jq .
```

## Local stack

```bash
make compose-up   # ClickHouse + Grafana + statix-gateway
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:3000/health   # 200
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

Optional host API: `make run-api` — never both on `:3000` ([ADR 009](../adr/009-finops-api-docker-compose.md)).

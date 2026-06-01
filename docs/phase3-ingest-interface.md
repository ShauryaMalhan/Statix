# Phase 3 ingest interface

Phase 3 ships **HTTP ingest** (not gRPC): the agent POSTs the same Phase 2 batch JSON; the API denormalizes rows to Kafka; ClickHouse consumes via the Kafka engine table.

**Enterprise constraints:** [enterprise-latency.md](enterprise-latency.md) · **Validation:** [phase3-validation.md](phase3-validation.md) · **ADR:** [adr/005-non-blocking-ingest-pipeline.md](adr/005-non-blocking-ingest-pipeline.md)

## Flow

```
finops-user  --POST /ingest-->  finops-api  --try_send-->  mpsc  --produce-->  Kafka
                                                                                    |
ClickHouse  finops_telemetry_kafka  <--MATERIALIZED VIEW-->  finops_telemetry
```

## Batch envelope (agent → API)

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | u32 | Always `2` |
| `window_start_ns` | u64 | Window open (Unix ns) |
| `window_end_ns` | u64 | Window close (Unix ns) |
| `node` | string | Hostname / `FINOPS_NODE_NAME` |
| `batch_id` | string | UUID v4 per flush — audit lineage ([ADR 017](adr/017-batch-lineage-metadata.md)) |
| `agent_version` | string | `finops-user` crate version at flush time |
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

## Kafka message (one per workload row)

API stamps envelope fields on each row before `produce`. Matches ClickHouse `JSONEachRow`:

```json
{
  "window_start_ns": 0,
  "window_end_ns": 0,
  "node": "host",
  "batch_id": "550e8400-e29b-41d4-a716-446655440000",
  "agent_version": "0.1.0",
  "cgroup_id": 1,
  "namespace": null,
  "pod": null,
  "container": null,
  "k8s_resolved": false,
  "memory_bytes_max": 0,
  "memory_bytes_last": 0,
  "exec_count": 1,
  "sample_count": 0
}
```

Topic: `finops-telemetry` — each row is produced with Kafka **key** = batch `node` (partition `hash(node) % topic_partitions`; see [ADR 010](adr/010-kafka-partition-key-by-node.md)).

## ClickHouse Kafka consumer (`infra/clickhouse/init.sql`)

| Setting | Local dev | Production |
|---------|-----------|------------|
| `kafka_skip_broken_messages` | `1000` | same — avoid poison-pill halt |
| `kafka_num_consumers` | `1` | **match topic partition count** (e.g. `8`) |

See [ADR 008](adr/008-clickhouse-kafka-engine-resilience.md).

## ClickHouse storage (`finops_telemetry`)

| Item | Value |
|------|--------|
| Engine | `ReplacingMergeTree()` — dedupes on background merge |
| Sort key | `(node, window_start_ns, cgroup_id)` — **not** `namespace` (mutable; retries must not change identity) |
| Billing queries | Always `FROM finops_telemetry FINAL` ([ADR 011](adr/011-replacingmergetree-dedupe-identity.md)) |

Schema change on existing volume: `docker compose down -v && make compose-up`.

## HTTP endpoints

| Route | Method | Response |
|-------|--------|----------|
| `/health` | GET | `200` if Kafka producer task is alive; `503` if `mpsc` sender is closed (producer crashed or never started) |
| `/metrics` | GET | Prometheus text exposition ([ADR 012](adr/012-finops-api-prometheus-metrics.md)) |
| `/ingest` | POST | See table below |

## HTTP responses (`POST /ingest`)

| Status | When | Body |
|--------|------|------|
| `200 OK` | Every workload row enqueued to the Kafka `mpsc` channel | empty |
| `400 Bad Request` | `schema_version != 2` (poison-pill defense — reject before Kafka/ClickHouse) | `Unsupported schema_version=N. Expected 2.` |
| `503 Service Unavailable` | First `try_send` fails (channel full / broker backpressure) | `Ingest channel full. Broker backpressure active.` |

Handler uses `impl IntoResponse`; it never awaits Kafka produce. On `503`, the agent retry worker backs off (1s→30s — [ADR 006](adr/006-shared-http-client-for-ingest.md)). Storage dedupe: [ADR 011](adr/011-replacingmergetree-dedupe-identity.md). Partial rows may already be enqueued before `503` until `batch_id` ships ([TODO](../../.cursor/skills/finops-ebpf-agent/TODO.md) 4.6).

## Environment variables

### Agent (`finops-user`)

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_INGEST_URL` | (unset) | If set, `POST` batch JSON here; else stdout |
| `FINOPS_HTTP_TIMEOUT_SECS` | `5` | Agent `reqwest` request timeout (seconds) |
| `FINOPS_HTTP_POOL_IDLE_SECS` | `55` | Agent pool idle timeout (seconds; &lt; ALB 60s typical) |
| `FINOPS_BACKOFF_INITIAL_SECS` | `1` | Retry worker base backoff (seconds) |
| `FINOPS_BACKOFF_MAX_SECS` | `30` | Retry worker max backoff cap (seconds) |
| (client) | — | `init_http_client` + `init_retry_worker`; queue 60; **30% jitter** on retry sleep ([ADR 006](adr/006-shared-http-client-for-ingest.md)) |
| `FINOPS_EBF_PATH` | (required) | Path to compiled BPF ELF |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation window |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | cgroup `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `FINOPS_CGROUP_ROOT` | `/sys/fs/cgroup` | cgroup v2 mount |
| `FINOPS_RAW_EVENTS` | off | Per-event debug JSON |

### API (`finops-api`)

| Variable | Default | Purpose |
|----------|---------|---------|
| `KAFKA_BROKERS` | `localhost:9092` | Kafka bootstrap (host: `localhost:9092`, in-compose: `kafka:29092`) |
| `FINOPS_API_PORT` | `3000` | HTTP listen port |
| `FINOPS_KAFKA_CHANNEL_SIZE` | `8192` (min 1024) | Ingest → producer `mpsc` depth |
| `FINOPS_KAFKA_BATCH_MAX` | `1024` (64–16384) | Micro-batch / produce chunk size |
| `FINOPS_KAFKA_LINGER_MS` | `50` (1–1000) | Partial batch linger before flush |

See [ADR 014](adr/014-kafka-producer-env-tuning.md).

## Local stack

```bash
make compose-up   # starts finops-api container (KAFKA_BROKERS=kafka:29092) + Kafka + ClickHouse
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:3000/health   # 200
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent on host → API in Docker on :3000
```

Optional host API instead of container: `make run-api` ([ADR 009](adr/009-finops-api-docker-compose.md) — never both on `:3000`).

**ClickHouse HTTP (docker-compose):** user `default`, password `finops_dev` (see `docker-compose.yml`). Example:

```bash
curl -s -u default:finops_dev 'http://localhost:8123/?query=SELECT%20count()%20FROM%20finops_telemetry%20FINAL'
```

## Deferred

- TLS between agent and API
- Kafka topic replication / multi-broker production config (set `kafka_num_consumers` when partition count changes)
- `batch_id` on wire + p99 analyzer (Phase 4 — see [TODO](../../.cursor/skills/finops-ebpf-agent/TODO.md))

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

Topic: `finops-telemetry`

## Environment variables

### Agent (`finops-user`)

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_INGEST_URL` | (unset) | If set, `POST` batch JSON here; else stdout |
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

## Local stack

```bash
make compose-up
make build-api && make run-api
sudo -E FINOPS_INGEST_URL=http://localhost:3000/ingest make run
```

## Deferred

- TLS between agent and API
- Multi-partition / replication tuning
- Dedupe and p99 analyzer (Phase 4)

# Phase 3 ingest interface (specification)

Phase 2 emits **batched workload rows** on stdout. Phase 3 agents will stream the same logical record over gRPC without changing field semantics.

## Batch envelope

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | u32 | Always `2` for Phase 2+ batches |
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

## Environment variables (Phase 2 agent)

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_EBF_PATH` | (required) | Path to compiled BPF ELF |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation window |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | cgroup `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `FINOPS_CGROUP_ROOT` | `/sys/fs/cgroup` | cgroup v2 mount |
| `FINOPS_RAW_EVENTS` | off | Per-event debug JSON |

## gRPC sketch (Phase 3 — not implemented)

```protobuf
message WorkloadBatch {
  uint32 schema_version = 1;
  uint64 window_start_ns = 2;
  uint64 window_end_ns = 3;
  string node = 4;
  repeated WorkloadRow workloads = 5;
}
```

Ingestion service responsibilities: dedupe by `(node, namespace, pod, container, window)`, write to ClickHouse, compute p99 for Phase 4 analyzer.

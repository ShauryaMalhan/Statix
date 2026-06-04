# FinOps eBPF Agent — Patterns

Enterprise templates. **Before coding:** [SKILL.md](SKILL.md) workflow → implement → `make build` → update ADR/docs/skills.

Rules: [enterprise-latency.md](../../../docs/enterprise-latency.md). Architecture: [REFERENCE.md](REFERENCE.md).

---

## Pattern 1 — `FinopsEvent` in finops-common

```rust
pub const EVENT_KIND_WORKLOAD_IDENTITY: u8 = 1;
pub const EVENT_KIND_MEMORY_SAMPLE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FinopsEvent { /* kind, cgroup_id, timestamp, memory_bytes, comm, ... */ }

#[cfg(feature = "user")]
unsafe impl aya::Pod for FinopsEvent {}
```

---

## Pattern 2 — Ring buffer map (finops-ebpf)

```rust
include!(concat!(env!("OUT_DIR"), "/ring_config.rs"));
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(RING_BUF_BYTES, 0);
// build.rs + make build-ebpf → target/bpf/finops-ebpf-{small,large,xlarge}; ebpf_select.rs picks by core count
```

---

## Pattern 3 — Tracepoint identity capture (kernel)

`reserve` → fill → `submit(0)`. Never `?` after `reserve`. On `None`, increment `RING_DROPS` key `0` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md)); agent polls every 10s.

---

## Pattern 4 — Attach tracepoint (loader.rs)

`program.attach("sched", "sched_process_exec")`

---

## Pattern 5 — User event loop + batch flush (main.rs)

K8s pod list refresh runs in a **detached** `tokio::spawn` (`AttributionCache::clone`), not inside `select!`.

```rust
if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
    output::emit_batch(&batch);
}
```

---

## Pattern 6 — Memory sampling (userspace hot path)

Precompute `memory.current` on identity as `Arc<PathBuf>` in cache; sampler snapshots `Arc::clone` only (no per-tick `PathBuf` alloc). `spawn_blocking` + stack `[u8; 32]` read (not `read_to_string` on the runtime worker).

---

## Pattern 5a — Batch lineage (audit)

Each `Aggregator::flush` sets `batch_id = Uuid::new_v4()` and `agent_version = env!("CARGO_PKG_VERSION")`.  
Propagated through `BatchJson` → ingest `FlatRow` → ClickHouse (not in `ORDER BY` — [ADR 017](../../../docs/adr/017-batch-lineage-metadata.md)).

## Pattern 5b — Aggregator clock domain

`Aggregator::new` calibrates `clock_offset_ns = wall_unix - CLOCK_MONOTONIC` once.  
Ring-buffer events: `wall_timestamp = event.timestamp + clock_offset_ns` in `on_finops_event`.  
`window_start_ns` / `window_end_ns` use `mono_now + offset` (not raw `SystemTime` per flush).  
Memory sampler timestamps are already wall — do not re-apply offset ([ADR 016](../../../docs/adr/016-clock-domain-offset.md)).

## Pattern 6b — Attribution cache

`AttributionCache`: one `Arc<RwLock<CacheState>>` with `FxHashMap` for paths, labels (`Arc<WorkloadLabels>`), and `pod_by_uid`.  
`labels_for_cgroup`: single `.read()` — no quadruple-lock herd; K8s/path misses cache under write lock; `DEFAULT_LABELS` `LazyLock` for unknown cgroups. `on_identity_event`: procfs read **before** `state.write()`. K8s refresh in background task.  
`cgroup_path_from_pid`: stack `[u8; 1024]` read of `/proc/{pid}/cgroup` (no `read_to_string` on exec path).  
Startup: `bootstrap_existing_cgroups` — `walkdir` on cgroup v2 root; dir `ino()` = `cgroup_id` ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md)).  
`parking_lot::RwLock`, cgroup v2 `split_once("::")`, `Path::components()`.

---

## Pattern 6c — Aggregator

`FxHashMap`, double buffer, flip-before-drain, early flush at `max_keys`.

---

## Pattern 7 — Batched JSON (schema v2)

Agent → API envelope unchanged; see `output::BatchJson`.

---

## Pattern 8 — OOM-safe bounds (Phase 4+)

```rust
requests = (p99 × 1.20).max(MIN_REQUESTS);
limits   = requests × 1.25;
```

---

## Pattern 9 — GitOps PR body

```markdown
## Test plan
- [ ] `make build` && `make check`
- [ ] Phase 3: `make compose-up` → `/health` + `/ready` + API `/metrics` + agent `:9091/metrics` → ingest → `SELECT count() FROM finops_telemetry FINAL` > 0
- [ ] ADR + skills + docs updated in same PR
```

---

## Pattern 10 — Phase 3 non-blocking ingest (enterprise)

**Agent:** `OnceLock<reqwest::Client>`; `FINOPS_API_TOKEN` → `default_headers` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md)); `PrometheusBuilder` → `:9091/metrics` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)); `init_retry_worker` — `mpsc(60)`; `emit_batch` → `try_send`; on `Full`, `try_lock` drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)).

**API:** `GET /health`, `GET /ready` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md)); `GET /metrics` ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md)); `expected_bearer` precomputed at startup — no per-request `format!` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)); `schema_version` `2..=3` ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md)); `try_send` — `200`/`401`/`400`/`503`.

**Kafka:** channel `(Vec<u8>, Vec<u8>)`; `bytes_to_record` moves vecs (no `to_vec`); env `FINOPS_KAFKA_*` — [ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md), [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md).

**ClickHouse:** Kafka engine settings — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md). `ReplacingMergeTree`: LC only `node`/`namespace`; `ORDER BY (node, window_start_ns, cgroup_id)`; billing `SELECT … FINAL` — [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md).

---

## Pattern 11 — Docker / Makefile (Phase 3 dev)

```bash
make compose-up    # stop-api (host binary only) + stack + health check
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
curl -s -u default:finops_dev 'http://localhost:8123/?query=SELECT%20count()%20FROM%20finops_telemetry%20FINAL'
make compose-down
```

- **Do not** `make run-api` while compose `finops-api` is on `:3000`.
- **Do not** `fuser -k 3000` — breaks Docker port-forward ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)).

Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md).

## Pattern 12 — Production container images (Target 1)

```bash
docker build -f deploy/docker/Dockerfile.gateway -t finops-gateway:latest .
docker build -f deploy/docker/Dockerfile.agent -t finops-agent:latest .
```

Gateway: non-root `finops` user ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)). Agent: root/privileged, `FINOPS_BPF_DIR=/app/bpf` ([ADR 024](../../../docs/adr/024-agent-production-container.md)).

```bash
kubectl apply -f deploy/k8s/gateway.yaml
kubectl apply -f deploy/k8s/agent-daemonset.yaml
```

See [deploy/k8s/README.md](../../../deploy/k8s/README.md) ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md)).

**API shutdown (container or host):** `with_graceful_shutdown` → drain mpsc → 10s cap ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)).

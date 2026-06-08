# Statix eBPF Agent — Patterns

Enterprise templates. **Before coding:** [SKILL.md](SKILL.md) workflow → implement → `make build` → update ADR/docs/skills.

Rules: [enterprise-latency.md](../../../docs/enterprise-latency.md). Architecture: [REFERENCE.md](REFERENCE.md).

---

## Pattern 1 — `StatixEvent` in statix-common

```rust
pub const EVENT_KIND_WORKLOAD_IDENTITY: u8 = 1;
pub const EVENT_KIND_MEMORY_SAMPLE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StatixEvent { /* kind, cgroup_id, timestamp, memory_bytes, comm, ... */ }

#[cfg(feature = "user")]
unsafe impl aya::Pod for StatixEvent {}
```

---

## Pattern 2 — Ring buffer map (statix-ebpf)

```rust
include!(concat!(env!("OUT_DIR"), "/ring_config.rs"));
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(RING_BUF_BYTES, 0);
// build.rs + make build-ebpf → target/bpf/statix-ebpf-{small,large,xlarge}; ebpf_select.rs picks by core count
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
if let Some(batch) = agg.on_statix_event(event, &cache, &node) {
    output::emit_batch(&batch);
}
```

---

## Pattern 6 — Memory sampling (userspace hot path)

Precompute `memory.current` on identity as `Arc<PathBuf>` in cache; sampler snapshots `Arc::clone` only (no per-tick `PathBuf` alloc). `spawn_blocking` + stack `[u8; 32]` read (not `read_to_string` on the runtime worker).

---

## Pattern 5a — Batch lineage (audit)

Each `Aggregator::flush` sets `batch_id = Uuid::new_v4()` and `agent_version = env!("CARGO_PKG_VERSION")`.  
Propagated through `statix_wire::IngestBatch` → `FlatRow` → ClickHouse (not in `ORDER BY` — [ADR 017](../../../docs/adr/017-batch-lineage-metadata.md), [ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md)).

## Pattern 5b — Aggregator clock domain

`init_clock_offset()` at agent startup; global `AtomicU64` in `statix-infra::clock`.  
Hot path: `clock_offset_ns()` (`Relaxed` load) — `wall = mono + offset` in `on_statix_event`.  
Background: `spawn_clock_recalibration_task` every `STATIX_CLOCK_RECALIBRATE_SECS` (default 3600).  
`window_start_ns` / `window_end_ns` use `mono_now + offset` (not `SystemTime` per event).  
Memory sampler timestamps are already wall — do not re-apply offset ([ADR 016](../../../docs/adr/016-clock-domain-offset.md), [047](../../../docs/adr/047-atomic-clock-offset-recalibration.md)).

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

Agent → API envelope: `statix_wire::IngestBatch` from `output::emit_batch`.

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
- [ ] Phase 3: `make compose-up` → `/health` + `/ready` + API `/metrics` + agent `:9091/metrics` → ingest → `SELECT count() FROM statix.workload_metrics FINAL` > 0
- [ ] ADR + skills + docs updated in same PR
```

---

## Pattern 10 — Phase 3 non-blocking ingest (enterprise)

**Agent:** `OnceLock<reqwest::Client>`; `STATIX_API_TOKEN` → `default_headers` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md)); `PrometheusBuilder` → `:9091/metrics` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)); `init_retry_worker` — `mpsc(60)`; `emit_batch` → `try_send`; on `Full`, `try_lock` drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)).

**API:** `GET /health`, `GET /ready` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md)); `GET /metrics` ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md)); `expected_bearer` precomputed at startup — no per-request `format!` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)); `schema_version` `2..=3` ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md)); `try_send` — `200`/`401`/`400`/`503`.

**Kafka:** channel `(Vec<u8>, Vec<u8>)`; `bytes_to_record` moves vecs (no `to_vec`); env `STATIX_KAFKA_*` — [ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md), [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md).

**ClickHouse:** Kafka engine settings — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md). `ReplacingMergeTree`: LC only `node`/`namespace`; `ORDER BY (node, window_start_ns, cgroup_id)`; billing `SELECT … FINAL` — [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md).

---

## Pattern 11 — Docker / Makefile (Phase 3 dev)

```bash
make compose-up    # stop-api (host binary only) + stack + health check; Grafana :3001
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
# set -a && source .env && set +a
curl -s -u "default:${CLICKHOUSE_PASSWORD}" 'http://localhost:8123/?query=SELECT%20count()%20FROM%20statix.workload_metrics%20FINAL'
make compose-down
```

- **Do not** `make run-api` while compose `statix-gateway` is on `:3000`.
- **Do not** `fuser -k 3000` — breaks Docker port-forward ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)).

Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md).

## Pattern 12 — Production container images (Target 1)

```bash
docker build -f deploy/docker/Dockerfile.gateway -t statix-gateway:latest .
docker build -f deploy/docker/Dockerfile.statix -t statix:latest .
```

Gateway: non-root `statix` user ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)). Agent: root/privileged, `STATIX_BPF_DIR=/app/bpf` ([ADR 024](../../../docs/adr/024-agent-production-container.md)).

```bash
kubectl apply -f deploy/k8s/gateway.yaml
kubectl apply -f deploy/k8s/statix-daemonset.yaml
```

See [deploy/k8s/README.md](../../../deploy/k8s/README.md) ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md)).

## Pattern 13 — ClickHouse Target 2 (`statix` database)

```bash
clickhouse-client --multiquery < deploy/clickhouse/01_init.sql
```

`statix.workload_metrics` + `kafka_telemetry_queue` + `telemetry_mv`; billing `FINAL` on `(node, window_start_ns, cgroup_id)` ([ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md)).

**API shutdown (container or host):** `with_graceful_shutdown` → drain mpsc → 10s cap ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)).

## Pattern 15 — Gateway `Config` (Phase 7)

All `statix-gateway` startup env is loaded once via `config::Config::from_env()` at the top of `main()` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md)).

| Env | `Config` field | Default |
|-----|----------------|---------|
| `KAFKA_BROKERS` | `kafka_brokers` | `localhost:9092` |
| `STATIX_API_PORT` | `api_port` | `3000` (invalid u16 → process exit) |
| `STATIX_API_TOKEN` | `api_token` | `None` |
| `CLICKHOUSE_URL` | `clickhouse_url` | `http://localhost:8123` |
| `CLICKHOUSE_USER` | `clickhouse_user` | `default` |
| `CLICKHOUSE_PASSWORD` | `clickhouse_password` | `""` |

Do not add new `std::env::var` calls in `main.rs` — extend `config.rs` instead.

## Pattern 14 — API read-path (Target 3)

```bash
curl -s 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=24' | jq .
```

- Env: `CLICKHOUSE_URL`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD` ([ADR 027](../../../docs/adr/027-api-read-path-clickhouse.md)).
- SQL uses `statix.workload_metrics FINAL`; default lookback 24h.
- Rebuild API after changes: `docker compose build statix-gateway && docker compose up -d statix-gateway`.

## Pattern 16 — Positive-bounded numeric env (`statix-infra::env`)

All numeric tuning env vars that feed timers, intervals, or channel depths must use `read_env_u64` / `read_env_usize` — never raw `parse()` in callers ([ADR 048](../../../docs/adr/048-generic-env-positive-parsing.md)).

```rust
// statix-infra/src/env.rs — internal generic; public wrappers unchanged
fn read_env_positive<T>(name: &str, default: T) -> T
where
    T: Copy + Default + PartialOrd + std::fmt::Display + FromStr,
{
    match var_with_legacy(name) {
        Some(s) => match s.parse::<T>() {
            Ok(v) if v > T::default() => v,
            _ => {
                log::warn!("Invalid {name}={s:?}; using default {default}");
                default
            }
        },
        None => default,
    }
}

pub fn read_env_u64(name: &str, default: u64) -> u64 {
    read_env_positive(name, default)
}
```

- `STATIX_WINDOW_SECS=0` → warns, uses default `10` (prevents Tokio zero-duration panic).
- Do **not** add parallel `read_env_i64` / ad-hoc `.max(1)` parsers — extend the generic if a new numeric type is needed.

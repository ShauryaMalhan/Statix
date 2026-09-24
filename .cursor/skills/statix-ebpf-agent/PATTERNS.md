# Statix eBPF Agent — Patterns

Enterprise templates. **Before coding:** [SKILL.md](SKILL.md) workflow → implement → `make build` → update ADR/docs/skills.

Rules: [enterprise-latency.md](../../../docs/guides/enterprise-latency.md). Architecture: [REFERENCE.md](REFERENCE.md).

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

`reserve` → fill → `submit(0)`. Never `?` after `reserve`. On `None`, increment `RING_DROPS` key `0` ([ADR 022](../../../docs/adr/ebpf/022-bpf-ring-buffer-drop-counter.md)); agent polls every 10s.

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

**Leaves only.** Skip any cgroup with child cgroups — its numbers already include them, so reading it double-counts. Detect with `is_leaf_cgroup`: kernfs sets a directory's `nlink` to 2 + subdirectories, so `nlink == 2` = leaf (one `stat`, inside the same `spawn_blocking`). Check **every tick** — cgroups gain children after boot. Prune `cpu_baseline` against what was actually read, so a cgroup that flips parent→leaf re-primes instead of emitting one huge delta ([ADR 066](../../../docs/adr/agent/066-sample-leaf-cgroups-only.md)).

---

## Pattern 5a — Batch lineage (audit)

Each `Aggregator::flush` sets `batch_id = Uuid::new_v4()` and `agent_version = env!("CARGO_PKG_VERSION")`.  
Propagated through `statix_wire::IngestBatch` → gateway `MetricRow` → ClickHouse (not in `ORDER BY` — [ADR 017](../../../docs/adr/ingest/017-batch-lineage-metadata.md), [ADR 028](../../../docs/adr/meta/028-finops-wire-and-agent-rename.md)).

## Pattern 5b — Aggregator clock domain

Window bounds come from `wall_unix_ns()` (`statix-infra::clock`), read directly in `flush`:
one reading per flush, passed to `reset_window`, so one window ends exactly where the next
begins. Nothing per-event is converted to wall time.

Do **not** reintroduce a cached monotonic→wall offset. It goes stale on any host pause
(laptop sleep, hypervisor pause, live migration) and stamps rows minutes or hours in the
past ([ADR 063](../../../docs/adr/agent/063-wall-clock-window-bounds.md), supersedes 016 and 047).

## Pattern 6b — Attribution cache

`AttributionCache`: one `Arc<RwLock<CacheState>>` with `FxHashMap` for paths, labels (`Arc<WorkloadLabels>`), and `pod_by_uid`.  
`labels_for_cgroup`: single `.read()` — no quadruple-lock herd; K8s/path misses cache under write lock; `DEFAULT_LABELS` `LazyLock` for unknown cgroups. `on_identity_event`: procfs read **before** `state.write()`. K8s refresh in background task.  
`cgroup_path_from_pid`: stack `[u8; 1024]` read of `/proc/{pid}/cgroup` (no `read_to_string` on exec path).  
Startup: `bootstrap_existing_cgroups` — `walkdir` on cgroup v2 root; dir `ino()` = `cgroup_id` ([ADR 015](../../../docs/adr/agent/015-cgroup-v2-bootstrap-on-startup.md)). **Registers only**: never feed the aggregator a synthetic event from here. The aggregator can't tell it from a real exec (phantom `exec_count`), and parents would get zero rows ([ADR 068](../../../docs/adr/agent/068-bootstrap-registers-only.md)).  
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

## Pattern 10 — Phase 13 queue-less ingest

**Agent:** `OnceLock<reqwest::Client>`; circuit breaker + WAL on sustained 503 ([ADR 054](../../../docs/adr/ingest/054-phase11-wal-spillway.md)); `init_retry_worker` — `mpsc(60)` + disk spillway.

**API:** `GET /health` (writer channel open); `GET /ready` (`ch_healthy` + mpsc &lt;80%); `POST /ingest` builds `MetricRow::from_ingest` inline into permits — Tier 1 `!ch_healthy`→503, Tier 2 `try_reserve_many`→503 ([ADR 055](../../../docs/adr/ingest/055-phase13-part1-kafka-removal-rowbinary.md), [056](../../../docs/adr/gateway/056-phase13-part2-ingest-zero-alloc.md)); `schema_version` `2..=3`; 2MB body limit.

**Writer:** `clickhouse_writer.rs` — coalesce `MetricRow`; RowBinary INSERT; sync `insert.end()` timeout; env `STATIX_CH_*`, `STATIX_INGEST_CHANNEL_SIZE` ([ADR 056](../../../docs/adr/gateway/056-phase13-part2-ingest-zero-alloc.md)).

**ClickHouse:** `statix.workload_metrics` only (no Kafka engine); `ReplacingMergeTree`; billing `FINAL` — [ADR 007](../../../docs/adr/storage/007-clickhouse-mergetree-tuning.md), [ADR 011](../../../docs/adr/storage/011-replacingmergetree-dedupe-identity.md).

*(Historical Kafka path: [ADR 005](../../../docs/adr/ingest/005-non-blocking-ingest-pipeline.md), [ADR 010](../../../docs/adr/kafka-legacy/010-kafka-partition-key-by-node.md), [ADR 014](../../../docs/adr/kafka-legacy/014-kafka-producer-env-tuning.md).)*

---

## Pattern 6d — CPU sampling (cumulative counter → per-window delta)

`cpu.stat` `usage_usec` is cumulative — store **delta** per window, not the raw counter.

- Baseline map (`Sampler.cpu_baseline`) survives aggregator window flips; **not** in `WorkloadStats`.
- **Priming:** first read per cgroup sets baseline only (delta 0) — avoids lifetime spike on boot.
- **Monotonic guard:** `current.saturating_sub(last)` on subsequent samples.
- **One sample per window:** every window closes through `sample_and_flush` (`main.rs`) — `sampler.tick`, then `agg.flush` — from the flush timer and both shutdown arms. Never call `agg.flush` directly, and never put sampling on its own timer. Two timers with the same period race in `select!` (random pick among ready branches): some windows get no sample (0 memory/CPU), others two (double CPU) ([ADR 067](../../../docs/adr/agent/067-sample-inside-flush.md)).
- Same tick as memory: `for_each_sample_target` → one `spawn_blocking` reads both files ([ADR 058](../../../docs/adr/agent/058-phase14-cpu-usage-tracking.md)).
- Agent emits schema v3 with `cpu_usage_usec`; gateway accepts v2..=3 (`#[serde(default)]`).

---

## Pattern 17 — Background gauge sampler + startup metric seed (Phase 10)

**When:** a gauge must reflect async state (mpsc depth, WAL bytes) without touching the hot path.

**Sampler (gateway mpsc):**
- Dedicated `tokio::spawn` loop; period from `STATIX_MPSC_DEPTH_SAMPLE_MS` via `read_env_u64` (Pattern 16).
- Depth = `channel_capacity − tx.capacity()` (same math as `/ready`).
- Seed gauge to `0.0` before the first tick.

**Startup seed:**
- `metrics` series absent from `/metrics` until first touch — seed counters with `increment(0)` and gauges with `set(0.0)` at startup so idle `0` is visible.
- Optional `describe_gauge!` / `describe_counter!` once at startup for HELP text ([ADR 060](../../../docs/adr/observability/060-phase10-golden-signal-saturation-metrics.md)).

**503 flat counter:** increment in the shared response recorder (`record_ingest_metrics`), not per rejection branch.

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
- **Do not** `fuser -k 3000` — breaks Docker port-forward ([ADR 009](../../../docs/adr/deploy/009-finops-api-docker-compose.md)).

Validate: [docs/guides/phase3-validation.md](../../../docs/guides/phase3-validation.md).

## Pattern 12 — Production container images (Target 1)

```bash
docker build -f deploy/docker/Dockerfile.gateway -t statix-gateway:latest .
docker build -f deploy/docker/Dockerfile.statix -t statix:latest .
```

Gateway: non-root `statix` user ([ADR 009](../../../docs/adr/deploy/009-finops-api-docker-compose.md)). Agent: root/privileged, `STATIX_BPF_DIR=/app/bpf` ([ADR 024](../../../docs/adr/deploy/024-agent-production-container.md)).

```bash
kubectl apply -f deploy/k8s/gateway.yaml
kubectl apply -f deploy/k8s/statix-daemonset.yaml
```

See [deploy/k8s/README.md](../../../deploy/k8s/README.md) ([ADR 025](../../../docs/adr/deploy/025-kubernetes-gateway-and-agent.md)).

## Pattern 13 — ClickHouse Target 2 (`statix` database)

```bash
clickhouse-client --multiquery < deploy/clickhouse/01_init.sql
```

`statix.workload_metrics` only; billing `FINAL` on `(node, window_start_ns, cgroup_id)` ([ADR 026](../../../docs/adr/storage/026-clickhouse-finops-database-init.md), [ADR 055](../../../docs/adr/ingest/055-phase13-part1-kafka-removal-rowbinary.md)).

**API shutdown (container or host):** `with_graceful_shutdown` → drain mpsc → 10s cap ([ADR 005](../../../docs/adr/ingest/005-non-blocking-ingest-pipeline.md)).

## Pattern 15 — Gateway `Config` (Phase 7)

All `statix-gateway` startup env is loaded once via `config::Config::from_env()` at the top of `main()` ([ADR 030](../../../docs/adr/gateway/030-finops-api-config-struct.md)).

| Env | `Config` field / module | Default |
|-----|-------------------------|---------|
| `STATIX_INGEST_CHANNEL_SIZE` | `clickhouse_writer` mpsc | `8192` (min 1024) |
| `STATIX_CH_BATCH_MAX` | writer coalesce | `1024` (64–16384) |
| `STATIX_CH_LINGER_MS` | writer coalesce | `50` (1–1000) |
| `STATIX_CH_INSERT_TIMEOUT_SECS` | insert ACK timeout | `3` (1–30; must be &lt; agent HTTP timeout) |
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

- Env: `CLICKHOUSE_URL`, `CLICKHOUSE_USER`, `CLICKHOUSE_PASSWORD` ([ADR 027](../../../docs/adr/gateway/027-api-read-path-clickhouse.md)).
- SQL uses `statix.workload_metrics FINAL`; default lookback 24h.
- Rebuild API after changes: `docker compose build statix-gateway && docker compose up -d statix-gateway`.

## Pattern 16 — Positive-bounded numeric env (`statix-infra::env`)

All numeric tuning env vars that feed timers, intervals, or channel depths must use `read_env_u64` / `read_env_usize` — never raw `parse()` in callers ([ADR 048](../../../docs/adr/meta/048-generic-env-positive-parsing.md)).

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

## Pattern 17 — Disk WAL spillway (Phase 11, `statix/src/wal/`)

When the in-memory retry queue saturates or the gateway is down, batches spill to
a bounded segmented append-only log on disk instead of being dropped — preserving
FinOps zero-data-loss. Full spec: [PHASE_11_WAL_PLAYBOOK.md](PHASE_11_WAL_PLAYBOOK.md), [ADR 054](../../../docs/adr/ingest/054-phase11-wal-spillway.md).

```rust
// statix/src/output.rs — enqueue_batch_json Full branch (hot path)
Err(mpsc::error::TrySendError::Full(json)) => {
    // try_append is a NON-BLOCKING try_send to the dedicated writer thread —
    // no disk I/O on the ring-buffer hot path. Returns the payload back on failure.
    let json = match WAL.get() {
        Some(wal) => match wal.try_append(json) {
            Ok(()) => return,
            Err(json) => json,
        },
        None => json,
    };
    // ...last-resort legacy drop-oldest only if WAL disabled / channel full
}
```

- **Storage:** segmented `seg-<seq>.wal`, CRC32-framed `[u32 len][u32 crc][u64 seq][payload]`; `bytes::Bytes` written verbatim (zero-copy). Never SQLite/mmap (write amplification / SIGBUS on ENOSPC).
- **Writer:** one `std::thread` (`statix-wal-writer`) owns the active fd; `fdatasync` (not `fsync`) group-commit. Never `spawn_blocking` (pool starvation).
- **Recovery:** `wal::recovery::recover()` at boot truncates torn tails (CRC/len), drops corrupt-header segments, rebuilds cursors from segments (superblock advisory).
- **Circuit:** `Closed/HalfOpen/Open` driven by retry-worker POST outcomes (`record_post_success/failure`) — no health polling. Drainer replays oldest-first, staggered by node-hash spread.
- **Bounds:** `STATIX_WAL_MAX_BYTES` drop-oldest at cap; ENOSPC → metric + truncate, never panic. At-least-once; deduped by `ReplacingMergeTree`.
- **Verify:** `make wal-test` (unit/integration), `make wal-faultfs` (root tmpfs ENOSPC).

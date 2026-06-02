# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5** — production-critical blockers. Bearer auth, schema 2..=3, `/ready` (Kafka metadata) shipped. **Regressions found:** write lock held across procfs I/O in `on_identity_event`; ring-drop Prometheus metric is a no-op (agent has no exporter). Both are P0 fixes.

**Completed:** Phases 1–3 (E2E ingest), **Phase 4** (scale & reliability). Phase 6 partially shipped (lock consolidation, Arc, FxHashMap) with regressions noted below. Roadmap: [ADR 018](../../../docs/adr/018-phase-roadmap-status.md).

**Validate after infra changes:** [phase3-validation.md](../../../docs/phase3-validation.md) (stack + agent + ClickHouse `FINAL`; ingest auth when `FINOPS_API_TOKEN` set). After API code changes: `docker compose build finops-api && docker compose up -d finops-api` — stale image returns **404** on `/ready` even though source has the route.

**Build tip:** `cargo check -p finops-api -p finops-user` for compile verify; `--release` ~10–15 min cold; `make build` includes eBPF.

---

## Phase 4 — Scale & reliability ✅ complete

### P1 — Before AWS ECS / production billing

- [x] **Kafka partition routing (1.1):** `node` as Kafka key + `DefaultHasher % partitions`; multi `PartitionClient` from broker metadata ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Agent ingest retry (3.2):** Background worker in `output.rs` — bounded queue 60, env backoff + 30% jitter on 5xx/429/transport; sync `try_lock` drop-oldest when full ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` + `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries use `FINAL` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md))
- [x] **Prometheus metrics (3.5):** `GET /metrics`; ingest counter/histogram; channel full + depth gauge; Kafka produce histogram ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md))

### P2 — Scale & audit correctness

- [x] **Ring buffer size (1.2):** `build.rs` + three ELFs (`target/bpf/`); CPU-tier auto-load in `ebpf_select.rs` ([ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md))
- [x] **Clock domain offset (4.1):** `clock_offset_ns` in `Aggregator::new`; BPF `timestamp` + offset; window bounds via same domain ([ADR 016](../../../docs/adr/016-clock-domain-offset.md))
- [x] **Data lineage (4.6):** `batch_id` (UUID v4 per flush) + `agent_version` on wire and ClickHouse ([ADR 017](../../../docs/adr/017-batch-lineage-metadata.md))

### P3 — Coverage & horizontal API

- [x] **Bootstrap running workloads (1.7):** `bootstrap_existing_cgroups` walks cgroup v2; inode = `cgroup_id`; synthetic identity events ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md))

### Ingest hardening (shipped with Phase 4)

- [x] **Kafka producer env tuning:** `FINOPS_KAFKA_CHANNEL_SIZE` / `BATCH_MAX` / `LINGER_MS` in `kafka.rs` ([ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md))
- [x] **Zero-copy node key in `ingest.rs`:** `KafkaQueueItem` key once per batch ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))

---

## Phase 5 — Production-critical blockers (active)

> Gates real deployment. Fix regressions first, then ship remaining infra items.

### P0 — Regressions & critical fixes

- [ ] **Fix `on_identity_event` write lock across procfs I/O:** `state.write()` is acquired _before_ `cgroup_path_from_pid(event.pid)` which does `open()` + `read()` + `close()` on `/proc/{pid}/cgroup` — 3 syscalls under exclusive lock. All readers (`labels_for_cgroup`, `for_each_memory_current_path`, K8s `upsert_pod_labels`) block behind procfs. **Fix:** read `/proc/{pid}/cgroup` before acquiring the write lock, then lock and insert.
- [ ] **Agent Prometheus exporter (ring drops metric is a no-op):** Agent has `metrics = "0.24"` but no `metrics-exporter-prometheus` and no recorder installed. `metrics::counter!("finops_agent_ring_drops_total").absolute(...)` writes to a no-op sink. Add `metrics-exporter-prometheus` to `finops-user/Cargo.toml`, install recorder at startup, serve `/metrics`. Without this, the ring-drop counter shipped in [ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md) is log-only.
- [ ] **Cache default `Arc<WorkloadLabels>` in `labels_for_cgroup`:** Fallback path (line 91 in `attribution.rs`) allocates `Arc::new(WorkloadLabels::default())` on every call for non-K8s cgroups. Use a `static LazyLock<Arc<WorkloadLabels>>` and return `Arc::clone`. Also: line 86 merges labels but doesn't write them back to `cgroup_labels`, so the next call for the same cgroup_id re-merges + re-allocates.

### P0 — Data integrity & security

- [x] **Bearer auth on `POST /ingest`:** API `AppState.api_token` + `401` on bad/missing `Authorization`; agent `init_http_client()` sets `reqwest` `default_headers` with `Bearer` from `FINOPS_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md))
- [ ] **TLS on `POST /ingest`:** Terminate HTTPS at LB/sidecar or gateway — bearer over plaintext means the token is sniffable on the network
- [x] **BPF ring buffer overflow counter:** `RING_DROPS` per-CPU array + agent polls every 10s + `log::error!` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md)). **Note:** Prometheus export blocked on agent exporter (above).
- [x] **Schema evolution:** Ingest accepts `schema_version` 2..=3 during rolling upgrades ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md))

### P1 — Operational readiness

- [x] **API `/ready` probe:** `kafka_ready` `AtomicBool` after `load_partition_clients`; `/ready` = ready + `!tx.is_closed()` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md))
- [ ] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc > 80% full (gauge exists; not wired to `/ready` yet)
- [ ] **Production ClickHouse `kafka_num_consumers`:** Set = Kafka topic partition count in env-specific SQL. With 1 consumer and N partitions, throughput is 1/Nth. ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `finops-telemetry` topic. Without explicit retention, brokers fill disk → throttling → consumer lag → data loss.
- [ ] **ClickHouse `kafka_skip_broken_messages` alerting:** Currently 1000 — up to 1000 rows silently dropped per block. Monitor `system.kafka_consumers` when skipped > 0.

---

## Phase 6 — Mechanical sympathy (hot-path performance)

> Lock consolidation, Arc, FxHashMap shipped. Remaining items are micro-alloc wins and regression fixes.

### Hot-path lock contention (shipped)

- [x] **`enqueue_batch_json` queue-full path:** `rx_arc.try_lock()` drop-oldest — no `tokio::spawn` ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation:** Single `RwLock<CacheState>`; one read guard per lookup. **Caveat:** introduced write-lock-across-procfs regression (fix tracked in Phase 5 P0).
- [x] **`AttributionCache`: `std::HashMap` → `FxHashMap`:** All `CacheState` maps use `FxHashMap` (`rustc-hash` 1.1)

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`:** Cache + `WorkloadStats`; flush uses `s.labels` only. **Caveat:** fallback path still allocates per-call (fix tracked in Phase 5 P0).
- [ ] **Split `WorkloadStats` into hot/cold:** Hot counters (24 bytes: exec_count, sample_count, memory_bytes_max, memory_bytes_last) share cache lines with cold label `Arc` (~120 bytes). Separate so hot fields fit one cache line.
- [ ] **Cache `agent_version` as `&'static str`:** `env!("CARGO_PKG_VERSION").to_string()` heap-allocates on every flush. Use `&'static str` in `BatchPayload`.
- [ ] **UUID without syscall:** `Uuid::new_v4()` calls `getrandom(2)` every flush. Seed a thread-local `SmallRng` once.
- [ ] **Cache `FINOPS_INGEST_URL` check:** `std::env::var("FINOPS_INGEST_URL")` scans the environment block on every `emit_batch` call (line 268, `output.rs`). Cache as `static OnceLock<bool>` at startup.
- [ ] **Fix `post_ingest` body clone:** `body.to_string()` (line 167, `output.rs`) clones the entire JSON payload on every POST attempt including retries. Change function to take `String` by value.
- [ ] **Dead computation in `on_finops_event`:** `mono_to_wall(event.timestamp)` (line 85, `aggregator.rs`) computed on every event but only used in `log::trace!` (disabled in production). Move inside the trace block.
- [ ] **Match guard prevents jump table:** `k if k == EVENT_KIND_MEMORY_SAMPLE =>` (line 94, `aggregator.rs`). Use constant pattern `EVENT_KIND_MEMORY_SAMPLE =>` directly; remove unused `_kind` parameter from `ingest_memory_sample_inner`.

### Gateway hot path

- [x] **Kafka queue `Vec<u8>`:** `KafkaQueueItem` = `(Vec<u8>, Vec<u8>)`; no `Bytes::to_vec()` at produce ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md)). **Note:** regressed per-row `node_vec.clone()` from O(1) refcount to O(n) memcpy — acceptable at typical node name sizes.
- [ ] **Precompute bearer token header value:** `format!("Bearer {expected_token}")` (line 66, `ingest.rs`) heap-allocates on every ingest request. Compute once at startup, store in `AppState`.
- [ ] **Reuse partition grouping HashMap:** `produce_grouped_batch` (line 188, `kafka.rs`) allocates a new `HashMap<i32, Vec<KafkaQueueItem>>` on every produce cycle. Pre-allocate and `.clear()` between batches.

### Memory sampler (shipped)

- [x] **`Arc<PathBuf>` in `memory_current_paths`:** Refcount clone per sample tick (no `to_path_buf()` storm)

---

## Phase 7 — Architecture & developer experience

### Crate structure

- [ ] **`finops-wire` crate:** Shared JSON wire format between agent and gateway. Eliminates type duplication: `BatchJson`/`IngestBatch`, `WorkloadBatchRow`/`WorkloadRow`/`FlatRow`, `SCHEMA_VERSION` (agent) vs `MIN/MAX_SCHEMA_VERSION` (gateway). Schema changes become compile-time errors instead of production mismatches.
- [ ] **Centralized `Config` struct:** 20+ `std::env::var` calls scattered across 6 files, some read multiple times (`FINOPS_INGEST_URL` at startup, spawn, and every emit; `FINOPS_NODE_NAME`/`NODE_NAME` in `main.rs` and `attribution.rs`). Parse once into a typed struct, pass by reference. Catches typos at startup (e.g., `FINOPS_EBF_PATH` in `ebpf_select.rs` — missing 'P' in 'EBPF').
- [ ] **Rename crates:** `finops-user` → `finops-agent`, `finops-api` → `finops-gateway` (reflects actual roles)
- [ ] **Remove deprecated `ProcessEvent`:** Dead struct in `finops-common/src/lib.rs` with unused `Pod` impl. Remove or `#[cfg(feature = "deprecated")]`-gate.

### Error handling

- [ ] **`thiserror` for gateway errors:** Structured error types enable proper HTTP status codes + machine-readable JSON error bodies. Currently `StatusCode::UNAUTHORIZED` returns an empty 401 with no diagnostic info for the client.
- [ ] **Typed errors in `attribution.rs`:** Replace `anyhow` in library-like code with enum errors (cgroup not found, proc parse failure, K8s API error) so callers can match on specific failure modes.

---

## Phase 8 — Kubernetes & deployment

- [ ] **DaemonSet + RBAC YAML:** Production-ready manifests with resource limits, node affinity, hostPID/hostNetwork as needed.
- [ ] **Reuse `kube::Client` across K8s refresh polls:** `refresh_k8s_pods` calls `kube::Client::try_default().await` every 30s — reads service account token from disk, constructs HTTP+TLS client each time. Create the client once in the spawn and pass by reference.
- [ ] **K8s informer (replace 30s poll):** `kube-runtime` watch stream. Defer until ~500+ pods/node — 30s poll works below that. Over-investment trap.
- [ ] **Stronger cgroup → pod mapping:** Path-based UID extraction is fragile across CRI runtimes (containerd vs CRI-O vs Docker). Consider kubelet stats endpoint or downward API.
- [ ] **Graceful rolling update drain:** During DaemonSet update, events between old agent shutdown and new agent startup are lost. Investigate BPF map pinning so new process inherits ring buffer.

---

## Phase 9 — Correctness & portability

- [ ] **cgroup v1-only host detection:** Agent silently produces wrong `cgroup_id` on v1-only hosts. Detect and warn or degrade gracefully.
- [ ] **arm64 eBPF CI:** Graviton/ARM instances need cross-compiled BPF ELFs.
- [ ] **eBPF verifier regression CI:** As BPF programs grow, verifier complexity limit (1M insns on 5.15+) becomes a constraint. CI should load the ELF on a target kernel and assert verifier success.
- [ ] **`FINOPS_REDACT_COMM`:** Strip process names from telemetry for security-sensitive environments.

---

## Phase 10 — Observability & cost

### Agent self-telemetry

- [ ] **Agent `/metrics` endpoint content:** Once exporter is installed (Phase 5 P0), expose: ring drops, flush duration, cache size, retry queue depth, event processing latency, memory sampler duration. Critical for operating at scale.
- [ ] **`aya-log` for dev BPF diagnostics:** BPF-side structured logging via map (not trace_pipe). Dev/debug only, do not ship to production.

### Infrastructure cost

- [ ] **Cross-AZ data transfer audit:** If agent → gateway → Kafka path crosses AZs, each hop costs $0.01/GB. At 10k nodes this compounds to thousands/month. Use AZ-aware Kafka client config or AZ-affinity routing.
- [ ] **ClickHouse merge pressure monitoring:** `ReplacingMergeTree` dedup happens at merge time. If parts accumulate faster than merges, `FINAL` queries degrade exponentially. Monitor `system.merges`, `system.parts` count; tune `max_parts_in_total`.
- [ ] **ClickHouse skip index / granularity tuning:** Default `index_granularity = 8192` is too coarse for time-range queries on `window_start_ns`. Add `SETTINGS index_granularity = 1024` or a `minmax` skip index on `window_start_ns`.

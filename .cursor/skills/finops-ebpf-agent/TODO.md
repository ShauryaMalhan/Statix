# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5** — production-critical security & readiness ([phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)).

**Completed:** Phases 1–3 (E2E ingest pipeline), **Phase 4** (scale & reliability), **Phase 6** (L8 mechanical sympathy). Roadmap snapshot: [ADR 018](../../../docs/adr/018-phase-roadmap-status.md).

**Validate after infra changes:** [phase3-validation.md](../../../docs/phase3-validation.md) (stack + agent + ClickHouse `FINAL`).

---

## Phase 4 — Scale & reliability ✅ complete

### P1 — Before AWS ECS / production billing

- [x] **Kafka partition routing (1.1):** `node` as Kafka key + `DefaultHasher % partitions`; multi `PartitionClient` from broker metadata ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Agent ingest retry (3.2):** Background worker in `output.rs` — bounded queue 60, env backoff + 30% jitter on 5xx/429/transport; drop-oldest when full ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` + `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries use `FINAL` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md))
- [x] **Prometheus metrics (3.5):** `GET /metrics`; ingest counter/histogram; channel full + depth gauge; Kafka produce histogram ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md))

### P2 — Scale & audit correctness

- [x] **Ring buffer size (1.2):** `build.rs` + three ELFs (`target/bpf/`); CPU-tier auto-load in `ebpf_select.rs` ([ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md))
- [x] **Clock domain offset (4.1):** `clock_offset_ns` in `Aggregator::new`; BPF `timestamp` + offset; window bounds via same domain ([ADR 016](../../../docs/adr/016-clock-domain-offset.md))
- [x] **Data lineage (4.6):** `batch_id` (UUID v4 per flush) + `agent_version` on wire and ClickHouse ([ADR 017](../../../docs/adr/017-batch-lineage-metadata.md))

### P3 — Coverage & horizontal API

- [x] **Bootstrap running workloads (1.7):** `bootstrap_existing_cgroups` walks cgroup v2; inode = `cgroup_id`; synthetic identity events ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md))

### Phase 3 ingest hardening (shipped under Phase 4 timeline)

- [x] **Kafka producer env tuning:** `FINOPS_KAFKA_CHANNEL_SIZE` / `BATCH_MAX` / `LINGER_MS` in `kafka.rs` ([ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md))

---

## Phase 5 — Production-critical security & readiness (active)

> Items here gate any real deployment. None are optional.

### P0 — Data integrity & security

- [ ] **TLS + auth on `POST /ingest`:** Without this, any process on the network can inject fake billing data. mTLS between agent ↔ gateway or shared bearer token at minimum. Blocks production billing trust.
- [ ] **BPF ring buffer overflow metric:** `EVENTS.reserve()` silently returns `None` on overflow — zero visibility into dropped telemetry. Add a per-CPU `ARRAY` counter map, increment on reserve failure, expose as `finops_agent_ring_drops_total` in agent metrics.
- [ ] **Schema evolution strategy:** `schema_version` is hardcoded to `2` with a hard reject on mismatch (`!= 2`). Rolling upgrades are impossible — old agents sending v2 will be rejected when API upgrades to expect v3. Accept `v(current)` and `v(current-1)` in the ingest handler.

### P1 — Operational readiness

- [ ] **API `/ready` probe:** Separate from `/health`. Check Kafka producer connectivity (not just `tx.is_closed()`), channel depth < 80% capacity. Required for ALB/K8s readiness gates.
- [ ] **Production ClickHouse `kafka_num_consumers`:** Set `kafka_num_consumers` = Kafka topic partition count in env-specific SQL. With 1 consumer and N partitions, ingestion throughput is 1/Nth. ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **Kafka retention policy:** Set `retention.ms` and `retention.bytes` on `finops-telemetry` topic. Without this, Kafka brokers fill disk → throttling → consumer lag → data loss. Alert on `LogDirSize`.
- [ ] **ClickHouse `kafka_skip_broken_messages` alerting:** Currently set to 1000 — up to 1000 rows silently dropped per block. Monitor via `system.kafka_consumers` and alert when skipped count > 0.

---

## Phase 6 — Mechanical sympathy (L8 hot path) ✅ complete

### Hot-path lock contention

- [x] **`enqueue_batch_json` queue-full path:** Drop-oldest via `rx_arc.try_lock()` — no `tokio::spawn` on retry queue full ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation:** Single `RwLock<CacheState>`; one read guard per `labels_for_cgroup` call
- [x] **`AttributionCache`: `std::HashMap` → `FxHashMap`:** All maps in `CacheState` use `FxHashMap` (`rustc-hash` 1.1)

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`:** Cache + `WorkloadStats` use `Arc`; flush reads `s.labels` only
- [ ] **Split `WorkloadStats` into hot/cold:** Hot counters (24 bytes: exec_count, sample_count, memory_bytes_max, memory_bytes_last) share cache lines with cold label metadata (~120 bytes). Split so hot counters fit one cache line.
- [ ] **Cache `agent_version` as `&'static str`:** `env!("CARGO_PKG_VERSION").to_string()` heap-allocates on every flush. Use `&'static str` in `BatchPayload`.
- [ ] **UUID without syscall:** `Uuid::new_v4()` calls `getrandom(2)` on every flush. Seed a thread-local `SmallRng` once at startup.
- [ ] **Cache `FINOPS_INGEST_URL` check:** `std::env::var()` scans the environment block on every `emit_batch` call. Cache as `static OnceLock<bool>` at startup.

### Memory sampler

- [x] **`Arc<PathBuf>` in memory_current_paths:** Sampler snapshots refcount clones only (no per-tick `to_path_buf()` storm)

### Kafka / API hot path (zero-copy)

- [x] **Kafka queue `Vec<u8>` (no `Bytes`→`to_vec`):** `KafkaQueueItem` = `(Vec<u8>, Vec<u8>)`; `rskafka` takes owned buffers directly ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))

---

## Phase 7 — Architecture & developer experience

### Crate structure

- [ ] **`finops-wire` crate:** Shared JSON wire format between agent and gateway. Eliminates type duplication (`BatchJson`/`IngestBatch`, `WorkloadBatchRow`/`WorkloadRow`/`FlatRow`, `SCHEMA_VERSION` vs hardcoded `2`). Schema changes become compile-time errors.
- [ ] **Centralized `Config` struct:** Env-var reads scattered across 6+ files, some read multiple times (`FINOPS_INGEST_URL` read at startup, spawn, and every emit). Parse once into a `Config` struct, pass by reference.
- [ ] **Rename crates for clarity:** `finops-user` → `finops-agent`, `finops-api` → `finops-gateway` (reflects actual roles in the architecture).

### Error handling

- [ ] **`thiserror` for gateway errors:** Structured error types in `finops-api` enable proper HTTP status codes and machine-readable error bodies instead of ad-hoc string formatting.
- [ ] **Typed errors in `attribution.rs`:** Replace `anyhow` in library-like code with enum errors so callers can match on specific failure modes (cgroup not found, proc parse failure, K8s API error).

---

## Phase 8 — Kubernetes & deployment

- [ ] **DaemonSet + RBAC YAML:** Production-ready manifests with resource limits, node affinity, and hostPID/hostNetwork as needed.
- [ ] **K8s informer (replace 30s poll):** `kube-runtime` watch stream. Only needed at ~500+ pods/node — the 30s poll works fine below that. Do not over-invest here early.
- [ ] **Stronger cgroup → pod mapping:** Current path-based UID extraction is fragile across CRI runtimes (containerd vs CRI-O vs Docker). Consider mapping via `/proc/{pid}/cgroup` + downward API or kubelet stats endpoint.
- [ ] **Graceful rolling update drain:** During DaemonSet update, events between old agent shutdown and new agent startup are lost. Investigate BPF map pinning so the new process inherits the ring buffer.

---

## Phase 9 — Correctness & portability

- [ ] **cgroup v1-only host detection:** Detect and warn (or degrade gracefully) on hosts without cgroup v2 unified hierarchy. Currently silently produces wrong `cgroup_id` values on v1-only.
- [ ] **arm64 eBPF CI:** Graviton/ARM instances need cross-compiled BPF ELFs. Add CI step that compiles and verifier-checks on `aarch64`.
- [ ] **eBPF verifier regression CI:** As BPF programs grow, the verifier complexity limit (1M insns on 5.15+) becomes a constraint. CI should load the ELF on a target kernel and assert verifier success.
- [ ] **`FINOPS_REDACT_COMM`:** Strip process names from telemetry for security-sensitive environments.

---

## Phase 10 — Observability & cost

### Agent self-telemetry

- [ ] **Agent Prometheus endpoint:** The agent currently has no `/metrics`. Expose: ring buffer utilization, event processing latency, cache hit rates, flush duration, retry queue depth. Critical for operating at scale.
- [ ] **`aya-log` for dev BPF diagnostics:** BPF-side structured logging via map (not trace_pipe). Dev/debug only, do not ship to production.

### Infrastructure cost

- [ ] **Cross-AZ data transfer audit:** If agent → gateway → Kafka path crosses AZs, each hop costs $0.01/GB. At 10k nodes this compounds to thousands/month. Use AZ-aware Kafka client config or AZ-affinity routing.
- [ ] **ClickHouse merge pressure monitoring:** `ReplacingMergeTree` dedup happens at merge time. Monitor `system.merges`, `system.parts` part count, and tune `max_parts_in_total` to prevent `FINAL` query degradation.
- [ ] **ClickHouse skip index:** Default `index_granularity = 8192` is too coarse for time-range queries on `window_start_ns`. Add `SETTINGS index_granularity = 1024` or a `minmax` skip index.

---

## Shipped (completed work — do not remove)

### Phase 3 — ingest hardening

- [x] **Kafka producer env tuning:** `FINOPS_KAFKA_CHANNEL_SIZE` / `BATCH_MAX` / `LINGER_MS` in `kafka.rs` ([ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md))

### Performance (shipped)

- [x] **Kafka queue `Vec<u8>` (no `Bytes`→`to_vec`):** `KafkaQueueItem` = `(Vec<u8>, Vec<u8>)`; `rskafka` takes owned buffers directly ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))

# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5** — production-critical security & readiness ([phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)). **Shipped:** bearer auth, schema 2..=3, `GET /ready`, BPF ring-drop counter. **Open:** TLS, optional `/ready` channel-depth gate, prod ClickHouse/Kafka ops.

**Completed:** Phases 1–3 (E2E ingest), **Phase 4** (scale & reliability), **Phase 6** (L8 mechanical sympathy). Roadmap: [ADR 018](../../../docs/adr/018-phase-roadmap-status.md).

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

---

## Phase 5 — Production-critical security & readiness (active)

> Gates real deployment. **P0 shipped:** bearer auth, schema window, ring-drop counter. **P1 shipped:** `/ready` (Kafka metadata). **Open:** TLS, channel-depth gate, prod CH/Kafka ops.

### P0 — Data integrity & security

- [x] **Bearer auth on `POST /ingest`:** API `AppState.api_token` + `401` on bad/missing `Authorization`; agent `init_http_client()` sets `reqwest` `default_headers` with `Bearer` from `FINOPS_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md))
- [ ] **TLS on `POST /ingest`:** Terminate HTTPS at LB/sidecar or gateway (bearer does not replace encryption in transit)
- [x] **BPF ring buffer overflow metric:** `RING_DROPS` per-CPU array; increment on `reserve` fail; agent polls every 10s → log + `finops_agent_ring_drops_total` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [x] **Schema evolution:** Ingest accepts `schema_version` 2..=3 during rolling upgrades ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md))

### P1 — Operational readiness

- [x] **API `/ready` probe:** `kafka_ready` `AtomicBool` after `load_partition_clients`; `/ready` = ready + `!tx.is_closed()` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md))
- [ ] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc &gt; 80% full (gauge exists; not wired to `/ready` yet)
- [ ] **Production ClickHouse `kafka_num_consumers`:** Match Kafka topic partition count ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `finops-telemetry`; alert on broker disk
- [ ] **ClickHouse `kafka_skip_broken_messages` alerting:** Monitor `system.kafka_consumers` when skipped &gt; 0

---

## Phase 6 — Mechanical sympathy (L8 hot path) ✅ complete

### Hot-path lock contention

- [x] **`enqueue_batch_json` queue-full path:** `rx_arc.try_lock()` drop-oldest — no `tokio::spawn` ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation:** Single `RwLock<CacheState>`; one read guard per lookup
- [x] **`AttributionCache`: `std::HashMap` → `FxHashMap`:** All `CacheState` maps use `FxHashMap` (`rustc-hash` 1.1)

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`:** Cache + `WorkloadStats`; flush uses `s.labels` only
- [ ] **Split `WorkloadStats` into hot/cold:** Separate counters from label metadata for cache-line fit
- [ ] **Cache `agent_version` as `&'static str`:** Avoid `to_string()` per flush in `BatchPayload`
- [ ] **UUID without syscall:** Thread-local RNG instead of `Uuid::new_v4()` per flush
- [ ] **Cache `FINOPS_INGEST_URL` check:** `OnceLock` at startup instead of `env::var` every `emit_batch`

### Memory sampler

- [x] **`Arc<PathBuf>` in `memory_current_paths`:** Refcount clone per sample tick (no `to_path_buf()` storm)

### Kafka / API hot path

- [x] **Kafka queue `Vec<u8>`:** `KafkaQueueItem` = `(Vec<u8>, Vec<u8>)`; no `Bytes::to_vec()` at produce ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))

---

## Phase 7 — Architecture & developer experience

### Crate structure

- [ ] **`finops-wire` crate:** Shared wire types (`BatchJson` / `IngestBatch` / `FlatRow`)
- [ ] **Centralized `Config` struct:** Parse env once; pass by reference
- [ ] **Rename crates:** `finops-user` → `finops-agent`, `finops-api` → `finops-gateway`

### Error handling

- [ ] **`thiserror` for gateway errors**
- [ ] **Typed errors in `attribution.rs`**

---

## Phase 8 — Kubernetes & deployment

- [ ] **DaemonSet + RBAC YAML**
- [ ] **K8s informer** (replace 30s poll; defer until ~500+ pods/node)
- [ ] **Stronger cgroup → pod mapping** (CRI/runtime portability)
- [ ] **Graceful rolling update drain** (BPF map pinning investigation)

---

## Phase 9 — Correctness & portability

- [ ] **cgroup v1-only host detection**
- [ ] **arm64 eBPF CI**
- [ ] **eBPF verifier regression CI**
- [ ] **`FINOPS_REDACT_COMM`**

---

## Phase 10 — Observability & cost

### Agent self-telemetry

- [ ] **Agent Prometheus endpoint:** Ring drops, flush duration, retry queue depth, cache size
- [ ] **`aya-log` for dev BPF diagnostics** (dev only)

### Infrastructure cost

- [ ] **Cross-AZ data transfer audit**
- [ ] **ClickHouse merge pressure monitoring**
- [ ] **ClickHouse skip index** for `window_start_ns` range queries

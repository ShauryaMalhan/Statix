# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5** — TLS, `/ready` channel-depth gate, prod ClickHouse/Kafka ops. P0 regressions and blockers shipped ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)).

**Completed:** Phases 1–3 (E2E ingest), **Phase 4** (scale & reliability), **Phase 6** (L8 hot path + P0 fixes in ADR 023). Roadmap: [ADR 018](../../../docs/adr/018-phase-roadmap-status.md).

**Validate after infra changes:** [phase3-validation.md](../../../docs/phase3-validation.md) (stack + agent + ClickHouse `FINAL`; ingest auth when `FINOPS_API_TOKEN` set). After API code changes: `docker compose build finops-api && docker compose up -d finops-api` — stale image returns **404** on `/ready`.

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

> Gates real deployment. P0 shipped; remaining items are TLS and prod ops.

### P0 — Regressions & critical fixes ✅

- [x] **Fix `on_identity_event` write lock across procfs I/O:** procfs read before `state.write()` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Agent Prometheus exporter:** `metrics-exporter-prometheus` on `:9091/metrics` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Cache labels in `labels_for_cgroup`:** `DEFAULT_LABELS` `LazyLock`; K8s/path misses write back to `cgroup_labels` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))

### P0 — Data integrity & security

- [x] **Bearer auth on `POST /ingest`:** `AppState.expected_bearer` + `401`; agent `default_headers` from `FINOPS_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **TLS on `POST /ingest`:** Terminate HTTPS at LB/sidecar or gateway — bearer over plaintext means the token is sniffable on the network
- [x] **BPF ring buffer overflow counter:** `RING_DROPS` + log + `finops_agent_ring_drops_total` on `:9091` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [x] **Schema evolution:** Ingest accepts `schema_version` 2..=3 ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md))

### P1 — Operational readiness

- [x] **API `/ready` probe:** `kafka_ready` after `load_partition_clients` ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md))
- [ ] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc > 80% full
- [ ] **Production ClickHouse `kafka_num_consumers`:** Match Kafka topic partition count ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `finops-telemetry`
- [ ] **ClickHouse `kafka_skip_broken_messages` alerting:** Monitor `system.kafka_consumers` when skipped > 0

---

## Phase 6 — Mechanical sympathy (hot-path performance)

> Core L8 items shipped; micro-optimizations below remain.

### Hot-path lock contention ✅

- [x] **`enqueue_batch_json` queue-full path:** sync `try_lock` drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation:** single `RwLock<CacheState>` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md) procfs + cache fixes)
- [x] **`AttributionCache`: `FxHashMap`:** all `CacheState` maps ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md))

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`:** cache + `WorkloadStats`
- [x] **Precompute bearer token header value:** `expected_bearer` at API startup ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **Split `WorkloadStats` into hot/cold:** cache-line fit for counters vs label `Arc`
- [ ] **Cache `agent_version` as `&'static str`:** avoid `to_string()` per flush
- [ ] **UUID without syscall:** thread-local RNG per flush
- [ ] **Cache `FINOPS_INGEST_URL` check:** `OnceLock` in `emit_batch`
- [ ] **Fix `post_ingest` body clone:** take `String` by value on retry path
- [ ] **Dead computation in `on_finops_event`:** `mono_to_wall` only inside `trace!`
- [ ] **Match guard → const pattern:** `EVENT_KIND_MEMORY_SAMPLE` in aggregator

### Gateway hot path

- [x] **Kafka queue `Vec<u8>`:** ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [ ] **Reuse partition grouping HashMap:** `.clear()` between batches in `produce_grouped_batch`

### Memory sampler ✅

- [x] **`Arc<PathBuf>` in `memory_current_paths`**

---

## Phase 7 — Architecture & developer experience

### Crate structure

- [ ] **`finops-wire` crate:** shared wire types agent ↔ gateway
- [ ] **Centralized `Config` struct:** parse env once
- [ ] **Rename crates:** `finops-user` → `finops-agent`, `finops-api` → `finops-gateway`
- [ ] **Remove deprecated `ProcessEvent`**

### Error handling

- [ ] **`thiserror` for gateway errors**
- [ ] **Typed errors in `attribution.rs`**

---

## Phase 8 — Kubernetes & deployment

- [x] **Production gateway image:** `deploy/docker/Dockerfile.gateway` ([deploy/docker/README.md](../../../deploy/docker/README.md))
- [x] **Production agent image:** `deploy/docker/Dockerfile.agent` — eBPF bundle in `/app/bpf`, `finops-agent` entrypoint, root runtime ([ADR 024](../../../docs/adr/024-agent-production-container.md))
- [x] **DaemonSet + RBAC YAML:** `deploy/k8s/gateway.yaml`, `agent-daemonset.yaml` ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md))
- [ ] **Reuse `kube::Client` across K8s refresh polls**
- [ ] **K8s informer** (defer until ~500+ pods/node)
- [ ] **Stronger cgroup → pod mapping**
- [ ] **Graceful rolling update drain**

---

## Phase 9 — Correctness & portability

- [ ] **cgroup v1-only host detection**
- [ ] **arm64 eBPF CI**
- [ ] **eBPF verifier regression CI**
- [ ] **`FINOPS_REDACT_COMM`**

---

## Phase 10 — Observability & cost

### Agent self-telemetry

- [x] **Agent `/metrics` endpoint (baseline):** `:9091` + `finops_agent_ring_drops_total` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **Extended agent metrics:** flush duration, retry queue depth, cache size, event latency
- [ ] **`aya-log` for dev BPF diagnostics** (dev only)

### Infrastructure cost

- [ ] **Cross-AZ data transfer audit**
- [ ] **ClickHouse merge pressure monitoring**
- [ ] **ClickHouse skip index / granularity tuning**

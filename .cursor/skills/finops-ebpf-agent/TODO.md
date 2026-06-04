# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5** — TLS, `/ready` channel-depth gate, prod Kafka/CH tuning ([phase5-production-readiness.md](../../../docs/phase5-production-readiness.md)).

**Completed:** Phases 1–4, **6** (L8 + [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)). **Targets 1–3** (packaging, CH init, API read-path). Roadmap: [ADR 018](../../../docs/adr/018-phase-roadmap-status.md).

**Validate:** [phase3-validation.md](../../../docs/phase3-validation.md). After API route changes: `docker compose build finops-api && docker compose up -d finops-api`. After CH schema change: `docker compose down -v && make compose-up`. Billing table: `finops.workload_metrics FINAL`.

**Build tip:** `cargo check -p finops-api -p finops-user`; full stack `make build`; prod images `deploy/docker/README.md`.

---

## Targets — Packaging & data engineering ✅

| Target | Shipped | ADRs / paths |
|--------|---------|----------------|
| **1 — Images + K8s** | [x] | `deploy/docker/Dockerfile.{gateway,agent}`, `deploy/k8s/*.yaml` — [024](../../../docs/adr/024-agent-production-container.md), [025](../../../docs/adr/025-kubernetes-gateway-and-agent.md) |
| **2 — ClickHouse init** | [x] | Single script `deploy/clickhouse/01_init.sql` (Compose + prod) — [026](../../../docs/adr/026-clickhouse-finops-database-init.md) |
| **3 — API read-path** | [x] | `GET /api/v1/workloads/summary` + `CLICKHOUSE_*` — [027](../../../docs/adr/027-api-read-path-clickhouse.md) |

---

## Phase 4 — Scale & reliability ✅ complete

### P1 — Before AWS ECS / production billing

- [x] **Kafka partition routing (1.1):** `node` as Kafka key + `DefaultHasher % partitions`; multi `PartitionClient` from broker metadata ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Agent ingest retry (3.2):** Background worker in `output.rs` — bounded queue 60, env backoff + 30% jitter on 5xx/429/transport; sync `try_lock` drop-oldest when full ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` + `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries use `FINAL` on `finops.workload_metrics` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md))
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

> P0 regressions shipped ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)). Open: TLS, prod ops tuning.

### P0 — Regressions & critical fixes ✅

- [x] **Fix `on_identity_event` write lock across procfs I/O** ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Agent Prometheus exporter:** `:9091/metrics` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Cache labels in `labels_for_cgroup`:** `DEFAULT_LABELS` + K8s/path write-back ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))

### P0 — Data integrity & security

- [x] **Bearer auth:** `expected_bearer` + agent `FINOPS_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **TLS on `POST /ingest`:** Terminate HTTPS at LB/sidecar or gateway
- [x] **BPF ring buffer overflow counter:** `RING_DROPS` + `finops_agent_ring_drops_total` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [x] **Schema evolution:** `schema_version` 2..=3 ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md))

### P1 — Operational readiness

- [x] **API `/ready` probe** ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md))
- [ ] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc > 80% full
- [ ] **Production `kafka_num_consumers`:** Match topic partitions on `finops.kafka_telemetry_queue` ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md), [ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `finops-telemetry`
- [ ] **ClickHouse broken-message alerting:** `kafka_skip_broken_messages` shipped in SQL; monitor `system.kafka_consumers` when skipped > 0

---

## Phase 6 — Mechanical sympathy ✅ (micro-opts remain)

### Hot-path lock contention ✅

- [x] **`enqueue_batch_json` queue-full path:** sync `try_lock` drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation** ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **`AttributionCache`: `FxHashMap`** ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md))

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`**
- [x] **Precompute bearer:** `expected_bearer` at startup ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **Split `WorkloadStats` hot/cold**
- [ ] **Cache `agent_version` as `&'static str`**
- [ ] **UUID without syscall** (thread-local RNG)
- [ ] **Cache `FINOPS_INGEST_URL` check** (`OnceLock`)
- [ ] **Fix `post_ingest` body clone**
- [ ] **Dead `mono_to_wall` in `on_finops_event`**
- [ ] **Match guard → const pattern** in aggregator

### Gateway hot path

- [x] **Kafka queue `Vec<u8>`** ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [ ] **Reuse partition grouping HashMap** in `produce_grouped_batch`

### Memory sampler ✅

- [x] **`Arc<PathBuf>` in `memory_current_paths`**

---

## Phase 7 — Architecture & developer experience

- [ ] **`finops-wire` crate**
- [ ] **Centralized `Config` struct**
- [ ] **Rename crates:** `finops-user` → `finops-agent`, `finops-api` → `finops-gateway`
- [ ] **Remove deprecated `ProcessEvent`**
- [ ] **`thiserror` for gateway errors**
- [ ] **Typed errors in `attribution.rs`**

---

## Phase 8 — Kubernetes & deployment (base shipped)

- [x] **Production gateway image:** `deploy/docker/Dockerfile.gateway` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- [x] **Production agent image:** `deploy/docker/Dockerfile.agent` ([ADR 024](../../../docs/adr/024-agent-production-container.md))
- [x] **K8s manifests:** `deploy/k8s/gateway.yaml`, `agent-daemonset.yaml` ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md))
- [ ] **Pin images to registry digests** (replace `:latest` in manifests)
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

- [x] **Agent `/metrics` baseline:** `:9091` + ring drops ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **Extended agent metrics:** flush duration, retry depth, cache size
- [ ] **`aya-log` for dev BPF** (dev only)
- [ ] **Cross-AZ data transfer audit**
- [ ] **ClickHouse merge pressure monitoring**
- [ ] **ClickHouse skip index / granularity tuning**

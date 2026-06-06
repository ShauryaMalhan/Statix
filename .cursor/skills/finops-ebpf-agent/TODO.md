# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5.5 V2** — L8 Audit V2 distributed hardening + micro-architecture fixes.

**Completed:** Phases 1–4, **5.5 V1** (L8 P0/P1/P2), **6**, **7**, **9** (eBPF CI). **Targets 1–3** (packaging, CH init, API read-path).

**Validate:** [phase3-validation.md](../../../docs/phase3-validation.md). After gateway route changes: `docker compose build finops-gateway && docker compose up -d finops-gateway`. After CH schema change: `docker compose down -v && make compose-up`. Billing table: `finops.workload_metrics FINAL`.

**Build tip:** `cargo check --workspace`; full stack `make build`; prod images `deploy/docker/README.md`.

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

## Phase 5 — Production-critical blockers (TLS remains)

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
- [x] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc > 80% full ([ADR 029](../../../docs/adr/029-ready-channel-depth-gate.md))
- [ ] **Production `kafka_num_consumers`:** Match topic partitions on `finops.kafka_telemetry_queue` ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md), [ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `finops-telemetry`
- [ ] **ClickHouse broken-message alerting:** `kafka_skip_broken_messages` shipped in SQL; monitor `system.kafka_consumers` when skipped > 0

---

## Phase 5.5 — L8 Audit V1 fixes ✅

> Playbook V1: [L8-AUDIT-FIXES.md](L8-AUDIT-FIXES.md). Shipped fixes are removed from the playbook; historical record in ADRs.

### P0-SHIP — Shipped ✅

- [x] **Agent hot-path P0 fixes** — [ADR 032](../../../docs/adr/032-phase55-l8-p0-hot-path-fixes.md) (OnceLock env, thread-local RNG, static `agent_version`, `DEFAULT_LABELS`, move `BatchPayload`, batched `spawn_blocking`, ring drain budget)

### P1-WEEK — Shipped ✅

- [x] **Gateway + agent P1 fixes** — [ADR 033](../../../docs/adr/033-phase55-l8-p1-week-gateway-fixes.md) (`Bytes` retry body, reuse `by_partition` + batch `Utc::now`, cached `kube::Client`, Kafka metadata refresh, `argMax` summary query)

### P2-SPRINT — Shipped ✅

- [x] **Ingest zero-copy hot path** — [ADR 034](../../../docs/adr/034-phase55-l8-p2-ingest-zero-copy.md) (`Arc<[u8]>` node key, `FlatRowRef` serialization)

---

## Phase 5.5 V2 — L8 Audit V2 fixes (ACTIVE)

> Playbook V2: [L8_AUDIT_V2_FIXES.md](L8_AUDIT_V2_FIXES.md). Fixes for Level 2 micro-architecture and distributed failure modes.

### P0-BLOCKS-GA — Data Integrity & Availability

- [ ] **V2-1: Agent SIGTERM handler** — Add `tokio::signal::unix::SignalKind::terminate()` to main select loop; flush partial window on SIGTERM (`finops-agent/src/main.rs`)
- [ ] **V2-2: `ReplacingMergeTree(window_end_ns)` version column** — Without version column, ClickHouse keeps arbitrary row during merge; billing data integrity risk (`deploy/clickhouse/01_init.sql`)
- [ ] **V2-3: Fix partial batch delivery in ingest handler** — Pre-check `kafka_tx.capacity()` vs `batch.workloads.len()` before sending any rows; prevent split-brain duplicates (`finops-gateway/src/routes/ingest.rs`)
- [ ] **V2-4: K8s Watch/Informer instead of List polling** — 5000 agents × 30s list = 167 req/s to API server; replace with `kube::runtime::watcher` (`finops-agent/src/attribution/mod.rs`)
- [ ] **V2-5: DaemonSet `preStop` hook + `terminationGracePeriodSeconds`** — Agent needs time to flush on eviction (`deploy/k8s/agent-daemonset.yaml`)
- [ ] **V2-6: Gateway `PodDisruptionBudget`** — Prevent both replicas from simultaneous eviction (`deploy/k8s/gateway.yaml`)
- [ ] **V2-7: Pin images to registry digests** — Replace `:latest` with `@sha256:...` (`deploy/k8s/*.yaml`)
- [ ] **V2-8: Cross-AZ placement constraints** — Moved from Phase 10; add `topologySpreadConstraints` to gateway deployment

### P1-WEEK — Hot-Path & Scale Fixes

- [ ] **V2-9: BPF ring buffer wakeup suppression** — Use `BPF_RB_NO_WAKEUP` for most events; adaptive threshold in kernel (`finops-ebpf/src/main.rs`)
- [ ] **V2-10: Deduplicate procfs reads in `on_identity_event`** — Skip `/proc/{pid}/cgroup` if `cgroup_id` already cached; eliminates 200k blocking syscalls/sec (`finops-agent/src/attribution/mod.rs`)
- [ ] **V2-11: Kafka produce retry buffer** — Buffer failed produce records in `VecDeque` with bounded cap; drain on next successful produce (`finops-gateway/src/kafka.rs`)
- [ ] **V2-12: Stable partition hash** — Replace `DefaultHasher` with `FxHasher` for cross-version determinism (`finops-gateway/src/kafka.rs`)
- [ ] **V2-13: Hoist node key allocation in `bytes_to_record`** — One `to_vec()` per partition group, not per record (`finops-gateway/src/kafka.rs`)
- [ ] **V2-14: Fix `merge_cgroup_labels_from_k8s` lock duration** — Clone `pod_by_uid` under read lock, compute labels outside lock, batch-insert under short write lock (`finops-agent/src/attribution/mod.rs`)

### P2-SPRINT — Thundering Herd & Observability

- [ ] **V2-15: Agent-side jittered backoff recovery** — Add `rand(0, window_secs)` delay between retry flushes after outage recovery (`finops-agent/src/output.rs`)
- [ ] **V2-16: ClickHouse merge pressure monitoring** — `system.merges`, `system.parts` per partition, background merge queue depth
- [ ] **V2-17: Kafka produce error rate metric** — `finops_api_kafka_produce_errors_total` counter in `produce_grouped_batch` (`finops-gateway/src/kafka.rs`)
- [ ] **V2-18: End-to-end latency histogram** — Agent→Gateway→Kafka→ClickHouse pipeline latency via `batch_id` correlation

---

## Phase 6 — Mechanical sympathy ✅ (micro-opts remain)

### Hot-path lock contention ✅

- [x] **`enqueue_batch_json` queue-full path:** sync `try_lock` drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **`labels_for_cgroup` lock consolidation** ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **`AttributionCache`: `FxHashMap`** ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md))

### Hot-path allocation reduction

- [x] **`WorkloadLabels` → `Arc<WorkloadLabels>`**
- [x] **Precompute bearer:** `expected_bearer` at startup ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] ~~**Split `WorkloadStats` hot/cold**~~ (CANCELLED — struct is 32 bytes, fits in half a cache line; splitting adds pointer-chasing overhead)
- [x] ~~**Dead `mono_to_wall` in `on_finops_event`**~~ (CANCELLED — `saturating_add` is a single instruction; the trace log is free when compiled out)
- [x] ~~**Match guard → const pattern in aggregator**~~ (CANCELLED — compiler optimizes identically)

### Gateway hot path

- [x] **Kafka queue `Vec<u8>`** ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Refactor ingest handler to `Arc<[u8]>` node key** — [ADR 034](../../../docs/adr/034-phase55-l8-p2-ingest-zero-copy.md)

### Memory sampler ✅

- [x] **`Arc<PathBuf>` in `memory_current_paths`**

---

## Phase 7 — Architecture & developer experience ✅

- [x] **`finops-wire` crate** — `IngestBatch`, `WorkloadRow`, `FlatRow` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md))
- [x] **Centralized `Config` struct** — `finops-gateway/src/config.rs` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md))
- [x] **Rename agent crate:** `finops-user` → `finops-agent` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md))
- [x] **Rename gateway crate:** `finops-api` → `finops-gateway` ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **Remove deprecated `ProcessEvent`:** Dead code in `finops-common` ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **`thiserror` for gateway errors** — `GatewayError` in `finops-gateway/src/error.rs` ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md))
- [x] **Typed errors in `attribution.rs`** — `AttributionError`; `read_memory_current_at` in attribution module ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md))
- [x] **Extract `finops-infra` crate:** `read_env_u64`/`read_env_usize`, clock utilities ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **Simplify `labels_for_cgroup` read path:** Read-only lookup; K8s merge in `refresh_k8s_pods` ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md))

---

## Phase 8 — Kubernetes & deployment (base shipped)

- [x] **Production gateway image:** `deploy/docker/Dockerfile.gateway` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- [x] **Production agent image:** `deploy/docker/Dockerfile.agent` ([ADR 024](../../../docs/adr/024-agent-production-container.md))
- [x] **K8s manifests:** `deploy/k8s/gateway.yaml`, `agent-daemonset.yaml` ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md))
- [ ] **Pin images to registry digests** (→ moved to V2-7)
- [ ] **Reuse `kube::Client` across K8s refresh polls** (shipped in Phase 5.5 P1)
- [ ] **K8s informer** (→ moved to V2-4, priority elevated)
- [ ] **Stronger cgroup → pod mapping**
- [ ] **Graceful rolling update drain** (→ moved to V2-1 + V2-5)

---

## Phase 9 — Correctness & portability (EXISTENTIAL RISK)

> The eBPF verifier compatibility gap is the single biggest existential threat to this architecture.
> A customer on kernel 5.10 silently failing to load the BPF program = total data loss + churn.

- [x] **eBPF verifier regression CI** — GitHub Actions matrix 5.10 / 5.15 / 6.1 / 6.8 via virtme-ng + `finops-ebpf-verify` ([ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md), `.github/workflows/ebpf-ci.yml`)
- [ ] **arm64 eBPF CI** — Required for Graviton (AWS) and Apple Silicon dev environments
- [ ] **cgroup v1-only host detection** — Graceful error + clear log instead of silent failure

---

## Phase 10 — Observability & cost

- [x] **Grafana in Compose:** `:3001` + `grafana-clickhouse-datasource` ([ADR 031](../../../docs/adr/031-grafana-clickhouse-compose.md))
- [x] **Agent `/metrics` baseline:** `:9091` + ring drops ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] **Extended agent metrics:** flush duration, retry depth, cache size, drain budget hits (→ V2-17/V2-18)
- [ ] **Cross-AZ data transfer audit** (→ moved to V2-8, P0)
- [ ] **ClickHouse merge pressure monitoring** (→ moved to V2-16)
- [ ] **ClickHouse skip index / granularity tuning:** Add `INDEX cgroup_idx cgroup_id TYPE minmax GRANULARITY 4` for cgroup-filtered queries
- [ ] **ClickHouse Kafka engine lag monitoring:** Alert on `system.kafka_consumers` lag exceeding threshold

---

## Execution Summary (L8 V2 recommended order)

```
WEEK 1 (P0-BLOCKS-GA):  V2-1 (SIGTERM), V2-2 (CH version col), V2-3 (atomic ingest),
                         V2-5 (preStop), V2-6 (PDB), V2-7 (pin images), TLS
WEEK 2 (P1-WEEK):       V2-4 (K8s watch), V2-9 (BPF wakeup), V2-10 (procfs dedup),
                         V2-11 (produce retry), V2-12 (stable hash), V2-13 (key alloc),
                         V2-14 (lock duration)
WEEK 3-4 (P2-SPRINT):   V2-8 (cross-AZ), V2-15 (jittered recovery), V2-16 (CH merge mon),
                         V2-17 (produce metrics), V2-18 (e2e latency)
MONTH 2 (P3):            arm64 CI, cgroup v1 detection, CH skip index, Kafka lag alerting
```

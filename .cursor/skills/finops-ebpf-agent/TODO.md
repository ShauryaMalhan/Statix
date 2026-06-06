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

## Phase 5 — Production-critical blockers (prod ops tuning remains)

> P0 regressions shipped ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)). TLS shipped at ALB ([ADR 043](../../../docs/adr/043-kubernetes-alb-tls-termination.md)).

### P0 — Regressions & critical fixes ✅

- [x] **Fix `on_identity_event` write lock across procfs I/O** ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Agent Prometheus exporter:** `:9091/metrics` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Cache labels in `labels_for_cgroup`:** `DEFAULT_LABELS` + K8s/path write-back ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))

### P0 — Data integrity & security

- [x] **Bearer auth:** `expected_bearer` + agent `FINOPS_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **TLS on `POST /ingest`:** AWS ALB Ingress HTTPS :443 → `finops-gateway-svc:3000` ([ADR 043](../../../docs/adr/043-kubernetes-alb-tls-termination.md))
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

- [x] **V2-1: Agent SIGTERM handler** — SIGTERM + SIGINT flush partial window in main `select!` (`finops-agent/src/main.rs`)
- [x] **V2-2: `ReplacingMergeTree(window_end_ns)` version column** — Deterministic merge winner on retry (`deploy/clickhouse/01_init.sql`)
- [x] **V2-3: Fix partial batch delivery in ingest handler** — Pre-check `kafka_tx.capacity()` vs `batch.workloads.len()`; atomic batch accept/reject (`finops-gateway/src/routes/ingest.rs`)
- [x] **V2-4: K8s Watch/Informer instead of List polling** — `watch_k8s_pods` via `kube::runtime::watcher` + node field selector ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **V2-5: DaemonSet `preStop` hook + `terminationGracePeriodSeconds`** — `sleep 5` preStop + 30s grace for eviction flush ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-6: Gateway `PodDisruptionBudget`** — `minAvailable: 1`; gateway preStop + grace ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-7: Pin images to registry digests** — `@sha256:<64-hex>` in gateway + agent manifests ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **V2-8: Cross-AZ placement constraints** — `topologySpreadConstraints` on `topology.kubernetes.io/zone` ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))

### P1-WEEK — Hot-Path & Scale Fixes

- [x] **V2-9: BPF ring buffer wakeup suppression** — `WAKEUP_COUNTER` + `BPF_RB_NO_WAKEUP` every 63/64 events; 1ms poll drain fallback (`finops-ebpf/src/main.rs`, `finops-agent/src/main.rs`)
- [x] **V2-10: Deduplicate procfs reads in `on_identity_event`** — Read-lock fast path + double-check before procfs ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-11: Kafka produce retry buffer** — `failed_batches` `VecDeque` cap 100; drain before produce + metadata tick ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-12: Stable partition hash** — `FxHasher` in `hash_node_to_slot` ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-13: Hoist node key allocation** — One `node.to_vec()` per partition chunk; `bytes_to_record` removed ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-14: Fix `merge_cgroup_labels_from_k8s` lock duration** — Snapshot under read lock, compute outside, batch insert ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))

### P2-SPRINT — Thundering Herd & Observability

- [x] **V2-15: Agent-side jittered backoff recovery** — 0–5s jitter after recovery when `backoff_secs > initial_backoff` ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [x] **V2-16: ClickHouse merge pressure monitoring** — `deploy/grafana/clickhouse_monitoring.sql` parts + merges queries ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [x] **V2-17: Kafka produce error rate metric** — `finops_api_kafka_produce_errors_total` + `finops_api_kafka_produce_dropped_total` (shipped with V2-11, [ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-18: End-to-end latency histogram** — `finops_api_ingest_lag_seconds` from `window_end_ns` ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))

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
- [x] **Simplify `labels_for_cgroup` read path:** Read-only lookup; K8s merge in `watch_k8s_pods` ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md), [041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))

---

## Phase 8 — Kubernetes & deployment (base shipped)

- [x] **Production gateway image:** `deploy/docker/Dockerfile.gateway` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- [x] **Production agent image:** `deploy/docker/Dockerfile.agent` ([ADR 024](../../../docs/adr/024-agent-production-container.md))
- [x] **K8s manifests:** `deploy/k8s/gateway.yaml`, `agent-daemonset.yaml` ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md))
- [x] **Pin images to registry digests** — V2-7 ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **Reuse `kube::Client` across K8s refresh polls** (shipped in Phase 5.5 P1)
- [x] **K8s informer** — V2-4 `watch_k8s_pods` ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [ ] **Stronger cgroup → pod mapping**
- [x] **Graceful rolling update drain** — V2-1 SIGTERM flush + V2-5 preStop ([ADR 038](../../../docs/adr/038-phase55-v2-wave1-l8-fixes.md), [040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))

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
- [ ] **Extended agent metrics:** flush duration, retry depth, cache size, drain budget hits (V2-18 gateway lag shipped — [ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [x] **Cross-AZ data transfer audit** — V2-8 topology spread ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **ClickHouse merge pressure monitoring** — V2-16 ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [ ] **ClickHouse skip index / granularity tuning:** Add `INDEX cgroup_idx cgroup_id TYPE minmax GRANULARITY 4` for cgroup-filtered queries
- [ ] **ClickHouse Kafka engine lag monitoring:** Alert on `system.kafka_consumers` lag exceeding threshold

---

## Execution Summary (L8 V2 recommended order)

```
L8 V2 (GA):             V2-1…18 shipped ([ADR 038](../../../docs/adr/038-phase55-v2-wave1-l8-fixes.md)–[042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md)); TLS at ALB ([ADR 043](../../../docs/adr/043-kubernetes-alb-tls-termination.md))
MONTH 2 (P3):            arm64 CI, cgroup v1 detection, CH skip index, Kafka lag alerting
```

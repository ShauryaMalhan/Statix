# Statix eBPF Platform — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Current focus:** **Phase 5.5 V3** — L8/L9 Post-GA audit. **Strategic pivot (planned):** **Phase 13** — queue-less ingest (remove Kafka; agent WAL + gateway → ClickHouse HTTP).

**Completed:** Phases 1–4, **5.5 V1** (L8 P0/P1/P2), **5.5 V2** (L8 V2 distributed hardening), **6**, **7**, **9** (eBPF CI). **Targets 1–3** (packaging, CH init, API read-path).

**Validate:** [phase3-validation.md](../../../docs/phase3-validation.md). After gateway route changes: `docker compose build statix-gateway && docker compose up -d statix-gateway`. After CH schema change: `docker compose down -v && make compose-up`. Billing table: `statix.workload_metrics FINAL`.

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
- [x] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` + `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries use `FINAL` on `statix.workload_metrics` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md))
- [x] **Prometheus metrics (3.5):** `GET /metrics`; ingest counter/histogram; channel full + depth gauge; Kafka produce histogram ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md))

### P2 — Scale & audit correctness

- [x] **Ring buffer size (1.2):** `build.rs` + three ELFs (`target/bpf/`); CPU-tier auto-load in `ebpf_select.rs` ([ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md))
- [x] **Clock domain offset (4.1):** BPF `timestamp` + offset; window bounds via same domain ([ADR 016](../../../docs/adr/016-clock-domain-offset.md))
- [x] **NTP drift recalibration:** `AtomicU64` + hourly background task; hot-path `Relaxed` load ([ADR 047](../../../docs/adr/047-atomic-clock-offset-recalibration.md))
- [x] **Data lineage (4.6):** `batch_id` (UUID v4 per flush) + `agent_version` on wire and ClickHouse ([ADR 017](../../../docs/adr/017-batch-lineage-metadata.md))

### P3 — Coverage & horizontal API

- [x] **Bootstrap running workloads (1.7):** `bootstrap_existing_cgroups` walks cgroup v2; inode = `cgroup_id`; synthetic identity events ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md))

### Ingest hardening (shipped with Phase 4)

- [x] **Kafka producer env tuning:** `STATIX_KAFKA_CHANNEL_SIZE` / `BATCH_MAX` / `LINGER_MS` in `kafka.rs` ([ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md))
- [x] **Zero-copy node key in `ingest.rs`:** `KafkaQueueItem` key once per batch ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))

---

## Phase 5 — Production-critical blockers (prod ops tuning remains)

> P0 regressions shipped ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)). TLS shipped at ALB ([ADR 043](../../../docs/adr/043-kubernetes-alb-tls-termination.md)).

### P0 — Regressions & critical fixes ✅

- [x] **Fix `on_identity_event` write lock across procfs I/O** ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Agent Prometheus exporter:** `:9091/metrics` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **Cache labels in `labels_for_cgroup`:** `DEFAULT_LABELS` + K8s/path write-back ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))

### P0 — Data integrity & security

- [x] **Bearer auth:** `expected_bearer` + agent `STATIX_API_TOKEN` ([ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [x] **TLS on `POST /ingest`:** AWS ALB Ingress HTTPS :443 → `statix-gateway-svc:3000` ([ADR 043](../../../docs/adr/043-kubernetes-alb-tls-termination.md))
- [x] **BPF ring buffer overflow counter:** `RING_DROPS` + `statix_ring_drops_total` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [x] **Schema evolution:** `schema_version` 2..=3 ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md))

### P1 — Operational readiness

- [x] **API `/ready` probe** ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md))
- [x] **API `/ready` channel depth gate:** Fail readiness when ingest mpsc > 80% full ([ADR 029](../../../docs/adr/029-ready-channel-depth-gate.md))
- [ ] **Production `kafka_num_consumers`:** Match topic partitions on `statix.kafka_telemetry_queue` ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md), [ADR 026](../../../docs/adr/026-clickhouse-finops-database-init.md))
- [ ] **Kafka retention policy:** `retention.ms` / `retention.bytes` on `statix-telemetry`
- [ ] **ClickHouse broken-message alerting:** `kafka_skip_broken_messages` shipped in SQL; monitor `system.kafka_consumers` when skipped > 0

---

## Phase 5.5 — L8 Audit V1 fixes ✅

> Playbook V1: [L8-AUDIT-FIXES.md](L8-AUDIT-FIXES.md). All fixes shipped.

### P0-SHIP — Shipped ✅

- [x] **Agent hot-path P0 fixes** — [ADR 032](../../../docs/adr/032-phase55-l8-p0-hot-path-fixes.md) (OnceLock env, thread-local RNG, static `agent_version`, `DEFAULT_LABELS`, move `BatchPayload`, batched `spawn_blocking`, ring drain budget)

### P1-WEEK — Shipped ✅

- [x] **Gateway + agent P1 fixes** — [ADR 033](../../../docs/adr/033-phase55-l8-p1-week-gateway-fixes.md) (`Bytes` retry body, reuse `by_partition` + batch `Utc::now`, cached `kube::Client`, Kafka metadata refresh, `argMax` summary query)

### P2-SPRINT — Shipped ✅

- [x] **Ingest zero-copy hot path** — [ADR 034](../../../docs/adr/034-phase55-l8-p2-ingest-zero-copy.md) (`Arc<[u8]>` node key, `FlatRowRef` serialization)

---

## Phase 5.5 V2 — L8 Audit V2 fixes ✅

> Playbook V2: [L8_AUDIT_V2_FIXES.md](L8_AUDIT_V2_FIXES.md). All V2 items shipped for GA.

### P0-BLOCKS-GA — Data Integrity & Availability ✅

- [x] **V2-1: Agent SIGTERM handler** — SIGTERM + SIGINT flush partial window in main `select!` (`statix/src/main.rs`)
- [x] **V2-2: `ReplacingMergeTree(window_end_ns)` version column** — Deterministic merge winner on retry (`deploy/clickhouse/01_init.sql`)
- [x] **V2-3: Fix partial batch delivery in ingest handler** — Pre-check `kafka_tx.capacity()` vs `batch.workloads.len()`; atomic batch accept/reject (`statix-gateway/src/routes/ingest.rs`)
- [x] **V2-4: K8s Watch/Informer instead of List polling** — `watch_k8s_pods` via `kube::runtime::watcher` + node field selector ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **V2-5: DaemonSet `preStop` hook + `terminationGracePeriodSeconds`** — `sleep 5` preStop + 30s grace for eviction flush ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-6: Gateway `PodDisruptionBudget`** — `minAvailable: 1`; gateway preStop + grace ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-7: Pin images to registry digests** — `@sha256:<64-hex>` in gateway + agent manifests ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **V2-8: Cross-AZ placement constraints** — `topologySpreadConstraints` on `topology.kubernetes.io/zone` ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))

### P1-WEEK — Hot-Path & Scale Fixes ✅

- [x] **V2-9: BPF ring buffer wakeup suppression** — `WAKEUP_COUNTER` + `BPF_RB_NO_WAKEUP` every 63/64 events; 1ms poll drain fallback (`statix-ebpf/src/main.rs`, `statix/src/main.rs`)
- [x] **V2-10: Deduplicate procfs reads in `on_identity_event`** — Read-lock fast path + double-check before procfs ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-11: Kafka produce retry buffer** — `failed_batches` `VecDeque` cap 100; drain before produce + metadata tick ([ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-12: Stable partition hash** — `FxHasher` in `hash_node_to_slot` ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-13: Hoist node key allocation** — One `node.to_vec()` per partition chunk; `bytes_to_record` removed ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))
- [x] **V2-14: Fix `merge_cgroup_labels_from_k8s` lock duration** — Snapshot under read lock, compute outside, batch insert ([ADR 039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md))

### P2-SPRINT — Thundering Herd & Observability ✅

- [x] **V2-15: Agent-side jittered backoff recovery** — 0–5s jitter after recovery when `backoff_secs > initial_backoff` ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [x] **V2-16: ClickHouse merge pressure monitoring** — `deploy/grafana/clickhouse_monitoring.sql` parts + merges queries ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))
- [x] **V2-17: Kafka produce error rate metric** — `statix_api_kafka_produce_errors_total` + `statix_api_kafka_produce_dropped_total` (shipped with V2-11, [ADR 040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))
- [x] **V2-18: End-to-end latency histogram** — `statix_api_ingest_lag_seconds` from `window_end_ns` ([ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md))

---

## Phase 5.5 V3 — L8/L9 Post-GA Audit (ACTIVE)

> Playbook V3: [L8_POST_GA_FIXES.md](L8_POST_GA_FIXES.md). Silent killers found at 10,000-node scale after 6-month continuous operation analysis.

### P0-CRITICAL — Silent Deaths & Data Integrity

- [x] **V3-7: K8s watcher `tokio::spawn` silently swallows panics** — `JoinHandle` in `select!`; panic metric + restart (`statix/src/main.rs`) ([ADR 049](../../../docs/adr/049-phase55-v3-wave1-silent-deaths.md))
- [x] **V3-8: Ring drops monitor `tokio::spawn` also silently swallows panics** — Return `JoinHandle` from `spawn_ring_drops_monitor`; monitor in `select!`; `statix_ring_monitor_panics_total` (`statix/src/loader.rs`, `main.rs`) ([ADR 049](../../../docs/adr/049-phase55-v3-wave1-silent-deaths.md))
- [x] **V3-13: Ingest handler capacity pre-check is TOCTOU** — Pre-serialize rows + `try_reserve_many`; atomic batch accept/reject (`statix-gateway/src/routes/ingest.rs`) ([ADR 049](../../../docs/adr/049-phase55-v3-wave1-silent-deaths.md))

### P0-WEEK — Resource Exhaustion Time Bombs

- [x] **V3-4: `AttributionCache` unbounded growth** — 60s `evict_stale_cgroups()` sweep; cascade delete; `statix_cache_evictions_total` (`statix/src/attribution/mod.rs`, `main.rs`) ([ADR 050](../../../docs/adr/050-phase55-v3-wave2-cache-eviction.md))
- [x] **V3-5: `pod_by_uid` never evicts deleted pods** — `Event::Delete` → `remove_pod_by_uid` in `watch_k8s_pods` ([ADR 050](../../../docs/adr/050-phase55-v3-wave2-cache-eviction.md))
- [x] **V3-9: K8s watcher reconnect has no backoff** — Jittered exponential backoff 5s→300s; reset on successful stream event ([ADR 050](../../../docs/adr/050-phase55-v3-wave2-cache-eviction.md))

### P1-SPRINT — Distributed State Physics

- [x] **V3-11: ClickHouse midnight partition boundary storm** — Hour-aligned `toStartOfHour` partition expression (`deploy/clickhouse/01_init.sql:31`) ([ADR 051](../../../docs/adr/051-phase55-v3-wave3-distributed-state.md))
- [x] **V3-12: `kafka_num_consumers = 1` bottleneck at scale** — `kafka_num_consumers = 4` on `kafka_telemetry_queue` (`deploy/clickhouse/01_init.sql:59`) ([ADR 051](../../../docs/adr/051-phase55-v3-wave3-distributed-state.md))
- [x] **V3-15: Agent recovery thundering herd** — Deterministic node-hash recovery spread 0–30s + PRNG (`statix/src/output.rs`) ([ADR 051](../../../docs/adr/051-phase55-v3-wave3-distributed-state.md))

### P1-WEEK — Performance & Observability

- [ ] **V3-2: `bootstrap_existing_cgroups` blocks async runtime** — Wrap `WalkDir` + `fs::metadata` in `spawn_blocking` (`statix/src/attribution/mod.rs:116-171`)
- [ ] **V3-6: `RING_DROPS` counter uses `absolute()`** — Track previous reading; emit increments instead of absolute value (`statix/src/loader.rs:67`)
- [ ] **V3-10: `spawn_blocking` JoinError silently returns empty Vec** — Log error + emit `statix_memory_sampler_errors_total` instead of `unwrap_or_default` (`statix/src/memory_sampler.rs:36-37`)
- [ ] **V3-14: No explicit body size limit on POST /ingest** — Add `DefaultBodyLimit::max(2MB)` to Axum router (`statix-gateway/src/routes/ingest.rs`)
- [ ] **V3-1: Agent DaemonSet missing resource requests/limits** — Add `requests: {cpu: 50m, memory: 64Mi}` + `limits: {cpu: 500m, memory: 256Mi}` (`deploy/k8s/statix-daemonset.yaml`)

### P2-MONTH — Micro-architecture Polish

- [ ] **V3-16: Magic number for `BPF_RB_NO_WAKEUP`** — Named constant `const BPF_RB_NO_WAKEUP: u64 = 1` (`statix-ebpf/src/main.rs:77`)
- [ ] **V3-17: No alignment assertion for `StatixEvent` pointer cast** — Compile-time `const _: () = assert!(align_of::<StatixEvent>() <= 8)` (`statix/src/main.rs:109`)
- [ ] **V3-18: 1ms poll interval unnecessarily aggressive** — Increase to 5ms (`statix/src/main.rs:92`)
- [ ] **V3-3: `node.to_string()` allocation on every flush** — Change `BatchPayload.node` to `Arc<str>` (`statix/src/aggregator.rs:216`)

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
- [x] ~~**Dead `mono_to_wall` in `on_statix_event`**~~ (CANCELLED — `saturating_add` is a single instruction; the trace log is free when compiled out)
- [x] ~~**Match guard → const pattern in aggregator**~~ (CANCELLED — compiler optimizes identically)

### Gateway hot path

- [x] **Kafka queue `Vec<u8>`** ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Refactor ingest handler to `Arc<[u8]>` node key** — [ADR 034](../../../docs/adr/034-phase55-l8-p2-ingest-zero-copy.md)

### Memory sampler ✅

- [x] **`Arc<PathBuf>` in `memory_current_paths`**

---

## Phase 7 — Architecture & developer experience ✅

- [x] **`statix-wire` crate** — `IngestBatch`, `WorkloadRow`, `FlatRow` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md))
- [x] **Centralized `Config` struct** — `statix-gateway/src/config.rs` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md))
- [x] **Rename agent crate:** `finops-user` → `finops-agent` → `statix` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md), [044](../../../docs/adr/044-statix-agent-rename.md))
- [x] **Rename gateway crate:** `finops-api` → `statix-gateway` ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **Remove deprecated `ProcessEvent`:** Dead code in `statix-common` ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **`thiserror` for gateway errors** — `GatewayError` in `statix-gateway/src/error.rs` ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md))
- [x] **Typed errors in `attribution.rs`** — `AttributionError`; `read_memory_current_at` in attribution module ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md))
- [x] **Extract `statix-infra` crate:** `read_env_u64`/`read_env_usize`, clock utilities ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md))
- [x] **Generic positive-bounded env parsing:** `read_env_positive<T>` — reject `<= T::default()`; agent window/sample via shared helper ([ADR 048](../../../docs/adr/048-generic-env-positive-parsing.md))
- [x] **Simplify `labels_for_cgroup` read path:** Read-only lookup; K8s merge in `watch_k8s_pods` ([ADR 036](../../../docs/adr/036-phase7-typed-errors-labels-read-path.md), [041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))

---

## Phase 8 — Kubernetes & deployment (base shipped)

- [x] **Statix company rename** — crate/dir `statix`, `Dockerfile.statix`, `statix-daemonset.yaml`, skill `statix-ebpf-agent` ([ADR 044](../../../docs/adr/044-statix-agent-rename.md))
- [x] **Statix platform rename** — `statix-common/wire/infra/gateway/ebpf`, K8s `statix-system`, `STATIX_*` env, CH `statix` DB ([ADR 045](../../../docs/adr/045-statix-platform-rename.md))
- [x] **Production gateway image:** `deploy/docker/Dockerfile.gateway` ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md))
- [x] **Production agent image:** `deploy/docker/Dockerfile.statix` ([ADR 024](../../../docs/adr/024-agent-production-container.md))
- [x] **K8s manifests:** `deploy/k8s/gateway.yaml`, `statix-daemonset.yaml` ([ADR 025](../../../docs/adr/025-kubernetes-gateway-and-agent.md))
- [x] **Pin images to registry digests** — V2-7 ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [x] **Reuse `kube::Client` across K8s refresh polls** (shipped in Phase 5.5 P1)
- [x] **K8s informer** — V2-4 `watch_k8s_pods` ([ADR 041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md))
- [ ] **Stronger cgroup → pod mapping**
- [x] **Graceful rolling update drain** — V2-1 SIGTERM flush + V2-5 preStop ([ADR 038](../../../docs/adr/038-phase55-v2-wave1-l8-fixes.md), [040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md))

---

## Phase 9 — Correctness & portability (EXISTENTIAL RISK)

> The eBPF verifier compatibility gap is the single biggest existential threat to this architecture.
> A customer on kernel 5.10 silently failing to load the BPF program = total data loss + churn.

- [x] **eBPF verifier regression CI** — GitHub Actions matrix 5.10 / 5.15 / 6.1 / 6.8 via virtme-ng + `statix-ebpf-verify` ([ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md), `.github/workflows/ebpf-ci.yml`)
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

## Phase 11 — Agent Network Hardening & Reliability

> Scope: `statix/src/output.rs` — HTTP ingest retry path. (Phase 7 = workspace/DX, already complete.)

- [x] **Exponential backoff with jitter (shipped)** — Phase 4 item 3.2, V2-15, [ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md), [ADR 042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md). `backoff * 2` capped at `STATIX_BACKOFF_MAX_SECS` (default 30s); 30% jitter on retry sleep; 0–5s PRNG recovery jitter in `statix/src/output.rs:112-131`.

- [x] **Implement deterministic node-hash recovery spread (V3-15)** — On gateway recovery (first `Success` after elevated backoff), sleep `hash(STATIX_NODE_NAME) % 30s` + 0–5s PRNG via `recovery_spread_sleep_secs` in `statix/src/output.rs` ([ADR 051](../../../docs/adr/051-phase55-v3-wave3-distributed-state.md)).

- [ ] **Local disk buffering (write-ahead log)** — **Primary resilience mechanism** for the queue-less architecture (Phase 13): replaces Kafka as the shock absorber when gateway or ClickHouse is unavailable. Unacknowledged batches today live in a bounded in-memory `mpsc(60)` only; queue-full path drop-oldest ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)); OOM kill or node reboot destroys queued financial telemetry. Route failed batches (503/timeout/transport) to a persistent local store (mmap segment or SQLite, e.g. `/var/lib/statix/buffer.db`); background sweeper drains WAL when gateway `/ready` returns 200. Gateway **503 backpressure** (Phase 13) intentionally trips agent circuit breakers → WAL. Depends on DaemonSet volume mount + durable volume sizing in K8s.

- [ ] **Circuit breaker on HTTP ingest client** — No open/half-open state today; retry worker keeps attempting `post_ingest` TCP handshakes on every backoff tick while gateway is hard-down. Wrap `reqwest` calls in a failure-count state machine: after N consecutive failures → **Open** (short-circuit POST, enqueue to WAL immediately); **Half-Open** probe every 30s; close on success. Complements WAL task above; recovery spread (V3-15) applies on **Close**.

---

## Phase 12 — Gateway Performance & Zero-Copy Hardening

- [x] **Eliminate hot-path heap allocations in `Config` bearer auth** — `Config::expected_bearer()` previously returned `Option<String>` via per-call `format!("Bearer {t}")`; ingest hot path was already safe via `AppState`, but the API invited accidental re-allocation. Precompute `pub expected_bearer: Option<String>` once in `Config::from_env()`; accessor returns `Option<&str>` via `as_deref()`. `AppState` clones at startup only (`statix-gateway/src/config.rs`, `main.rs`).

---

## Phase 13 — Queue-less Architecture (Kafka Removal)

> **Strategic pivot:** Eliminate Apache Kafka from the stack. Agent local disk WAL is the edge shock absorber; `statix-gateway` writes telemetry directly to ClickHouse via HTTP. Supersedes Phases 3–4 Kafka ingest path ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md), [ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md)) once shipped. *(Phase 9 in this file remains eBPF CI — queue-less tracked here as Phase 13.)*

- [ ] **Strip Kafka producer from gateway** — Remove `rskafka`, `statix-gateway/src/kafka.rs`, ingest `mpsc` → Kafka channels, and Kafka from `docker-compose.yml` / K8s manifests. Refactor `POST /ingest` (`statix-gateway/src/routes/ingest.rs`) to denormalize batches and **async insert directly into ClickHouse** via the native HTTP client (`clickhouse` crate / JSONEachRow). Retain `ReplacingMergeTree` dedupe on `(node, window_start_ns, cgroup_id)` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)).

- [ ] **Implement gateway 503 backpressure** — Without Kafka, the gateway cannot buffer when ClickHouse is down. If ClickHouse insert fails or times out, return **`503 Service Unavailable`** immediately on `POST /ingest` (no partial accept). Agents' circuit breakers (Phase 11) open and batches go to **local disk WAL**. `/ready` should reflect ClickHouse reachability, not Kafka mpsc depth ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md), [ADR 029](../../../docs/adr/029-ready-channel-depth-gate.md) — refactor probes).

- [ ] **Remove ClickHouse Kafka engine tables** — Drop `statix.kafka_telemetry_queue`, `telemetry_mv`, and `kafka_num_consumers` tuning from `deploy/clickhouse/01_init.sql`; direct insert to `statix.workload_metrics` only. Cancels open Phase 5 Kafka ops items (retention, `kafka_num_consumers`, broken-message alerting).

---

## Execution Summary

```
L8 V1 (shipped):        P0/P1/P2 hot-path fixes (ADR 032–034)
L8 V2 (shipped, GA):    V2-1…18 distributed hardening (ADR 038–043)
L8/L9 V3 (ACTIVE):      V3-4…18 (Wave 1–3 shipped: ADR 049–051)
  Week 1:               [x] V3-7, V3-8, V3-13 (silent death + data integrity)
  Week 2:               [x] V3-4, V3-5, V3-9    (memory leaks + API DDoS)
  Week 3:               [x] V3-11, V3-12, V3-15 (distributed state)
  Week 4:               V3-2, V3-6, V3-10, V3-14, V3-1 (perf + observability)
  Month 2:              V3-16…18, V3-3      (micro-architecture polish)
MONTH 3 (P3):           arm64 CI, cgroup v1 detection, CH skip index, Kafka lag alerting
PHASE 11 (planned):     agent WAL (primary buffer), circuit breaker
PHASE 13 (pivot):       Remove Kafka; gateway → ClickHouse HTTP; 503 → agent WAL
```

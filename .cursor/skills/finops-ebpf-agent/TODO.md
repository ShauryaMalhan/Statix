# FinOps Agent — Roadmap & completed work

Mark shipped items `[x]` (do not remove). See [docs/adr/](../../../docs/adr/) for decisions.

**Gate:** Phase 1–3 E2E (agent → API → Kafka → ClickHouse) is validated locally; use [phase3-validation.md](../../../docs/phase3-validation.md) after infra changes. Start Phase 4 only when that checklist passes on your target environment.

---

## Phase 4 — Scale & reliability (production roadmap)

### P1 — Before AWS ECS / production billing

- [x] **Kafka partition routing (1.1):** `node` as Kafka key + `DefaultHasher % partitions`; multi `PartitionClient` from broker metadata ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [x] **Agent ingest retry (3.2):** Background worker in `output.rs` — bounded queue 60, env backoff + 30% jitter on 5xx/429/transport; drop-oldest when full ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md))
- [x] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` + `ORDER BY (node, window_start_ns, cgroup_id)`; billing queries use `FINAL` ([ADR 011](../../../docs/adr/011-replacingmergetree-dedupe-identity.md)). Optional: `batch_id` on wire for audit (see Data lineage 4.6 below)
- [x] **Prometheus metrics (3.5):** `GET /metrics`; ingest counter/histogram; channel full + depth gauge; Kafka produce histogram ([ADR 012](../../../docs/adr/012-finops-api-prometheus-metrics.md))

### P2 — Scale & audit correctness

- [x] **Ring buffer size (1.2):** `build.rs` + three ELFs (`target/bpf/`); CPU-tier auto-load in `ebpf_select.rs` ([ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md))
- [ ] **Clock domain offset (4.1):** BPF timestamps use kernel boot time; agent uses wall clock — compute offset so NTP clock steps do not warp billing windows
- [ ] **Data lineage (4.6):** `agent_version` + `batch_id` on wire and in ClickHouse for financial audits

### P3 — Coverage & horizontal API

- [ ] **Bootstrap running workloads (1.7):** Scan `/sys/fs/cgroup` on startup — eBPF only sees new `sched_process_exec`; miss already-running DBs until restart
- [ ] **API `/ready` probe (1.6):** separate readiness (e.g. channel depth / Kafka lag) — `/health` shipped (`kafka_tx.is_closed()`); ALB multi-replica tuning deferred

---

## Phase 3 — ingest hardening

- [x] **Kafka producer env tuning:** `FINOPS_KAFKA_CHANNEL_SIZE` / `BATCH_MAX` / `LINGER_MS` in `kafka.rs` ([ADR 014](../../../docs/adr/014-kafka-producer-env-tuning.md))
- [ ] **Production ClickHouse:** set `kafka_num_consumers` = Kafka topic partition count in env-specific SQL ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **TLS + auth on `POST /ingest`**

---

## Performance

- [x] **Zero-copy node key in `ingest.rs`:** `KafkaQueueItem` = `(Bytes, Bytes)`; `node_bytes` once per batch, `node_bytes.clone()` per row ([ADR 010](../../../docs/adr/010-kafka-partition-key-by-node.md))
- [ ] **`labels_for_cgroup`: fewer `RwLock` read passes**
- [ ] **BPF-side memory samples** (if sysfs profiled hot)

---

## Correctness & portability

- [ ] **cgroup v1-only host detection**
- [ ] **arm64 eBPF CI**
- [ ] **`FINOPS_REDACT_COMM`**

---

## Kubernetes & deployment

- [ ] **K8s informer** (replace 30s poll)
- [ ] **Stronger cgroup → pod mapping**
- [ ] **DaemonSet + RBAC YAML**

---

## Observability (agent / BPF dev)

- [ ] **`aya-log` for dev BPF diagnostics**
- [ ] **Agent metrics: ring drops, cache size, sample failures** (complements Phase 4 P1 API `/metrics`)

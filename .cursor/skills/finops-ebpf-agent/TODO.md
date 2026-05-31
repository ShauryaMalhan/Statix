# FinOps Agent — Remaining work (open only)

Delete a line when it ships. See [docs/adr/](../../../docs/adr/) for completed decisions.

**Gate:** Phase 1–3 E2E (agent → API → Kafka → ClickHouse) is validated locally; use [phase3-validation.md](../../../docs/phase3-validation.md) after infra changes. Start Phase 4 only when that checklist passes on your target environment.

---

## Phase 4 — Scale & reliability (production roadmap)

### P1 — Before AWS ECS / production billing

- [ ] **Kafka partition routing (1.1):** Hashed routing by node name (or similar); single partition caps ClickHouse/API throughput to ~1 consumer thread — required before multi-node ECS deploy
- [ ] **Agent ingest retry (3.2):** Honor `503` from `POST /ingest` + exponential backoff on shared `reqwest` client; today transport errors and backpressure only `log::warn`
- [ ] **Dedupe / idempotency (4.4):** `ReplacingMergeTree` or `batch_id` in ClickHouse — must ship **with** retries to avoid double-billing
- [ ] **Prometheus metrics (3.5):** `finops-api` `/metrics` — mpsc channel depth, dropped rows, ingest HTTP latency, Kafka produce latency

### P2 — Scale & audit correctness

- [ ] **Ring buffer size (1.2):** Make `EVENTS` ring buffer byte size configurable via env (512KB too small on large nodes / 1TB hosts)
- [ ] **Clock domain offset (4.1):** BPF timestamps use kernel boot time; agent uses wall clock — compute offset so NTP clock steps do not warp billing windows
- [ ] **Data lineage (4.6):** `agent_version` + `batch_id` on wire and in ClickHouse for financial audits

### P3 — Coverage & horizontal API

- [ ] **Bootstrap running workloads (1.7):** Scan `/sys/fs/cgroup` on startup — eBPF only sees new `sched_process_exec`; miss already-running DBs until restart
- [ ] **API `/ready` probe (1.6):** separate readiness (e.g. channel depth / Kafka lag) — `/health` shipped (`kafka_tx.is_closed()`); ALB multi-replica tuning deferred

---

## Phase 3 — ingest hardening

- [ ] **Production ClickHouse:** set `kafka_num_consumers` = Kafka topic partition count in env-specific SQL ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **TLS + auth on `POST /ingest`**

---

## Performance

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

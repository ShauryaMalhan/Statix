# FinOps Agent — Remaining work (open only)

Delete a line when it ships. See [docs/adr/](../../../docs/adr/) for completed decisions.

---

## Phase 3 — ingest hardening

- [ ] **Production ClickHouse:** set `kafka_num_consumers` = Kafka topic partition count in env-specific SQL ([ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- [ ] **TLS + auth on `POST /ingest`**
- [ ] **Agent ingest retry / dead-letter**
- [ ] **Ingest metrics** (channel depth, drops, produce latency)
- [ ] **finops-api in Docker Compose**

---

## Performance

- [ ] **`cgroup_path_from_pid`: stack-buffer read**
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

## Observability

- [ ] **`aya-log` for dev BPF diagnostics**
- [ ] **Agent metrics: ring drops, cache size, sample failures**

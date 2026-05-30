<<<<<<< HEAD
# FinOps Agent — Remaining work (current scope)

Open items only — deferred optimizations and Phase 2 gaps.  
**Not listed here:** completed work (see git history / skills) or later phases (see [phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)).

Add a bullet when we postpone something; **delete the line** when it ships (do not keep a “done” list in this file).
=======
# FinOps Agent — Remaining work (open only)

Deferred enterprise / correctness items. **Delete a line when it ships.**  
Completed phases: git history, [docs/adr/](../../../docs/adr/), skills.
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

**Status:** `[ ]` open · `[~]` in progress

---

<<<<<<< HEAD
## Performance

- [ ] **`cgroup_path_from_pid`: stack-buffer read instead of `read_to_string`**  
  - **Where:** `finops-user/src/attribution.rs`  
  - **Why:** Exec storms allocate a heap `String` per `/proc/{pid}/cgroup` read.  
  - **How:** `File::open` + stack buffer (512–1024 B), parse in-place (same idea as `memory_sampler`).

- [ ] **`labels_for_cgroup`: fewer `RwLock` read passes**  
  - **Where:** `finops-user/src/attribution.rs`  
  - **How:** One read guard per call, or refresh merged labels on K8s update.

- [ ] **BPF-side memory samples (only if sysfs becomes a bottleneck)**  
  - **Where:** `finops-ebpf`, `finops-common`  
  - **How:** Profile first; then throttled kernel samples if needed.
=======
## Phase 3 — ingest hardening

- [ ] **TLS + auth on `POST /ingest`**  
  - **Where:** `finops-api`, agent `reqwest` client  
  - **Why:** Production DaemonSet → API must not be plaintext-only.

- [ ] **Agent ingest retry / dead-letter**  
  - **Where:** `finops-user/src/output.rs`  
  - **Why:** Today failed POST is log-only; enterprise may need bounded retry queue.

- [ ] **Ingest metrics** (channel depth, drops, produce latency)  
  - **Where:** `finops-api`  
  - **How:** Prometheus counters on `try_send` failures and Kafka errors.

- [ ] **finops-api in Docker Compose**  
  - **Why:** Single `compose up` for full stack without host-run API.

---

## Performance

- [ ] **`cgroup_path_from_pid`: stack-buffer read instead of `read_to_string`**  
  - **Where:** `finops-user/src/attribution.rs`

- [ ] **`labels_for_cgroup`: fewer `RwLock` read passes**  
  - **Where:** `finops-user/src/attribution.rs`

- [ ] **BPF-side memory samples (only if sysfs profiled hot)**  
  - **Where:** `finops-ebpf`, `finops-common`
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

---

## Correctness & portability

<<<<<<< HEAD
- [ ] **Detect / document cgroup v1-only hosts**  
  - **How:** Startup check; clear error or degrade.

- [ ] **arm64 (multi-arch) eBPF build in CI**

- [ ] **`FINOPS_REDACT_COMM` — optional `comm` redaction in JSON output**
=======
- [ ] **Detect / document cgroup v1-only hosts**

- [ ] **arm64 eBPF build in CI**

- [ ] **`FINOPS_REDACT_COMM` — optional `comm` redaction**
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

---

## Kubernetes & deployment

<<<<<<< HEAD
- [ ] **K8s informer instead of 30s pod list poll**  
  - **Where:** `attribution::refresh_k8s_pods`

- [ ] **Stronger cgroup → pod mapping (beyond path heuristics)**  
  - **Why:** containerd / crio / kind naming differs.

- [ ] **DaemonSet + RBAC YAML in repo**  
  - **Ref:** [phase2-validation.md](../../../docs/phase2-validation.md)

---

## Observability & repo

- [ ] **`aya-log` / `aya-log-ebpf` for dev-only attach diagnostics**

- [ ] **Metrics: ring buffer drops, cache size, sample failures**
=======
- [ ] **K8s informer instead of 30s pod list poll**

- [ ] **Stronger cgroup → pod mapping (containerd/crio/kind)**

- [ ] **DaemonSet + RBAC YAML in repo**

---

## Observability

- [ ] **`aya-log` / `aya-log-ebpf` for dev-only BPF diagnostics**

- [ ] **Agent metrics: ring buffer drops, cache size, sample failures**
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

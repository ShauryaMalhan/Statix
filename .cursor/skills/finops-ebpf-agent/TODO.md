# FinOps Agent — Remaining work (current scope)

Open items only — deferred optimizations and Phase 2 gaps.  
**Not listed here:** completed work (see git history / skills) or later phases (see [phase3-ingest-interface.md](../../../docs/phase3-ingest-interface.md)).

Add a bullet when we postpone something; **delete the line** when it ships (do not keep a “done” list in this file).

**Status:** `[ ]` open · `[~]` in progress

---

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

---

## Correctness & portability

- [ ] **Detect / document cgroup v1-only hosts**  
  - **How:** Startup check; clear error or degrade.

- [ ] **arm64 (multi-arch) eBPF build in CI**

- [ ] **`FINOPS_REDACT_COMM` — optional `comm` redaction in JSON output**

---

## Kubernetes & deployment

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

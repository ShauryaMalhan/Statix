# ADR 036: Phase 7 typed errors and read-only `labels_for_cgroup`

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 7 code tasks ([TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md)) — `thiserror` on gateway and agent attribution; L8 FLAW 9 (write lock on label read path).

## Decision

### Gateway (`statix-gateway`)

- **`thiserror`** dependency; `src/error.rs` — `GatewayError` enum.
- **`main`:** `Result<(), GatewayError>` — Prometheus install, bind, serve, drain timeout.
- **`kafka.rs`:** `load_partition_clients`, `run_producer_loop` return `GatewayError` (`Kafka`, `NoTopicPartitions`).
- **`routes/query.rs`:** ClickHouse failures → `GatewayError::ClickHouse` → HTTP 500 via `status_code()`.

### Agent (`statix`)

- **`attribution/error.rs`** — `AttributionError` for cgroup path and `memory.current` I/O.
- **`cgroup_path_from_pid`**, **`read_memory_current_at`** (moved to attribution module, called from `memory_sampler`) use typed errors.
- **`refresh_k8s_pods`:** `Result<(), AttributionError>`; after pod upsert, **`merge_cgroup_labels_from_k8s`** writes merged labels under write lock (background only).

### L8 FLAW 9 — `labels_for_cgroup`

- **Before:** Read lock → optional write lock to merge K8s labels on cache miss (hot-path contention).
- **After:** Single read lock; lookup `cgroup_labels` only; `DEFAULT_LABELS` on miss. K8s merge runs in `refresh_k8s_pods` every 30s.

## Rationale

- Typed errors replace stringly `anyhow::bail!` at attribution and Kafka setup boundaries.
- Hot path (`on_finops_event`, memory samples) never upgrades `RwLock` to write for label merge.

## Consequences

- **Positive:** Label lookup is O(1) read under one lock; gateway startup/Kafka failures are matchable enums.
- **Negative:** K8s namespace/pod labels may lag up to one refresh interval (~30s) for new cgroups — acceptable for FinOps rollups.
- **Module layout:** `attribution.rs` → `attribution/mod.rs` + `attribution/error.rs`.

## References

- [ADR 023](023-phase5-hot-path-fixes.md) — prior `labels_for_cgroup` lock consolidation
- [ADR 035](035-phase7-workspace-restructure.md)

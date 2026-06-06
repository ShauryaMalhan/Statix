# ADR 023: Phase 5 hot-path fixes (attribution, agent metrics, ingest bearer)

**Status:** Accepted  
**Date:** 2026-06-04  
**Context:** L8 audit regressions — write lock held across procfs I/O, unbounded `Arc` allocs on label miss, ring-drop counter with no Prometheus recorder, per-request `format!` on ingest auth ([TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md) Phase 5 P0).

## Decision

### 1. Attribution (`finops-user/src/attribution.rs`)

- **`on_identity_event`:** Read `/proc/{pid}/cgroup` **before** `state.write()`; insert path/memory paths under the write lock only.
- **`labels_for_cgroup`:** `static DEFAULT_LABELS: LazyLock<Arc<WorkloadLabels>>` for unknown cgroups (no per-call `Arc::new(default())`).
- **K8s merge + path-derived labels:** On miss, build `Arc`, `drop` read guard, write-lock insert into `cgroup_labels` so subsequent lookups hit the fast path.

### 2. Agent Prometheus (`finops-user`)

- Dependency: `metrics-exporter-prometheus = "0.12"`.
- At startup (after `env_logger::init`): `PrometheusBuilder::new().with_http_listener(([0, 0, 0, 0], 9091)).install()` (warn on failure, do not abort agent).
- Scrape: `http://<node>:9091/metrics` — includes `statix_ring_drops_total` from [ADR 022](022-bpf-ring-buffer-drop-counter.md).

### 3. Gateway bearer compare (`finops-api`)

- `AppState.expected_bearer: Option<String>` — `STATIX_API_TOKEN` mapped once at startup to full header value `Bearer {token}`.
- `ingest` handler compares `Authorization` to `expected_bearer.as_str()` with no per-request allocation.

## Consequences

- **Positive:** Ring-buffer path no longer blocks readers on procfs; ingest auth and label lookup avoid hot-path heap churn; ring drops observable via Prometheus.
- **Negative:** Agent exposes a second HTTP port (9091) — document in DaemonSet/network policy.
- **Negative:** Agent exporter `0.12` vs API `0.17` — separate dependency trees; acceptable until unified in Phase 10.

## References

- [ADR 019](019-ingest-bearer-token-auth.md), [ADR 022](022-bpf-ring-buffer-drop-counter.md)
- `finops-user/src/attribution.rs`, `main.rs`, `loader.rs`
- `finops-api/src/main.rs`, `routes/ingest.rs`

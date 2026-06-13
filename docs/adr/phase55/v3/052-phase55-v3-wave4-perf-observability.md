# ADR 052: Phase 5.5 V3 Wave 4 — performance & observability

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** L8/L9 Post-GA audit Wave 4 ([L8_POST_GA_FIXES.md](../../../../.cursor/skills/statix-ebpf-agent/L8_POST_GA_FIXES.md)) — async runtime blocking, fragile metrics, silent thread failures, ingest body limits, and K8s QoS.

## Decision

### V3-2 — Non-blocking cgroup bootstrap (`statix/src/attribution/mod.rs`)

- Move `WalkDir` + `fs::metadata` into `tokio::task::spawn_blocking`.
- Register cgroups and synthesize identity events on the async thread after discovery.

### V3-6 — Ring drops counter increments (`statix/src/loader.rs`)

- Track `prev_total`; emit `increment(delta)` instead of `absolute(total_drops)` so BPF reload counter resets do not violate Prometheus monotonicity.

### V3-10 — Memory sampler JoinError visibility (`statix/src/memory_sampler.rs`)

- Replace `unwrap_or_default()` with explicit `Err` branch: log error + `statix_memory_sampler_errors_total`.

### V3-14 — Explicit ingest body limit (`statix-gateway/src/main.rs`)

- `DefaultBodyLimit::max(2 * 1024 * 1024)` on `POST /ingest`.

### V3-1 — DaemonSet resource requests/limits (`deploy/k8s/statix-daemonset.yaml`)

- `requests: {cpu: 50m, memory: 64Mi}`; `limits: {cpu: 500m, memory: 256Mi}` for Burstable QoS.

## Rationale

- Blocking cgroup walk on startup stalls the Tokio runtime during agent boot on large nodes.
- Absolute counter export breaks after BPF program reload; increments preserve scrape semantics.
- Silent `JoinError` hides memory sampler panics — operators need a metric.
- Unbounded ingest bodies risk OOM; 2MB aligns with batch sizing.
- Missing resource stanzas leave agent in BestEffort QoS — kubelet may OOM-kill under node pressure.

## Consequences

- **Positive:** Startup no longer blocks workers; correct ring-drop metrics; observable sampler failures; gateway memory bounded; predictable agent scheduling.
- **Negative:** Clients sending >2MB ingest payloads receive 413; bootstrap still serializes registration on async thread (acceptable — one-time at startup).

## References

- [ADR 051](051-phase55-v3-wave3-distributed-state.md) — Wave 3
- [ADR 015](../../015-cgroup-v2-bootstrap-on-startup.md) — cgroup bootstrap
- [ADR 022](../../022-bpf-ring-buffer-drop-counter.md) — ring drops map
- [TODO.md](../../../../.cursor/skills/statix-ebpf-agent/TODO.md) — V3-2, V3-6, V3-10, V3-14, V3-1

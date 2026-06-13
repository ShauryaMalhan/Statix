# ADR 050: Phase 5.5 V3 Wave 2 — cache eviction and K8s reconnect backoff

**Status:** Accepted  
**Date:** 2026-06-12
**Context:** L8/L9 Post-GA audit Wave 2 ([L8_POST_GA_FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8_POST_GA_FIXES.md)) — `AttributionCache` and `pod_by_uid` grew without bound; K8s watcher reconnect loop hammered the API at a fixed 5s interval during outages.

## Decision

### V3-4 — Stale cgroup eviction (`statix/src/attribution/mod.rs`, `main.rs`)

- Add `AttributionCache::evict_stale_cgroups()` — scan `memory_current_paths`; if path missing, cascade-delete from `cgroup_paths`, `memory_current_paths`, `cgroup_labels`.
- 60s timer in main `select!`; log count + `statix_cache_evictions_total` counter increment.

### V3-5 — Pod delete eviction (`watch_k8s_pods`)

- On `Event::Delete`, remove pod UID from `pod_by_uid` via `remove_pod_by_uid`.

### V3-9 — Jittered reconnect backoff (`watch_k8s_pods`)

- Outer reconnect loop: initial 5s, double to max 300s, 30% jitter on sleep.
- Reset backoff to 5s on each successful watcher event (stream connected).

## Rationale

- Terminated pod cgroups leave stale cache entries → memory leak + ENOENT spam in memory sampler.
- Deleted pods must leave `pod_by_uid` or label merges retain ghost metadata.
- 10k nodes × 5s reconnect = 2k RPS against apiserver during outages — exponential backoff spreads load.

## Consequences

- **Positive:** Cache footprint bounded by live cgroups/pods on node; apiserver-friendly reconnect.
- **Negative:** 60s eviction lag before stale entries removed — acceptable for FinOps rollups.
- **Unchanged:** Hot-path `labels_for_cgroup` remains read-only.

## References

- [ADR 049](049-phase55-v3-wave1-silent-deaths.md) — Wave 1
- [TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md) — V3-4, V3-5, V3-9

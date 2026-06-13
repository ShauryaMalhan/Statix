# ADR 049: Phase 5.5 V3 Wave 1 — silent async deaths and atomic ingest

**Status:** Accepted  
**Date:** 2026-06-12 
**Context:** L8/L9 Post-GA audit Wave 1 ([L8_POST_GA_FIXES.md](../../../../.cursor/skills/statix-ebpf-agent/L8_POST_GA_FIXES.md)) — fire-and-forget `tokio::spawn` tasks could panic without visibility; ingest capacity pre-check was TOCTOU under concurrent `POST /ingest`.

## Decision

### V3-7 — K8s watcher panic visibility (`statix/src/main.rs`)

- Store `JoinHandle` from `spawn_k8s_watcher` in the main `select!` loop (branch gated by `in_k8s`).
- On `JoinError`: log `error!`, increment `statix_k8s_watcher_panics_total`, restart watcher.
- On normal exit: log `warn!`, restart watcher.

### V3-8 — Ring drops monitor panic visibility (`statix/src/loader.rs`, `main.rs`)

- `spawn_ring_drops_monitor` returns `JoinHandle<()>` instead of dropping it.
- Main loop `select!` branch awaits handle; on `JoinError` log + `statix_ring_monitor_panics_total`.

### V3-13 — Atomic batch accept on ingest (`statix-gateway/src/routes/ingest.rs`)

- Pre-serialize all flat rows, then `kafka_tx.try_reserve_many(n)` (Tokio 1.33+).
- Send all rows through reserved `Permit`s — no partial batch delivery when capacity is insufficient (503 on `Full` before any send).
- Replaces non-atomic `capacity()` pre-check + per-row `try_send` ([ADR 038](../v2/038-phase55-v2-wave1-l8-fixes.md) V2-3 partial mitigation).

## Rationale

- Silent task death = months of undetected `k8s_resolved: false` or invisible ring drops at scale.
- TOCTOU split batches corrupt billing windows under concurrent ingest load.
- `try_reserve_many` preserves non-blocking 503 backpressure ([ADR 005](../../../005-non-blocking-ingest-pipeline.md)).

## Consequences

- **Positive:** Panics and ring-monitor failures observable via logs + Prometheus; ingest batches are all-or-nothing on capacity.
- **Negative:** K8s watcher restart loop could spin on persistent API failure — Wave 2 V3-9 adds reconnect backoff.
- **Unchanged:** Kafka producer path; agent HTTP retry worker.

## References

- [TODO.md](../../../../.cursor/skills/statix-ebpf-agent/TODO.md) — V3-7, V3-8, V3-13
- `statix/src/main.rs`, `statix/src/loader.rs`, `statix-gateway/src/routes/ingest.rs`

# ADR 003: Early flush instead of cap eviction

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** An earlier `enforce_cap` randomly removed cgroup keys when the map hit a size limit.

## Decision

When `active_len() >= max_keys` (4096), call `flush()` and emit the batch immediately. Never drop keys to make room.

## Rationale

- FinOps billing / capacity signals must not lose memory spikes from bursty pod churn.
- Early flush shortens that batch’s logical window but preserves every cgroup seen.
- Avoids map growth past preallocated capacity (no resize spike under load).

## Consequences

- **Positive:** No silent telemetry loss; bounded map size.
- **Negative:** More frequent stdout batches during exec storms; `main.rs` must emit on `try_early_flush` return paths.
- **Removed:** `enforce_cap` random eviction.

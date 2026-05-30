# ADR 002: Double-buffered aggregator maps

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Each flush window drained the active map and replaced it with a new empty `HashMap`, causing malloc/free per window.

## Decision

Keep two pre-sized `FxHashMap` buffers: `buffers: [FxHashMap; 2]`, ping-pong `active` index on flush, `.clear()` the drained buffer (retain capacity).

## Rationale

- Swapping pointers alone still allocated a fresh map for the incoming side.
- `.clear()` drops entries but keeps bucket capacity—steady-state zero alloc/dealloc per flush.
- Agent is single-threaded on the aggregator (`&mut` from one Tokio task); no cross-thread map sharing required.

## Consequences

- **Positive:** Predictable memory and no per-window heap churn.
- **Negative:** ~2× map capacity reserved at startup (`DEFAULT_MAX_KEYS` each).
- **Code:** `finops-user/src/aggregator.rs` — see ADR 004 for flush ordering.

# ADR 004: Flip active buffer before draining on flush

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** `flush()` originally iterated the active map (label resolution + `Vec` build) before `active = 1 - active`.

## Decision

On flush: if the active buffer is non-empty, capture `window_start_ns`, flip `active` and `reset_window()` **first**, then iterate `buffers[flush_idx]`, `clear()` it, return `BatchPayload`.

## Rationale

- Ingest paths use `active_mut()`; after flip they target the empty secondary buffer while the old buffer is drained.
- Correct double-buffer semantics if flush work is ever split or moved off the hot path.
- Today the whole `flush()` still holds `&mut Aggregator` in one task—ring buffer polling resumes after return.

## Consequences

- **Positive:** Semantically correct handoff; ready for deferred batch build later.
- **Negative:** None for current single-threaded loop.
- **Code:** `Aggregator::flush` in `aggregator.rs`.

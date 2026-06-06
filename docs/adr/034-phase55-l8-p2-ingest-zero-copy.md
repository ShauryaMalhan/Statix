# ADR 034: Phase 5.5 L8 audit — P2 ingest zero-copy hot path

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Final L8 playbook item F1 — eliminate per-row heap allocations on the gateway `POST /ingest` thread ([L8-AUDIT-FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8-AUDIT-FIXES.md), [ADR 033](033-phase55-l8-p1-week-gateway-fixes.md)).

## Decision

| Change | File | Detail |
|--------|------|--------|
| `KafkaQueueItem` | `finops-api/src/kafka.rs` | `(Arc<[u8]>, Vec<u8>)` — one node key alloc per batch; `Arc::clone` per row |
| `bytes_to_record` | `finops-api/src/kafka.rs` | Accept `Arc<[u8]>`; `key: Some(node.to_vec())` on producer thread |
| `FlatRowRef<'a>` | `finops-api/src/routes/ingest.rs` | Local `Serialize` struct borrowing from `IngestBatch` / `WorkloadRow` |
| Ingest loop | `finops-api/src/routes/ingest.rs` | No `FlatRow::from_ingest`; no `node`/`batch_id`/`agent_version` clones on HTTP thread |

## Rationale

- Prior path: N× `node_vec.clone()` + 3 string clones per row via `FlatRow::from_ingest` on the Tokio HTTP worker.
- `FlatRowRef` preserves schema v2 JSON field names and `skip_serializing_if` semantics identical to `statix_wire::FlatRow`.
- `node.to_vec()` for rskafka remains on the background producer task — off the ingest hot path ([ADR 005](005-non-blocking-ingest-pipeline.md)).

## Consequences

- **Positive:** HTTP ingest thread allocation count scales with JSON encode buffer only, not envelope string clones.
- **Negative:** `FlatRow::from_ingest` unused on ingest path; `statix-wire::FlatRow` retained for tests and `into_flat_rows`.
- **L8 playbook:** All audit fixes shipped (ADR 032, 033, 034).

## References

- [ADR 033](033-phase55-l8-p1-week-gateway-fixes.md)
- [statix-wire `FlatRow`](../../statix-wire/src/lib.rs)

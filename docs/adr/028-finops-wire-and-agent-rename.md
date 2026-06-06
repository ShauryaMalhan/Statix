# ADR 028: `statix-wire` crate and `statix` rename (Phase 7)

**Status:** Accepted  
**Date:** 2026-06-05  
**Context:** Phase 7 DX — duplicate ingest types in agent and API; crate name `finops-user` no longer matched the deployed binary (`statix`).

## Decision

1. **Rename** workspace member `finops-user/` → `statix/`; package/binary `statix`.
2. **New crate** `statix-wire` — shared wire types:
   - `IngestBatch` — `POST /ingest` JSON envelope
   - `WorkloadRow` — per-workload rollup in a batch
   - `FlatRow` — denormalized Kafka / ClickHouse `JSONEachRow` row
3. **Dependencies:** `statix` and `statix-gateway` depend on `statix-wire` via path; ingest handler uses `statix_wire::{IngestBatch, FlatRow}`; agent `emit_batch` serializes `IngestBatch`.
4. **Makefile:** `build-user` → `build-agent`; release binary `target/release/statix`.
5. **Deferred (shipped):** `finops-api` → `statix-gateway` in [ADR 035](035-phase7-workspace-restructure.md).

## Rationale

- Single source of truth for schema drift between agent POST and API Kafka flatten step.
- Aligns crate name with K8s/Docker binary `statix`.
- `FlatRow::from_ingest` keeps gateway flatten logic explicit; owned `FlatRow` matches JSONEachRow on the wire.

## Consequences

- **Positive:** Agent/API schema changes start in `statix-wire/src/lib.rs`.
- **Negative:** Gateway flatten clones batch metadata per row (acceptable; prior borrow-based `FlatRow` removed).
- **Migration:** Rebuild agent (`make build`); Docker agent image builds `-p statix`.

## References

- `statix-wire/src/lib.rs`
- `statix-gateway/src/routes/ingest.rs`
- `statix/src/output.rs`, `aggregator.rs`

# ADR 028: `finops-wire` crate and `finops-agent` rename (Phase 7)

**Status:** Accepted  
**Date:** 2026-06-05  
**Context:** Phase 7 DX — duplicate ingest types in agent and API; crate name `finops-user` no longer matched the deployed binary (`finops-agent`).

## Decision

1. **Rename** workspace member `finops-user/` → `finops-agent/`; package/binary `finops-agent`.
2. **New crate** `finops-wire` — shared wire types:
   - `IngestBatch` — `POST /ingest` JSON envelope
   - `WorkloadRow` — per-workload rollup in a batch
   - `FlatRow` — denormalized Kafka / ClickHouse `JSONEachRow` row
3. **Dependencies:** `finops-agent` and `finops-api` depend on `finops-wire` via path; ingest handler uses `finops_wire::{IngestBatch, FlatRow}`; agent `emit_batch` serializes `IngestBatch`.
4. **Makefile:** `build-user` → `build-agent`; release binary `target/release/finops-agent`.
5. **Deferred:** `finops-api` → `finops-gateway` rename (separate change).

## Rationale

- Single source of truth for schema drift between agent POST and API Kafka flatten step.
- Aligns crate name with K8s/Docker binary `finops-agent`.
- `FlatRow::from_ingest` keeps gateway flatten logic explicit; owned `FlatRow` matches JSONEachRow on the wire.

## Consequences

- **Positive:** Agent/API schema changes start in `finops-wire/src/lib.rs`.
- **Negative:** Gateway flatten clones batch metadata per row (acceptable; prior borrow-based `FlatRow` removed).
- **Migration:** Rebuild agent (`make build`); Docker agent image builds `-p finops-agent`.

## References

- `finops-wire/src/lib.rs`
- `finops-api/src/routes/ingest.rs`
- `finops-agent/src/output.rs`, `aggregator.rs`

# ADR 020: Ingest schema version window (2 and 3)

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Hard reject `schema_version != 2` breaks rolling K8s upgrades when gateway and agents briefly disagree on version ([Phase 5 TODO](../../.cursor/skills/finops-ebpf-agent/TODO.md)).

## Decision

`POST /ingest` accepts `schema_version` in **2..=3** inclusive. Versions outside that range return `400` with body `Unsupported schema_version=N. Expected 2 or 3.`

Wire layout for v2 and v3 is identical today; v3 is reserved for the next agent release. When v3 fields ship, extend parsing in agent and gateway together and bump `MAX_SCHEMA_VERSION`.

## Consequences

- **Positive:** Gateway can deploy before or after agents without dropping valid batches during the window.
- **Negative:** Must keep v2 and v3 JSON compatible until v4 retires v2.
- **Agent:** Still emits `SCHEMA_VERSION = 2` until a v3 contract is defined.

## References

- `finops-api/src/routes/ingest.rs`

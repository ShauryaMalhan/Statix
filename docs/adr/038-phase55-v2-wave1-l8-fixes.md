# ADR 038: Phase 5.5 V2 L8 Wave 1 fixes

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** L8 Audit V2 P0/P1 — data integrity, availability, kernel scheduling ([L8_AUDIT_V2_FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8_AUDIT_V2_FIXES.md) Wave 1).

## Decision

| ID | Area | Fix |
|----|------|-----|
| V2-1 | `statix/src/main.rs` | SIGTERM + SIGINT → `agg.flush()` before exit (K8s eviction) |
| V2-2 | `deploy/clickhouse/01_init.sql` | `ReplacingMergeTree(window_end_ns)` — deterministic merge winner on retry |
| V2-3 | `statix-gateway/src/routes/ingest.rs` | Pre-check `kafka_tx.capacity()` ≥ `batch.workloads.len()` — atomic batch accept/reject |
| V2-9 | `statix-ebpf` + agent | `WAKEUP_COUNTER` + `BPF_RB_NO_WAKEUP` on 63/64 events; 1ms poll drain fallback |

## Consequences

- **V2-2:** Existing CH volumes require `docker compose down -v && make compose-up`.
- **V2-9:** Rebuild eBPF bundle (`make build-ebpf`) before agent deploy.

## References

- [ADR 011](011-replacingmergetree-dedupe-identity.md) — dedupe identity (amended by V2-2)
- [ADR 010](010-kafka-partition-key-by-node.md) — partition routing
- [TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md)

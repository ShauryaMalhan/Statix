# ADR 018: Phase roadmap status (Phases 4–6 complete → Phase 5 focus)

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Phase 4 (scale & reliability) and Phase 6 (L8 mechanical sympathy) shipped; logs and `TODO.md` still referenced Phase 3 as “current,” causing onboarding drift.

## Decision

| Phase | Status | Summary |
|-------|--------|---------|
| 1–3 | **Done** | eBPF agent, batched telemetry, HTTP → Kafka → ClickHouse E2E |
| 4 | **Done** | Partition routing, retry/jitter, dedupe, Prometheus, ring tiers, clock offset, lineage, cgroup bootstrap |
| 5 | **Active** | Auth, schema 2..3, `/ready`, ring drops + agent `:9091/metrics`, attribution/bearer hot-path fixes ([ADR 023](023-phase5-hot-path-fixes.md)); TLS + prod CH/Kafka ops remain |
| 6 | **Done** | Single attribution lock, `FxHashMap`, `Arc` labels/paths, sync retry drop-oldest, `Vec<u8>` Kafka queue; P0 regressions fixed in [ADR 023](023-phase5-hot-path-fixes.md) |
| 7–10 | **Planned** | Wire crate, K8s deploy, portability, extended observability |

Engineering gate doc: [phase5-production-readiness.md](../phase5-production-readiness.md).

## Consequences

- **Positive:** Single source of truth for “what’s next” (Phase 5 security).
- **Negative:** Historical docs still titled “Phase 3” where they describe the ingest *interface* (unchanged wire contract).

## References

- [.cursor/skills/finops-ebpf-agent/TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md)
- `finops-user/src/main.rs`, `finops-api/src/main.rs`

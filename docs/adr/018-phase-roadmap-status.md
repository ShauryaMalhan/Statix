# ADR 018: Phase roadmap status

**Status:** Accepted (updated 2026-06-04)  
**Date:** 2026-06-01  
**Context:** Onboarding drift when phase labels, deploy targets, and ADRs fall out of sync.

## Decision

| Phase / target | Status | Summary |
|----------------|--------|---------|
| 1–3 | **Done** | eBPF agent, batched telemetry, HTTP → Kafka → ClickHouse E2E |
| 4 | **Done** | Partition routing, retry/jitter, dedupe, Prometheus, ring tiers, clock offset, lineage, cgroup bootstrap |
| 5 / 5.5 | **Done** | L8 audit complete ([ADR 032](032-phase55-l8-p0-hot-path-fixes.md)–[034](034-phase55-l8-p2-ingest-zero-copy.md)); TLS; prod CH/Kafka ops remain in Phase 5 |
| 6 | **Done** | L8 hot path + P0 fixes ([ADR 023](023-phase5-hot-path-fixes.md)); micro-opts remain in TODO |
| **Target 1** | **Done** | `deploy/docker/*`, `deploy/k8s/*` ([ADR 024](024-agent-production-container.md), [025](025-kubernetes-gateway-and-agent.md)) |
| **Target 2** | **Done** | `deploy/clickhouse/01_init.sql` → `finops.workload_metrics` ([ADR 026](026-clickhouse-finops-database-init.md)) |
| **Target 3** | **Done** | `GET /api/v1/workloads/summary` → ClickHouse ([ADR 027](027-api-read-path-clickhouse.md)) |
| 7 | **Done** | Workspace restructure + typed errors + read-only labels ([ADR 028](028-finops-wire-and-agent-rename.md)–[036](036-phase7-typed-errors-labels-read-path.md)) |
| 9 | **Partial** | eBPF verifier CI matrix shipped ([ADR 037](037-phase9-ebpf-verifier-ci.md)); arm64 / cgroup v1 open |
| 8, 10 | **Planned** | K8s hardening, extended observability |

Engineering gate: [phase5-production-readiness.md](../phase5-production-readiness.md).  
Canonical task list: [.cursor/skills/finops-ebpf-agent/TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md).

## Consequences

- **Positive:** Single map from phases → code → `deploy/` artifacts.
- **Negative:** Phase 3 doc titles still describe the ingest *wire* contract (unchanged paths).

## References

- ADRs [019](019-ingest-bearer-token-auth.md)–[026](026-clickhouse-finops-database-init.md)
- `deploy/` tree

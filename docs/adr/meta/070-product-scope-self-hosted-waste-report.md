# ADR 070: Product scope — a self-hosted, single-tenant Kubernetes waste report

**Status:** Accepted  
**Date:** 2026-09-26

## Context

Statix grew phase by phase without a written product goal. Individual decisions were sound, but nothing said what the pipeline is *for*, so nothing said what to leave out — and the TODO list mixed goal-critical gaps with general dashboard polish.

The product is now defined ([docs/PRODUCT.md](../../PRODUCT.md)): a read-only Kubernetes waste report — right-sizing by need, short-lived job cost, zombie pods, egress cost — sold to companies, with an open-source core. "Tenant-based buyers" could mean either of two architectures, and they diverge on every layer.

## Decision

1. **Self-hosted, one install per company.** Each buying company runs its own agent DaemonSet, gateway and ClickHouse. One company = one tenant = one install. Data never leaves the customer's cluster.
2. **Single-tenant stays the design.** No `tenant_id` column, no per-tenant auth or query isolation, no shared backend. The dashboard cache keyed on query params rather than caller ([ADR 061](../ui/061-phase15-dashboard-read-tier.md)) remains correct.
3. **A waste report, not a monitoring app.** Features are judged against the four goals in `docs/PRODUCT.md`. General monitoring (alerting, logs, traces, arbitrary dashboards) is out of scope.
4. **Stay light.** Prefer reading kernel-maintained files (cgroupfs/procfs) over new eBPF; add no new moving parts (queues, sidecars, databases).

## Alternatives considered

- **Hosted multi-tenant SaaS.** One shared backend for many companies. Rejected for now: needs a tenant id on every row, per-tenant credentials, strict isolation in every query, and per-tenant limits — a large cross-cutting change, and customers would have to send infrastructure topology off-cluster. Revisit only with paying demand; nothing decided here prevents it.

## Consequences

- **Positive:** a filter for every future TODO item and PR ("which goal does this serve?"). Security work that matters to a self-hosting company — auth on by default, security headers — stays in scope; per-tenant work does not.
- **Negative:** each customer operates their own ClickHouse. The install and upgrade story therefore has to be simple, which is itself a reason to stay light.
- **TODO.md** is reordered by goal; items that serve none of the goals are parked, not deleted.

## References

- [docs/PRODUCT.md](../../PRODUCT.md) — goals and the "stay light" rule
- [ADR 055](../ingest/055-phase13-part1-kafka-removal-rowbinary.md) — Kafka removed: the precedent for fewer moving parts
- [ADR 061](../ui/061-phase15-dashboard-read-tier.md) — single-tenant dashboard read tier

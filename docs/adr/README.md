# Architecture Decision Records (ADRs)

Point-in-time notes on **why** we chose something — not polished docs. When code changes, add a new numbered file; don't rewrite history.

**➡️ [Full index, grouped by topic](INDEX.md)**

## Rules

- **The number is the identity.** It never changes, even if the file moves folders. Links elsewhere in the repo reference numbers.
- **Numbering is global-sequential** across all folders — take the next number after the current highest.
- **Never edit a decision to reflect a later change.** Supersede it with a new ADR and cross-link.
- Every architectural change adds/updates an ADR **and** syncs [enterprise-latency.md](../guides/enterprise-latency.md) + `.cursor/skills/statix-ebpf-agent/`.

## Folders

| Folder | What lives there |
|--------|------------------|
| [`ebpf/`](ebpf/) | BPF program, ring buffer sizing, verifier CI |
| [`agent/`](agent/) | Aggregator, attribution, clock domain, cgroup sampling |
| [`ingest/`](ingest/) | Agent→gateway HTTP, retry, WAL spillway, wire contract |
| [`gateway/`](gateway/) | Endpoints, probes, auth, config |
| [`storage/`](storage/) | ClickHouse schema, tuning, dedup, skip index |
| [`observability/`](observability/) | Metrics and saturation signals |
| [`ui/`](ui/) | Read tier and dashboard |
| [`deploy/`](deploy/) | Docker, Compose, Kubernetes, TLS, secrets |
| [`fixes/`](fixes/) | Audit / fix waves — cross-cutting by nature |
| [`meta/`](meta/) | Renames, workspace restructure, roadmap |
| [`kafka-legacy/`](kafka-legacy/) | Removed Phase 13 — history only, do not reintroduce |

## Writing a good one

State what you decided, **why**, and what you rejected. Then — the part that actually earns its keep — write down what it **costs**: the known weakness you accepted and when it will bite. Six months on, that's the difference between "past me was careless" and "past me already weighed this."

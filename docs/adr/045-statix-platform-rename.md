# ADR 045: FinOps → Statix platform rename (shared crates & ops surface)

**Status:** Accepted  
**Date:** 2026-06-07  
**Context:** [ADR 044](044-statix-agent-rename.md) renamed the node agent to `statix`. Shared libraries, gateway, eBPF crate, K8s/Compose names, ClickHouse database, Kafka topic wiring, env prefixes, and metrics still used `finops-*` / `FINOPS_*`, which split branding and complicated deploy docs.

## Decision

### Workspace crates (directory + Cargo `package.name`)

| Before | After |
|--------|-------|
| `finops-common/` | `statix-common/` |
| `finops-wire/` | `statix-wire/` |
| `finops-infra/` | `statix-infra/` |
| `finops-gateway/` | `statix-gateway/` |
| `finops-ebpf/` (out-of-workspace BPF) | `statix-ebpf/` |
| `statix/` (agent) | unchanged |

Root `Cargo.toml` members: `statix-common`, `statix-wire`, `statix-infra`, `statix`, `statix-gateway`.

### Rust / wire symbols

- `finops_common`, `finops_wire`, `finops_infra` → `statix_*` crate roots.
- `FinopsEvent` → `StatixEvent`; BPF tracepoint hook `finops_sched_process_exec` → `statix_sched_process_exec`.
- eBPF binary artifact: `statix-ebpf` (+ `statix-ebpf-{small,large,xlarge}` bundles).

### Environment variables

- Operational prefix `FINOPS_*` → `STATIX_*` (e.g. `STATIX_API_TOKEN`, `STATIX_INGEST_URL`, `STATIX_BPF_DIR`).
- **`statix-infra::env`:** reads `STATIX_*` first, then falls back to legacy `FINOPS_*` for the same suffix (migration window).

### Kubernetes / Compose

- Namespace / network / secrets: `statix-system`, `statix-net`, `statix-secrets`, `statix-telemetry`.
- Service: `statix-gateway-svc`; Grafana container `statix-grafana`.
- Kafka cluster display name in Compose: `statix`.

### ClickHouse

- Database `finops` → `statix`.
- Tables/views: `statix.workload_metrics`, `statix.kafka_telemetry_queue`, `statix.telemetry_mv`.
- Grafana datasource SQL: `database = 'statix'`.

### Metrics / API

- Prometheus prefix `finops_api_*` → `statix_api_*`.
- Agent self-metrics and docs reference `statix` crates consistently.

### Tooling

- `Makefile`: `EBPF_OUT_NAME := statix-ebpf`; `AGENT_DIR` already `statix/`.
- Cursor skill remains `.cursor/skills/statix-ebpf-agent/`; internal references updated to `statix-*` crates.

## Rationale

- One product name (Statix) across agent, gateway, shared libs, and data plane.
- Legacy `FINOPS_*` env fallback avoids breaking existing Compose/K8s secrets during rollout.
- ADR 044 intentionally left shared crates on FinOps naming; this ADR completes the rename.

## Consequences

- **Positive:** `cargo check -p statix-gateway` and path deps align with directory names; CH queries use `statix` database.
- **Negative:** Existing ClickHouse volumes with `finops` DB require re-init or manual `RENAME`/migration; external forks must update import paths.
- **Migration:** Re-run `deploy/clickhouse/01_init.sql` on fresh volumes or migrate data; export `STATIX_*` (or keep `FINOPS_*` until cutover); `make check`.

## References

- Workspace: `Cargo.toml`
- Init SQL: `deploy/clickhouse/01_init.sql`
- Env helper: `statix-infra/src/env.rs`
- Prior agent rename: [ADR 044](044-statix-agent-rename.md), [ADR 028](028-statix-wire-and-agent-rename.md)

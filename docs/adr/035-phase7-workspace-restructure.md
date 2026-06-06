# ADR 035: Phase 7 workspace restructure — `finops-gateway` + `finops-infra`

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 7 structural tasks ([TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md)) — rename gateway crate, extract shared utilities, remove dead types. Deferred from [ADR 028](028-finops-wire-and-agent-rename.md).

## Decision

### 1. Rename `finops-api` → `finops-gateway`

- Crate directory `finops-gateway/`; package/binary `finops-gateway`.
- Workspace member replaces `finops-api` in root `Cargo.toml`.
- Docker Compose service `finops-gateway`; dev image `Dockerfile.gateway` (was `Dockerfile.api`).
- Makefile: `build-gateway`, `run-gateway`, `stop-gateway` (+ `build-api` / `run-api` aliases).

### 2. New `finops-infra` library crate

| Module | Contents |
|--------|----------|
| `env.rs` | `read_env_u64`, `read_env_usize` (deduped from agent `output.rs` and gateway `kafka.rs`) |
| `clock.rs` | `mono_now_ns`, `wall_unix_ns`, `calibrate_clock_offset_ns` (from agent `aggregator.rs`) |

Dependencies: `finops-agent`, `finops-gateway`.

### 3. Remove `ProcessEvent`

- Deleted deprecated Phase 1 alias from `finops-common`; `FinopsEvent` + `EVENT_KIND_*` remain.

## Rationale

- Crate name matches K8s deployment (`finops-gateway`) and binary identity.
- Shared env/clock helpers prevent drift between agent and gateway tuning parsers.
- `ProcessEvent` had no callers after unified `FinopsEvent` layout.

## Consequences

- **Positive:** `cargo check --workspace` validates all host crates including `finops-infra`.
- **Negative:** Historical ADRs may still say `finops-api` in titles; current paths use `finops-gateway`.
- **Unchanged:** Ingest/aggregation logic; env var names (`FINOPS_API_*`, `KAFKA_BROKERS`, …).

## References

- [ADR 028](028-finops-wire-and-agent-rename.md)
- [ADR 030](030-finops-api-config-struct.md) — `finops-gateway/src/config.rs`

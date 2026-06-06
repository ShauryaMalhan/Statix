# ADR 035: Phase 7 workspace restructure — `statix-gateway` + `statix-infra`

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 7 structural tasks ([TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md)) — rename gateway crate, extract shared utilities, remove dead types. Deferred from [ADR 028](028-statix-wire-and-agent-rename.md).

## Decision

### 1. Rename `finops-api` → `statix-gateway`

- Crate directory `statix-gateway/`; package/binary `statix-gateway`.
- Workspace member replaces `finops-api` in root `Cargo.toml`.
- Docker Compose service `statix-gateway`; dev image `Dockerfile.gateway` (was `Dockerfile.api`).
- Makefile: `build-gateway`, `run-gateway`, `stop-gateway` (+ `build-api` / `run-api` aliases).

### 2. New `statix-infra` library crate

| Module | Contents |
|--------|----------|
| `env.rs` | `read_env_u64`, `read_env_usize` (deduped from agent `output.rs` and gateway `kafka.rs`) |
| `clock.rs` | `mono_now_ns`, `wall_unix_ns`, `calibrate_clock_offset_ns` (from agent `aggregator.rs`) |

Dependencies: `statix`, `statix-gateway`.

### 3. Remove `ProcessEvent`

- Deleted deprecated Phase 1 alias from `statix-common`; `StatixEvent` + `EVENT_KIND_*` remain.

## Rationale

- Crate name matches K8s deployment (`statix-gateway`) and binary identity.
- Shared env/clock helpers prevent drift between agent and gateway tuning parsers.
- `ProcessEvent` had no callers after unified `StatixEvent` layout.

## Consequences

- **Positive:** `cargo check --workspace` validates all host crates including `statix-infra`.
- **Negative:** Historical ADRs may still say `finops-api` in titles; current paths use `statix-gateway`.
- **Unchanged:** Ingest/aggregation logic; env var names (`STATIX_API_*`, `KAFKA_BROKERS`, …).

## References

- [ADR 028](028-statix-wire-and-agent-rename.md)
- [ADR 030](030-finops-api-config-struct.md) — `statix-gateway/src/config.rs`

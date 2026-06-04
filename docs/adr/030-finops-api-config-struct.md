# ADR 030: Centralized `finops-api` `Config`

**Status:** Accepted  
**Date:** 2026-06-05  
**Context:** Phase 7 DX — scattered `std::env::var` in `main.rs` made defaults and validation inconsistent ([TODO](../../.cursor/skills/finops-ebpf-agent/TODO.md)).

## Decision

- **`finops-api/src/config.rs`** — `Config::from_env()` at the top of `main()` (before `env_logger::init()`).
- **Fields:** `kafka_brokers`, `api_port`, `api_token`, `clickhouse_url`, `clickhouse_user`, `clickhouse_password` with documented defaults.
- **Fail fast:** invalid `FINOPS_API_PORT` (non-u16 or `0`) → `eprintln!` + `process::exit(1)` (no silent fallback).
- **Helpers:** `expected_bearer()`, `clickhouse_client()` on `Config`.
- **Deferred:** Kafka tuning env (`FINOPS_KAFKA_CHANNEL_SIZE`, etc.) remain in `kafka.rs` ([ADR 014](014-kafka-producer-env-tuning.md)).

## Consequences

- **Positive:** Single place for gateway env contract; `main.rs` wires clients from `config`.
- **Negative:** Port misconfiguration kills the process at startup (intentional).

## Env → `Config` mapping

| Environment variable | Field | Default | Validation |
|---------------------|-------|---------|------------|
| `KAFKA_BROKERS` | `kafka_brokers` | `localhost:9092` | empty → default + warn |
| `FINOPS_API_PORT` | `api_port` | `3000` | invalid / `0` → **exit 1** |
| `FINOPS_API_TOKEN` | `api_token` | `None` | empty string → `None` |
| `CLICKHOUSE_URL` | `clickhouse_url` | `http://localhost:8123` | empty → default + warn |
| `CLICKHOUSE_USER` | `clickhouse_user` | `default` | empty → default + warn |
| `CLICKHOUSE_PASSWORD` | `clickhouse_password` | `""` | always allowed |

## References

- `finops-api/src/main.rs`, `config.rs`

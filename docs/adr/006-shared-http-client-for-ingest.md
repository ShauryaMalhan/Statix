# ADR 006: Shared `reqwest::Client` for ingest POST

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 3 plan noted that `reqwest::Client::new()` per batch creates a new connection pool each flush.

## Decision

Initialize one `reqwest::Client` at agent startup via `output::init_http_client()` and `static HTTP_CLIENT: OnceLock<reqwest::Client>`. All `tokio::spawn` ingest tasks clone this client.

Builder settings (process-wide):

- **`timeout(Duration::from_secs(3))`** — entire request (connect + send + response). Prevents black-hole TCP from leaving spawned tasks alive forever (RAM growth on ~10s flush cadence).
- **`pool_idle_timeout(Duration::from_secs(90))`** — reuse connections between flushes without unbounded idle sockets.

## Rationale

- Connection pooling and TLS session reuse require a long-lived client.
- `emit_batch` is on the hot path; client construction per batch adds alloc + TCP setup under flush storms.
- Default `reqwest` has no timeout; VPC/proxy black holes would accumulate unbounded `tokio::spawn` tasks and retained JSON bodies.

## Consequences

- **Positive:** Stable latency for repeated `POST` to `finops-api`; hung ingest fails fast and drops the batch (agent keeps sampling).
- **Negative:** Slow or distant API may log `ingest POST failed` under 3s — acceptable vs node OOM.
- **Code:** `finops-user/src/output.rs`, called from `main.rs` before the event loop.

# ADR 006: Shared `reqwest::Client` for ingest POST

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 3 plan noted that `reqwest::Client::new()` per batch creates a new connection pool each flush.

## Decision

Initialize one `reqwest::Client` at agent startup via `output::init_http_client()` and `static HTTP_CLIENT: OnceLock<reqwest::Client>`. All `tokio::spawn` ingest tasks clone this client.

## Rationale

- Connection pooling and TLS session reuse require a long-lived client.
- `emit_batch` is on the hot path; client construction per batch adds alloc + TCP setup under flush storms.

## Consequences

- **Positive:** Stable latency for repeated `POST` to `finops-api`.
- **Negative:** Client config is process-wide (timeouts/TLS tuned once — good for enterprise).
- **Code:** `finops-user/src/output.rs`, called from `main.rs` before the event loop.

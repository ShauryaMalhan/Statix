# ADR 006: Shared `reqwest::Client` and ingest retry worker

**Status:** Accepted (amended 2026-06-01)  
**Date:** 2026-05-28  
**Context:** Phase 3 plan noted that `reqwest::Client::new()` per batch creates a new connection pool each flush. Agent retries must not double-bill when K8s labels change between attempts ([ADR 011](011-replacingmergetree-dedupe-identity.md)).

## Decision

1. **`output::init_http_client()`** — one `reqwest::Client` via `static HTTP_CLIENT: OnceLock<reqwest::Client>`.
2. **`output::init_retry_worker(url)`** — single background task; bounded `mpsc(60)` (~10 min of 10s windows).
3. **`emit_batch`** — serialize JSON, `try_send` to retry queue (no per-batch `tokio::spawn` POST). On queue full: synchronous `try_lock` + drop-oldest + re-`try_send` (no spawned tasks on the hot path).
4. **Retry policy** — exponential backoff on transport errors, **5xx**, and **429**; reset after **2xx**. Non-retryable **4xx** logged and dropped.
5. **Backoff + jitter (thundering herd)** — env `FINOPS_BACKOFF_INITIAL_SECS` (default **1**), `FINOPS_BACKOFF_MAX_SECS` (default **30**). Sleep = `backoff + rand * (backoff * 0.3)` before doubling (AWS-style spread after gateway outages).

Builder settings (process-wide, env-configurable):

- **`FINOPS_HTTP_TIMEOUT_SECS`** (default **5**) — entire request timeout (connect + send + response). Cross-region VPCs need &gt;3s; still bounds black-hole TCP on the retry worker.
- **`FINOPS_HTTP_POOL_IDLE_SECS`** (default **55**) — idle connection reuse; default stays under typical **AWS ALB 60s** idle drop so the pool refreshes before the LB silently kills sockets.

## Rationale

- Connection pooling and TLS session reuse require a long-lived client.
- Fire-and-forget `spawn` per batch loses billing data on transient API/network failure.
- Bounded queue + drop-oldest caps agent RAM under prolonged API outage.

## Consequences

- **Positive:** Stable latency for repeated `POST` to `finops-api`; retries pair with ClickHouse dedupe sort key.
- **Negative:** Slow API may log retry warnings under sustained 503 — agent keeps sampling.
- **Code:** `finops-user/src/output.rs`, `init_*` from `main.rs` when `FINOPS_INGEST_URL` is set.

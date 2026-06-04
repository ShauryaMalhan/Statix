# ADR 019: Bearer token auth on `POST /ingest`

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Phase 5 requires untrusted networks cannot inject billing data ([phase5-production-readiness.md](../phase5-production-readiness.md)).

## Decision

1. **Gateway (`finops-api`):** `FINOPS_API_TOKEN` → `AppState.expected_bearer: Option<String>` at startup (`format!("Bearer {token}")` once — [ADR 023](023-phase5-hot-path-fixes.md)).
   - If set: `POST /ingest` requires exact `Authorization` header match; else `401 Unauthorized`.
   - If unset: auth disabled (local dev only); startup logs **ENABLED** / **DISABLED**.
2. **Agent (`finops-user`):** When `FINOPS_API_TOKEN` is set at `init_http_client()`, `reqwest::Client` uses `default_headers` with `Authorization: Bearer <token>` on every ingest `POST`.

TLS termination remains out of scope (load balancer or sidecar).

## Rationale

- Pre-shared bearer is minimal ops overhead vs mTLS for first production gate.
- Optional token preserves `make compose-up` dev flow without secrets.
- Agent and API share one env name for symmetric configuration.

## Consequences

- **Positive:** Production can require auth without code changes beyond env.
- **Negative:** Token in env vars — use K8s secrets / SSM in prod; rotate manually until vault integration.
- **Negative:** No constant-time compare yet; use network TLS + secret rotation for defense in depth.

## References

- `finops-api/src/main.rs`, `routes/ingest.rs` (`expected_bearer`)
- [ADR 023](023-phase5-hot-path-fixes.md)
- `finops-user/src/output.rs`

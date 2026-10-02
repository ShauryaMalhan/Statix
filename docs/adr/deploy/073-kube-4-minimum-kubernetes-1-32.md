# ADR 073: `kube` 4.2 + `k8s-openapi` 0.28; the agent supports Kubernetes 1.32 and newer

**Status:** Accepted  
**Date:** 2026-10-02

## Context

The agent used `kube` 0.98 with `k8s-openapi` 0.24 (feature `v1_30`). Stage 1c of requests/limits per pod rewrites the pod watcher, so the client library was upgraded first, letting that code be written once against the current API.

`k8s-openapi` must be exactly the version `kube` is built against. Dependabot bumped them separately twice — #19 (`k8s-openapi` alone) and #23 (`kube` alone, despite the new `kube` group from `b680736`) — and both times two copies were built, the one `kube` uses had no `v1_*` feature, and its build script panicked.

`k8s-openapi` carries one set of Rust types per Kubernetes release and supports a window of about five releases, matching Kubernetes' own support window: 0.26 offered `v1_30`–`v1_34`, 0.28 offers `v1_32`–`v1_36` (checked with `cargo info`). `v1_30` no longer exists.

## Decision

- `kube = "4.2"`, `k8s-openapi = "0.28"`, changed together, with a comment in `statix/Cargo.toml` saying they must match.
- Feature **`v1_32`, the lowest available**. Newer API servers return everything older ones did (plus fields we ignore), so the lowest feature works with the widest range of clusters.
- **The agent's minimum supported Kubernetes is now 1.32** (was 1.30).

### Verified (dev VM, k3s 1.36, 2026-10-02)

`cargo check` clean — no code changes needed; our watcher, list fallback and `k8s_client` use only APIs that did not change across 0.98 → 4.2. Live: `DEV MODE` warning, watcher initial sync for node `colima`, no client errors, all 7 pods (k3s system pods, `reqtest`, `guaranteed-test`) attributed as before. TLS unchanged: `main` still installs the `ring` rustls provider explicitly; `aws-lc-rs` was already in the lockfile.

## Consequences

- **Companies on Kubernetes 1.30/1.31 are not supported.** Check a customer's cluster version before an install; managed offerings generally stay within recent releases, but their calendars were not checked here.
- **Every `k8s-openapi` bump moves the minimum.** Choosing the lowest `v1_*` each time keeps the window as wide as possible; record the new minimum in README.
- Dependabot's PR #23 becomes redundant once this is pushed.

## References

- [ADR 064](../meta/064-dependabot-and-cargo-audit.md) — Dependabot and `cargo audit`
- [ADR 072](072-agent-kubernetes-access.md) — how the agent reaches Kubernetes

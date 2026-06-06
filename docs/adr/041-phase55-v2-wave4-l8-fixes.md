# ADR 041: Phase 5.5 V2 L8 Wave 4 fixes (GA hardening)

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** L8 Audit V2 Wave 4 — protect the K8s control plane, reduce cross-AZ cost, and pin supply-chain images.

## Decision

| ID | Area | Fix |
|----|------|-----|
| V2-4 | `statix/src/attribution/mod.rs` | Replace 30s `pods.list()` poll with `kube::runtime::watcher` (`watch_k8s_pods`); node field selector; `merge_cgroup_labels_from_k8s` on Apply + InitDone |
| V2-7 | `deploy/k8s/*.yaml` | Pin `image:` to `@sha256:<64-hex>` digests (placeholder production builds) |
| V2-8 | `deploy/k8s/gateway.yaml` | `topologySpreadConstraints` on `topology.kubernetes.io/zone`, `maxSkew: 1`, `ScheduleAnyway` |

## Rationale

- **V2-4:** 5000 agents × list/30s ≈ 167 API calls/sec — etcd overload. Watch streams deltas per node.
- **V2-7:** `:latest` tags drift across rolling restarts; digest pins immutable artifacts.
- **V2-8:** Spreading gateway replicas across AZs reduces cross-AZ ingest traffic and single-AZ blast radius.

## Consequences

- **RBAC:** Existing `watch` verb on `pods` ClusterRole is required (already present).
- **Deps:** `kube` `runtime` feature + `futures` in `statix`.
- **CI/CD:** Replace placeholder digests with pipeline-templated values (Kustomize/Helm overlay).
- **Fallback:** `refresh_k8s_pods` retained for one-shot list refresh / tests.

## References

- [ADR 025](025-kubernetes-gateway-and-agent.md) — K8s manifests + agent RBAC
- [ADR 036](036-phase7-typed-errors-labels-read-path.md) — K8s merge outside hot path
- [ADR 039](039-phase55-v2-wave2-l8-fixes.md) — `merge_cgroup_labels_from_k8s` lock discipline

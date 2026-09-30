# ADR 072: The agent reaches Kubernetes only as a DaemonSet pod; `STATIX_DEV_KUBECONFIG` is for development

**Status:** Accepted  
**Date:** 2026-10-01  
**Serves:** Goal 1 — requests/limits per pod need the pod watcher running ([docs/PRODUCT.md](../../PRODUCT.md))

## Context

The pod watcher (`watch_k8s_pods`) gives every cgroup its namespace, pod and — next — container name, requests and limits. It only started when `KUBERNETES_SERVICE_HOST` was set, a variable Kubernetes injects **into pods only**. In the dev loop the agent runs as a plain binary in the Colima VM next to k3s, so the watcher never started and every workload showed as unattributed. `dev-up.sh` also pinned `STATIX_NODE_NAME=colima-vm`, while the k3s node is `colima`, so even a working watcher's node-scoped query (`spec.nodeName=…`) would have matched no pods.

Two ways to test against a real cluster:

| | Binary in the VM | DaemonSet pod |
|---|---|---|
| change → test | `make build` + restart, ~1 min | build image → import into k3s → restart pod, several minutes |
| credentials | a kubeconfig file — on k3s, `/etc/rancher/k3s/k3s.yaml`, a **cluster-admin** key | the pod's own ServiceAccount |
| matches production | no | yes |

## Decision

1. **Production: the agent runs only as the DaemonSet pod** (`deploy/k8s/statix-daemonset.yaml`), one per node, using its ServiceAccount `statix-sa`, whose ClusterRole grants exactly `get`/`list`/`watch` on `pods`. One `kubectl apply` covers every node, including nodes added later; nothing is configured inside application pods.
2. **Development: `STATIX_DEV_KUBECONFIG=<path>`** lets the agent binary use a kubeconfig file, and it logs a `DEV MODE … never use this in production` warning at startup.
   - Deliberately **not** plain `KUBECONFIG`: that variable is often set on machines for unrelated reasons, and would silently hand the agent whatever key it points to. A name containing `DEV` cannot be enabled by accident.
   - An empty value means "off", so `dev-up.sh` can pass it unconditionally and set it only while k3s is running.
3. **One rule, three call sites:** `attribution::k8s_configured()` (in a pod, or a dev kubeconfig) decides whether the watcher starts, whether `main`'s `select!` restarts it after a crash, and whether `refresh_k8s_pods` runs. `attribution::k8s_client()` builds the client either way.
4. **`dev-up.sh` no longer overrides the node name.** The agent falls back to `/etc/hostname` (`colima`), which is also the k3s node name.

### Verified (dev VM, 2026-10-01)

Agent log: `node=colima`, the `DEV MODE` warning, `K8s watcher initial sync complete for node colima`. ClickHouse: all k3s system pods and the Guaranteed `guaranteed-test` pod resolved with `k8s_resolved = true` (the latter only since the pod-UID parser fix in the same item). The two-container `reqtest` Deployment was recreated and confirmed by the user.

## Alternatives considered

- **Accept plain `KUBECONFIG`.** Rejected: accidental admin access on any machine where it happens to be set.
- **DaemonSet only, even in development.** Rejected for the inner loop — several minutes per change — but the report ships only after one end-to-end run through the real DaemonSet.

## Consequences

- **Positive:** the dev loop can see real pods; production posture is explicit and least-privilege.
- **Negative:** a dev-only code path exists in the binary. It is inert unless the variable is set, and loud when it is.
- **Dashboard:** the dev node renamed from `colima-vm` to `colima`; old rows show as a stale second node for up to the health window.

## References

- [ADR 025](025-kubernetes-gateway-and-agent.md) — Kubernetes gateway Deployment + agent DaemonSet
- [ADR 070](../meta/070-product-scope-self-hosted-waste-report.md) — self-hosted, one install per company

# ADR 025: Kubernetes gateway Deployment + agent DaemonSet

**Status:** Accepted  
**Date:** 2026-06-04  
**Context:** Target 1 packaging — production images ([ADR 009](009-finops-api-docker-compose.md), [ADR 024](024-agent-production-container.md)) need cluster orchestration.

## Decision

1. **`deploy/k8s/gateway.yaml`**
   - `Namespace` `statix-system`
   - `Deployment` `statix-gateway` (2 replicas): image `statix-gateway:latest`, `STATIX_API_TOKEN` from `statix-secrets`, `KAFKA_BROKERS` cluster DNS
   - Liveness `/health`, readiness `/ready` on port 3000 (`initialDelaySeconds: 2`)
   - `Service` `statix-gateway-svc` ClusterIP :3000

2. **`deploy/k8s/statix-daemonset.yaml`**
   - `ServiceAccount` `statix-sa` + `ClusterRole`/`Binding` (list/watch `pods` for K8s label refresh)
   - `DaemonSet` `statix`: `hostPID: true`, `privileged: true`, toleration `operator: Exists`
   - Env: `STATIX_INGEST_URL` → gateway Service DNS, shared bearer secret, `STATIX_NODE_NAME` downward API
   - `hostPath` volumes: `/sys/fs/cgroup`, `/proc` (read-only mounts)

3. **Secret:** `statix-secrets` / `api-token` created out-of-band (not committed).

## Consequences

- **Positive:** Matches dev probes and ingest URL contract; agents on all nodes including control-plane (toleration).
- **Negative:** Privileged DaemonSet — cluster policy review required.
- **Negative:** Image tags `latest` + `imagePullPolicy: Always` — pin digests in prod.

## References

- [deploy/k8s/README.md](../../deploy/k8s/README.md)
- [ADR 021](021-ingest-ready-probe.md), [ADR 019](019-ingest-bearer-token-auth.md)

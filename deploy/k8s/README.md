# Kubernetes — FinOps platform (Target 1)

Namespace: `finops-system`. Apply from repo root:

```bash
# 1) Secret (same token on gateway + agent)
kubectl create namespace finops-system --dry-run=client -o yaml | kubectl apply -f -
kubectl -n finops-system create secret generic finops-secrets \
  --from-literal=api-token='CHANGE_ME' \
  --from-literal=clickhouse-password='CHANGE_ME' \
  --dry-run=client -o yaml | kubectl apply -f -

# 2) Gateway (includes Namespace)
kubectl apply -f deploy/k8s/gateway.yaml

# 3) Agent DaemonSet (+ ServiceAccount + RBAC for in-cluster pod labels)
kubectl apply -f deploy/k8s/agent-daemonset.yaml
```

## Verify

```bash
kubectl -n finops-system get deploy,ds,svc,pods
kubectl -n finops-system rollout status deployment/finops-gateway
curl -s "http://$(kubectl -n finops-system get svc finops-gateway-svc -o jsonpath='{.spec.clusterIP}'):3000/health"
```

## Images

Build and push to your registry, then update the `@sha256:...` digests in manifests (replace placeholder digests after each release):

- [../docker/Dockerfile.gateway](../docker/Dockerfile.gateway)
- [../docker/Dockerfile.agent](../docker/Dockerfile.agent)

## Notes

- **Kafka:** `KAFKA_BROKERS` points at `kafka-broker.default.svc.cluster.local:9092` — adjust for your cluster.
- **ClickHouse:** `CLICKHOUSE_URL` on gateway — adjust host; password from `finops-secrets` key `clickhouse-password` ([ADR 027](../../docs/adr/027-api-read-path-clickhouse.md)).
- **Agent:** `privileged: true`, `hostPID: true`, host `/proc` and `/sys/fs/cgroup` mounts ([ADR 024](../../docs/adr/024-agent-production-container.md)).
- **Metrics:** scrape agent pods on port `9091` (`finops_agent_ring_drops_total`, etc.).
- **Eviction / drain:** agent DaemonSet and gateway Deployment use `terminationGracePeriodSeconds: 30` and `preStop: sleep 5` so SIGTERM flush keeps a live network path ([ADR 040](../../docs/adr/040-phase55-v2-wave3-l8-fixes.md)).
- **Gateway PDB:** `finops-gateway-pdb` `minAvailable: 1` — at least one replica during node drains ([ADR 040](../../docs/adr/040-phase55-v2-wave3-l8-fixes.md)).
- **Cross-AZ spread:** gateway `topologySpreadConstraints` on `topology.kubernetes.io/zone` ([ADR 041](../../docs/adr/041-phase55-v2-wave4-l8-fixes.md)).
- **Digest pins:** images use `@sha256:<64-hex>` — template from CI/CD ([ADR 041](../../docs/adr/041-phase55-v2-wave4-l8-fixes.md)).
- **Agent K8s labels:** `watch_k8s_pods` streams node-scoped pod events (no 30s list poll) ([ADR 041](../../docs/adr/041-phase55-v2-wave4-l8-fixes.md)).

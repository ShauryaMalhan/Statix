# Kubernetes — FinOps platform (Target 1)

Namespace: `finops-system`. Apply from repo root:

```bash
# 1) Secret (same token on gateway + agent)
kubectl create namespace finops-system --dry-run=client -o yaml | kubectl apply -f -
kubectl -n finops-system create secret generic finops-secrets \
  --from-literal=api-token='CHANGE_ME' \
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

Build and push to your registry, then set image on manifests (replace `finops-gateway:latest` / `finops-agent:latest`):

- [../docker/Dockerfile.gateway](../docker/Dockerfile.gateway)
- [../docker/Dockerfile.agent](../docker/Dockerfile.agent)

## Notes

- **Kafka:** `KAFKA_BROKERS` points at `kafka-broker.default.svc.cluster.local:9092` — adjust for your cluster.
- **Agent:** `privileged: true`, `hostPID: true`, host `/proc` and `/sys/fs/cgroup` mounts ([ADR 024](../../docs/adr/024-agent-production-container.md)).
- **Metrics:** scrape agent pods on port `9091` (`finops_agent_ring_drops_total`, etc.).

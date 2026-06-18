# Kubernetes — Statix platform (Target 1)

Namespace: `statix-system`. Apply from repo root:

```bash
# 1) Secret (same token on gateway + agent)
kubectl create namespace statix-system --dry-run=client -o yaml | kubectl apply -f -
kubectl -n statix-system create secret generic statix-secrets \
  --from-literal=api-token='CHANGE_ME' \
  --from-literal=clickhouse-password='CHANGE_ME' \
  --dry-run=client -o yaml | kubectl apply -f -

# 2) Gateway (includes Namespace)
kubectl apply -f deploy/k8s/gateway.yaml

# 3) Agent DaemonSet (+ ServiceAccount + RBAC for in-cluster pod labels)
kubectl apply -f deploy/k8s/statix-daemonset.yaml

# 4) Gateway Ingress (AWS ALB TLS termination — requires AWS LB Controller + ACM cert)
#    Replace certificate-arn in gateway-ingress.yaml before apply.
kubectl apply -f deploy/k8s/gateway-ingress.yaml
```

## Architecture

```
statix agent (DaemonSet) → POST /ingest → statix-gateway → ClickHouse
```

No Kafka broker required. Gateway coalesces rows and inserts RowBinary into `statix.workload_metrics` ([ADR 055](../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).

## Verify

```bash
kubectl -n statix-system get deploy,ds,svc,ingress,pods
kubectl -n statix-system rollout status deployment/statix-gateway
curl -s "http://$(kubectl -n statix-system get svc statix-gateway-svc -o jsonpath='{.spec.clusterIP}'):3000/health"
curl -s "http://$(kubectl -n statix-system get svc statix-gateway-svc -o jsonpath='{.spec.clusterIP}'):3000/ready"
```

## Images

Build and push to your registry, then update the `@sha256:...` digests in manifests (replace placeholder digests after each release):

- [../docker/Dockerfile.gateway](../docker/Dockerfile.gateway)
- [../docker/Dockerfile.statix](../docker/Dockerfile.statix)

## Notes

- **ClickHouse:** `CLICKHOUSE_URL` on gateway — adjust host; password from `statix-secrets` key `clickhouse-password` ([ADR 027](../../docs/adr/027-api-read-path-clickhouse.md)). Writer env: `STATIX_INGEST_CHANNEL_SIZE`, `STATIX_CH_BATCH_MAX`, `STATIX_CH_LINGER_MS`, `STATIX_CH_INSERT_TIMEOUT_SECS` ([ADR 055](../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).
- **Agent:** `privileged: true`, `hostPID: true`, host `/proc` and `/sys/fs/cgroup` mounts ([ADR 024](../../docs/adr/024-agent-production-container.md)).
- **Metrics:** scrape agent pods on port `9091` (`statix_ring_drops_total`, etc.).
- **Eviction / drain:** agent DaemonSet and gateway Deployment use `terminationGracePeriodSeconds: 30` and `preStop: sleep 5` so SIGTERM flush keeps a live network path ([ADR 040](../../docs/adr/phase55/v2/040-phase55-v2-wave3-l8-fixes.md)).
- **Gateway PDB:** `statix-gateway-pdb` `minAvailable: 1` — at least one replica during node drains ([ADR 040](../../docs/adr/phase55/v2/040-phase55-v2-wave3-l8-fixes.md)).
- **Cross-AZ spread:** gateway `topologySpreadConstraints` on `topology.kubernetes.io/zone` ([ADR 041](../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md)).
- **Digest pins:** images use `@sha256:<64-hex>` — template from CI/CD ([ADR 041](../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md)).
- **Agent K8s labels:** `watch_k8s_pods` streams node-scoped pod events (no 30s list poll) ([ADR 041](../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md)).
- **TLS / Ingress:** `gateway-ingress.yaml` — ALB terminates HTTPS on `ingest.your-startup.com/ingest`; gateway pod stays HTTP :3000 ([ADR 043](../../docs/adr/phase55/v2/043-kubernetes-alb-tls-termination.md)).

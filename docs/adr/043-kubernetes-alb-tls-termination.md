# ADR 043: TLS termination at AWS ALB Ingress

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** Phase 5 TLS blocker — offload HTTPS from the Rust gateway to Kubernetes infrastructure.

## Decision

Terminate TLS on an **AWS Application Load Balancer** via the AWS Load Balancer Controller:

- Manifest: `deploy/k8s/gateway-ingress.yaml`
- `ingressClassName: alb`, `scheme: internet-facing`, listen **HTTPS 443**
- ACM certificate ARN annotation (placeholder; replace per account/region)
- Host `ingest.your-startup.com`, path `/ingest` → `finops-gateway-svc:3000` (HTTP backend)

The gateway container continues to serve plain HTTP on port 3000 inside the cluster.

## Rationale

- Keeps the Rust hot path free of TLS handshake and certificate rotation logic.
- ALB + ACM is the standard EKS pattern for internet-facing ingest endpoints.
- In-cluster agents can keep `http://finops-gateway-svc.../ingest`; external callers use `https://ingest.your-startup.com/ingest`.

## Consequences

- **Prerequisite:** AWS Load Balancer Controller installed; ACM cert in the same region as the ALB.
- **Agent config:** External agents set `FINOPS_INGEST_URL=https://ingest.your-startup.com/ingest`.
- **Security:** Bearer token ([ADR 019](019-ingest-bearer-token-auth.md)) still required; TLS protects token on the wire.

## References

- [ADR 025](025-kubernetes-gateway-and-agent.md) — gateway Service and Deployment
- [ADR 019](019-ingest-bearer-token-auth.md) — ingest bearer auth

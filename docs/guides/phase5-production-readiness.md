# Phase 5 — Production-critical security & readiness

**Status:** In progress (current engineering focus)  
**Prerequisites:** Phases 1–3 E2E validated ([phase3-validation.md](phase3-validation.md)); Phase 4 scale/reliability and Phase 6 mechanical sympathy **complete** ([TODO.md](../.cursor/skills/statix-ebpf-agent/TODO.md)).

## Goal

Make the ingest pipeline safe to run on a real network and operable under load before AWS ECS / production billing.

## P0 — Security & data integrity (blockers)

| Item | Why |
|------|-----|
| **Bearer auth on `POST /ingest`** | **Shipped:** set `STATIX_API_TOKEN` on API and agent ([ADR 019](adr/019-ingest-bearer-token-auth.md)). |
| **TLS on `POST /ingest`** | **Shipped:** AWS ALB Ingress (`deploy/k8s/gateway-ingress.yaml`) — HTTPS :443, ACM cert ([ADR 043](adr/043-kubernetes-alb-tls-termination.md)). |
| **BPF ring buffer overflow metric** | **Shipped:** `RING_DROPS` + scrape `http://<node>:9091/metrics` ([ADR 022](adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](adr/023-phase5-hot-path-fixes.md)). |
| **Attribution / ingest hot path** | **Shipped:** procfs before write lock; label cache + `DEFAULT_LABELS`; `expected_bearer` precomputed ([ADR 023](adr/023-phase5-hot-path-fixes.md)). |
| **Schema evolution** | **Shipped:** gateway accepts `schema_version` 2 or 3 ([ADR 020](adr/020-ingest-schema-version-window.md)). |

## P1 — Operational readiness

| Item | Why |
|------|-----|
| **`GET /ready`** | **Shipped:** `ch_healthy` + ingest mpsc &lt; 80% ([ADR 021](../adr/021-ingest-ready-probe.md), [ADR 029](../adr/029-ready-channel-depth-gate.md), [ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)). |
| ~~ClickHouse `kafka_num_consumers`~~ | **Cancelled** — Kafka removed Phase 13 ([ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)). |
| ~~Kafka retention + disk alerts~~ | **Cancelled** — Phase 13. |
| ~~Broken-message alerting~~ | **Cancelled** — no Kafka engine consumer. |
| **ClickHouse insert health** | Monitor `statix_api_ch_*` metrics; `/ready` gates on `ch_healthy`. |

## Deploy artifacts (Targets 1–2)

| Artifact | Path |
|----------|------|
| Gateway image | `deploy/docker/Dockerfile.gateway` |
| Agent image | `deploy/docker/Dockerfile.statix` |
| K8s | `deploy/k8s/gateway.yaml`, `gateway-ingress.yaml`, `statix-daemonset.yaml` |
| ClickHouse | `deploy/clickhouse/01_init.sql` — `statix.workload_metrics FINAL` |

## Local dev

```bash
make compose-up
export STATIX_API_TOKEN=dev-secret-change-me   # same value on API container + agent
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

**Note:** Dev stack may omit `STATIX_API_TOKEN` (auth disabled). Production must set the same token on `statix-gateway` and the agent.

## References

- [enterprise-latency.md](enterprise-latency.md)
- [phase3-ingest-interface.md](phase3-ingest-interface.md)
- [ADR 005](adr/005-non-blocking-ingest-pipeline.md), [ADR 012](adr/012-finops-api-prometheus-metrics.md), [ADR 055](adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)
- [ADR 024](adr/024-agent-production-container.md)–[026](adr/026-clickhouse-finops-database-init.md)

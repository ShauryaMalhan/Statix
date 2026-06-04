# Phase 5 — Production-critical security & readiness

**Status:** In progress (current engineering focus)  
**Prerequisites:** Phases 1–3 E2E validated ([phase3-validation.md](phase3-validation.md)); Phase 4 scale/reliability and Phase 6 mechanical sympathy **complete** ([TODO.md](../.cursor/skills/finops-ebpf-agent/TODO.md)).

## Goal

Make the ingest pipeline safe to run on a real network and operable under load before AWS ECS / production billing.

## P0 — Security & data integrity (blockers)

| Item | Why |
|------|-----|
| **Bearer auth on `POST /ingest`** | **Shipped:** set `FINOPS_API_TOKEN` on API and agent ([ADR 019](adr/019-ingest-bearer-token-auth.md)). |
| **TLS on `POST /ingest`** | Terminate HTTPS at load balancer or sidecar (still required on untrusted networks). |
| **BPF ring buffer overflow metric** | **Shipped:** `RING_DROPS` + scrape `http://<node>:9091/metrics` ([ADR 022](adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](adr/023-phase5-hot-path-fixes.md)). |
| **Attribution / ingest hot path** | **Shipped:** procfs before write lock; label cache + `DEFAULT_LABELS`; `expected_bearer` precomputed ([ADR 023](adr/023-phase5-hot-path-fixes.md)). |
| **Schema evolution** | **Shipped:** gateway accepts `schema_version` 2 or 3 ([ADR 020](adr/020-ingest-schema-version-window.md)). |

## P1 — Operational readiness

| Item | Why |
|------|-----|
| **`GET /ready`** | **Shipped:** `kafka_ready` after broker + partition metadata ([ADR 021](adr/021-ingest-ready-probe.md)). Optional: channel depth &lt; 80% gate. |
| **ClickHouse `kafka_num_consumers`** | Match Kafka partition count in prod ([ADR 008](adr/008-clickhouse-kafka-engine-resilience.md)). |
| **Kafka retention + disk alerts** | Prevent broker fill → throttle → consumer lag. |
| **Broken-message alerting** | `kafka_skip_broken_messages` can hide poison pills; monitor `system.kafka_consumers`. |

## Local dev

```bash
make compose-up
export FINOPS_API_TOKEN=dev-secret-change-me   # same value on API container + agent
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

**Note:** Dev stack may omit `FINOPS_API_TOKEN` (auth disabled). Production must set the same token on `finops-api` and the agent.

## References

- [enterprise-latency.md](enterprise-latency.md)
- [phase3-ingest-interface.md](phase3-ingest-interface.md)
- [ADR 005](adr/005-non-blocking-ingest-pipeline.md), [ADR 012](adr/012-finops-api-prometheus-metrics.md)

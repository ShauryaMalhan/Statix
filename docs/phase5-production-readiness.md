# Phase 5 — Production-critical security & readiness

**Status:** In progress (current engineering focus)  
**Prerequisites:** Phases 1–3 E2E validated ([phase3-validation.md](phase3-validation.md)); Phase 4 scale/reliability and Phase 6 mechanical sympathy **complete** ([TODO.md](../.cursor/skills/finops-ebpf-agent/TODO.md)).

## Goal

Make the ingest pipeline safe to run on a real network and operable under load before AWS ECS / production billing.

## P0 — Security & data integrity (blockers)

| Item | Why |
|------|-----|
| **TLS + auth on `POST /ingest`** | Untrusted networks must not accept arbitrary billing payloads. Planned: `FINOPS_API_TOKEN` (bearer) or mTLS between agent and gateway. |
| **BPF ring buffer overflow metric** | `EVENTS.reserve()` failure is silent today; expose `finops_agent_ring_drops_total` (or equivalent) for capacity planning. |
| **Schema evolution** | Accept `schema_version` current and current−1 during rolling upgrades (today hard-rejects `!= 2`). |

## P1 — Operational readiness

| Item | Why |
|------|-----|
| **`GET /ready`** | Beyond `/health` (`kafka_tx.is_closed()`): channel depth, broker reachability for ALB/K8s. |
| **ClickHouse `kafka_num_consumers`** | Match Kafka partition count in prod ([ADR 008](adr/008-clickhouse-kafka-engine-resilience.md)). |
| **Kafka retention + disk alerts** | Prevent broker fill → throttle → consumer lag. |
| **Broken-message alerting** | `kafka_skip_broken_messages` can hide poison pills; monitor `system.kafka_consumers`. |

## Local dev (until auth ships)

```bash
make compose-up
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

**Note:** Dev stack may run without `FINOPS_API_TOKEN` until ingest auth is implemented; production deploys must not.

## References

- [enterprise-latency.md](enterprise-latency.md)
- [phase3-ingest-interface.md](phase3-ingest-interface.md)
- [ADR 005](adr/005-non-blocking-ingest-pipeline.md), [ADR 012](adr/012-finops-api-prometheus-metrics.md)

# ADR 021: `/ready` probe vs `/health` liveness

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** ALB/K8s must not route ingest traffic until Kafka is reachable and partition clients exist ([Phase 5 TODO](../../.cursor/skills/finops-ebpf-agent/TODO.md)). `/health` only checked `kafka_tx.is_closed()` — true while the producer was still connecting (“load balancer trap”).

## Decision

| Route | Meaning | `200` when |
|-------|---------|------------|
| `GET /health` | **Liveness** | Ingest `mpsc` sender not closed (HTTP + producer task alive) |
| `GET /ready` | **Readiness** | `kafka_ready` (`AtomicBool`, set after `load_partition_clients`) **and** `!kafka_tx.is_closed()` |

`KafkaProducer.is_ready` is set with `Ordering::Release` immediately after successful broker connect + metadata load; `readiness_check` loads with `Ordering::Acquire`.

Channel depth &gt; 80% full fails `/ready` — [ADR 029](029-ready-channel-depth-gate.md).

## Consequences

- **Positive:** Rolling deploys: readiness fails until Kafka is actually wired.
- **Negative:** Brief `503` on `/ready` at cold start until metadata load completes (expected).
- **K8s:** Use `/ready` for readinessProbe, `/health` for livenessProbe.

## References

- `finops-api/src/kafka.rs`, `main.rs`

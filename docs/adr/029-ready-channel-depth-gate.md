# ADR 029: `/ready` ingest mpsc depth gate (80%)

**Status:** Accepted  
**Date:** 2026-06-05  
**Context:** Phase 5 — load balancers should stop sending ingest when the gateway queue is nearly full, before `POST /ingest` returns `503` per row ([ADR 021](021-ingest-ready-probe.md), [ADR 014](014-kafka-producer-env-tuning.md)).

## Decision

- **`AppState.kafka_channel_capacity`** — configured capacity from `FINOPS_KAFKA_CHANNEL_SIZE` (same as `mpsc::channel` in `kafka::spawn_producer`; default **8192**, min **1024** — [ADR 014](014-kafka-producer-env-tuning.md)).
- **`GET /ready`** — after Kafka `kafka_ready` and `!kafka_tx.is_closed()`:
  - `remaining = kafka_tx.capacity()` (tokio mpsc free slots)
  - If more than **80%** full (`remaining * 100 < total * 20`) → `503` + `warn` log with used/remaining counts
  - Else → `200`
- **`GET /health`** unchanged (liveness only; no depth gate).

## Rationale

- Readiness reflects “can accept more ingest load,” not just “Kafka TCP up.”
- Integer percent math avoids float in hot path; threshold constant `READY_CHANNEL_FULL_THRESHOLD_PCT = 80` in `main.rs`.

## Consequences

- **Positive:** K8s/ALB removes pods from rotation before sustained `503` on `/ingest`.
- **Negative:** Bursty ingest may flap `/ready` during spikes (expected; scale gateway or raise `FINOPS_KAFKA_CHANNEL_SIZE`).
- **Code:** `finops-api/src/main.rs`, `kafka.rs` (`ingest_channel_capacity`, `KafkaProducer.channel_capacity`).

## References

- [ADR 012](012-finops-api-prometheus-metrics.md) — `finops_api_kafka_channel_depth` gauge (orthogonal to probe)

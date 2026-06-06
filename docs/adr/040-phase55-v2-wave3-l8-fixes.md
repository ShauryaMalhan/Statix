# ADR 040: Phase 5.5 V2 L8 Wave 3 fixes (durability + K8s eviction)

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** L8 Audit V2 Wave 3 — survive Kafka rebalance drops and coordinated K8s evictions.

## Decision

| ID | Area | Fix |
|----|------|-----|
| V2-11 | `statix-gateway/src/kafka.rs` | `failed_batches: VecDeque<(i32, Vec<Record>)>` — on produce `Err`, queue records; drain before each `produce_grouped_batch` + metadata tick; cap **100** batches then drop + metric |
| V2-5 | `deploy/k8s/statix-daemonset.yaml` | `terminationGracePeriodSeconds: 30`; `preStop` `sleep 5` before SIGTERM flush |
| V2-6 | `deploy/k8s/gateway.yaml` | `PodDisruptionBudget` `minAvailable: 1`; gateway `terminationGracePeriodSeconds: 30` + `preStop` sleep |

## Rationale

- **V2-11:** Rebalance / broker blips must not silently drop micro-batches already accepted via HTTP ingest.
- **V2-5:** preStop delay keeps the pod network up while the agent POSTs the final window (pairs with V2-1 SIGTERM flush).
- **V2-6:** Node drains cannot evict all gateway replicas at once.

## Consequences

- **Metrics:** `statix_api_kafka_produce_errors_total`, `statix_api_kafka_produce_dropped_total` on retry overflow.
- **Memory:** Up to 100 failed record batches retained in gateway process during prolonged broker outage.

## References

- [ADR 038](038-phase55-v2-wave1-l8-fixes.md) — SIGTERM flush
- [ADR 010](010-kafka-partition-key-by-node.md) — Kafka producer
- [ADR 025](025-kubernetes-gateway-and-agent.md) — K8s manifests

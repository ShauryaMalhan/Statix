# ADR 014: Env-tuned Kafka producer backpressure and batching

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Fixed `mpsc` depth (1024), micro-batch size (256), and linger (5ms) caused `503` avalanches under multi-node ingest bursts and under-utilized broker throughput.

## Decision

`finops-api/src/kafka.rs` reads at startup (invalid env → warn + default):

| Variable | Default | Bounds | Role |
|----------|---------|--------|------|
| `STATIX_KAFKA_CHANNEL_SIZE` | 8192 | `.max(1024)` | Ingest `mpsc` capacity before `503` |
| `STATIX_KAFKA_BATCH_MAX` | 1024 | `64..=16384` | Max rows per channel micro-batch / produce chunk |
| `STATIX_KAFKA_LINGER_MS` | 50 | `1..=1000` | Partial-batch flush wait |

Removed public `CHANNEL_SIZE` and internal `BATCH_*` consts.

## Rationale

- **Channel:** Thousands of nodes flushing 10s windows need headroom; 1024 fills in seconds → cascading agent retries.
- **Batch max:** Larger batches amortize `produce()` round-trips per partition ([ADR 010](010-kafka-partition-key-by-node.md)).
- **Linger:** Slightly higher default coalesces sparse rows without blocking HTTP handlers.

## Consequences

- **Positive:** Tunable per environment (local vs ECS) without recompile.
- **Negative:** Mis-set huge `CHANNEL_SIZE` increases API RAM; cap discipline is operator responsibility (min 1024 only).
- **Ops:** Set in `docker-compose.yml` / DaemonSet env for prod burst tests.

## References

- `finops-api/src/kafka.rs`, [ADR 005](005-non-blocking-ingest-pipeline.md), [ADR 012](012-finops-api-prometheus-metrics.md) (`statix_api_kafka_channel_full_total`)

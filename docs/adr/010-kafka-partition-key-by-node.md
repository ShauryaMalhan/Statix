# ADR 010: Kafka partition routing by `node` message key

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** `finops-api` produced all rows to partition `0`, capping throughput to one broker partition and one effective ClickHouse Kafka consumer thread per topic. Multi-node ECS deploys need keyed routing so each agent node's windows stay ordered on one partition while load spreads across partitions.

## Decision

1. **Channel:** `mpsc::Sender<(Vec<u8>, Vec<u8>)>` — Kafka key + JSONEachRow payload. Ingest builds `node` key **once** per HTTP batch (`as_bytes().to_vec()`); per-row `node_vec.clone()`; producer moves `Vec` into `Record` with no `Bytes::to_vec()` memcpy (amended 2026-06-01).
2. **Broker metadata:** On producer startup, `Client::list_topics()` resolves partition IDs for `finops-telemetry` (fallback `[0]` if topic not yet auto-created).
3. **Partition clients:** One `PartitionClient` per partition ID (no hardcoded `partition_client(..., 0)` only).
4. **Routing:** `DefaultHasher` over `node` UTF-8 bytes (`&[u8]`) `% num_partitions` → partition slot; record **key** / **value** own the queued `Vec<u8>` buffers at produce time.
5. **Micro-batch:** `FINOPS_KAFKA_BATCH_MAX` / `FINOPS_KAFKA_LINGER_MS` ([ADR 014](014-kafka-producer-env-tuning.md)); after each channel batch, **group by partition** and `produce()` per partition sub-batch.

## Consequences

- **Local Docker:** Auto-created topic still has 1 partition → identical behavior to partition 0.
- **Production:** Increase topic partition count and set `kafka_num_consumers` in ClickHouse to match ([ADR 008](008-clickhouse-kafka-engine-resilience.md)).
- **Ordering:** Rows for the same `node` land on one partition → per-node chronological order preserved.
- **Not in scope:** Murmur2-compatible broker default partitioner (we pick partition explicitly); HTTP 400/503/200 unchanged.

## References

- `finops-api/src/kafka.rs`, `routes/ingest.rs`, `main.rs`
- Phase 4 TODO 1.1 (shipped)

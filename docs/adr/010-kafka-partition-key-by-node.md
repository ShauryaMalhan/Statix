# ADR 010: Kafka partition routing by `node` message key

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** `finops-api` produced all rows to partition `0`, capping throughput to one broker partition and one effective ClickHouse Kafka consumer thread per topic. Multi-node ECS deploys need keyed routing so each agent node's windows stay ordered on one partition while load spreads across partitions.

## Decision

1. **Channel:** `mpsc::Sender<(bytes::Bytes, bytes::Bytes)>` — Kafka key + JSONEachRow payload. Ingest converts `batch.node` to `Bytes` **once** per HTTP batch; per-row `Bytes::clone` on the key (refcount, not O(N) `String` clones).
2. **Broker metadata:** On producer startup, `Client::list_topics()` resolves partition IDs for `finops-telemetry` (fallback `[0]` if topic not yet auto-created).
3. **Partition clients:** One `PartitionClient` per partition ID (no hardcoded `partition_client(..., 0)` only).
4. **Routing:** `DefaultHasher` over `node` UTF-8 bytes (`&[u8]`) `% num_partitions` → partition slot; record **key** = `node.to_vec()` at produce time.
5. **Micro-batch:** Keep `BATCH_MAX_RECORDS` (256) and `BATCH_LINGER` (5ms); after each channel batch, **group by partition** and `produce()` per partition sub-batch.

## Consequences

- **Local Docker:** Auto-created topic still has 1 partition → identical behavior to partition 0.
- **Production:** Increase topic partition count and set `kafka_num_consumers` in ClickHouse to match ([ADR 008](008-clickhouse-kafka-engine-resilience.md)).
- **Ordering:** Rows for the same `node` land on one partition → per-node chronological order preserved.
- **Not in scope:** Murmur2-compatible broker default partitioner (we pick partition explicitly); HTTP 400/503/200 unchanged.

## References

- `finops-api/src/kafka.rs`, `routes/ingest.rs`, `main.rs`
- Phase 4 TODO 1.1 (shipped)

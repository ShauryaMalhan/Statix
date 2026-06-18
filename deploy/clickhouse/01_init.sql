-- FinOps telemetry: gateway RowBinary → ReplacingMergeTree (single source of truth)
-- Wire: POST /ingest JSON batch → gateway coalescer → RowBinary INSERT
--
-- Billing: SELECT * FROM statix.workload_metrics FINAL WHERE node = '...';
--
-- Schema change on existing volume: docker compose down -v && make compose-up

CREATE DATABASE IF NOT EXISTS statix;

CREATE TABLE IF NOT EXISTS statix.workload_metrics
(
    window_start_ns UInt64,
    window_end_ns UInt64,
    node LowCardinality(String),
    batch_id String,
    agent_version LowCardinality(String),
    cgroup_id UInt64,
    namespace LowCardinality(Nullable(String)),
    -- High-cardinality K8s fields: plain String (LowCardinality OOM risk at scale — ADR 007)
    pod Nullable(String),
    container Nullable(String),
    k8s_resolved Bool,
    memory_bytes_max UInt64,
    memory_bytes_last UInt64,
    exec_count UInt32,
    sample_count UInt32,
    cpu_usage_usec UInt64
)
ENGINE = ReplacingMergeTree(window_end_ns)-- Hour-aligned partitions: reduces midnight UTC boundary merge storms (V3-11 / ADR 051)
PARTITION BY toStartOfHour(toDateTime(intDiv(window_start_ns, 1000000000)))
ORDER BY (node, window_start_ns, cgroup_id)
TTL toDateTime(intDiv(window_start_ns, 1000000000)) + INTERVAL 30 DAY;

-- Phase 13: Kafka removed — gateway inserts RowBinary batches directly.
-- Drop the consumer (MV) before the source (Kafka table) so no rows are read mid-teardown.
DROP VIEW  IF EXISTS statix.telemetry_mv          SYNC;
DROP TABLE IF EXISTS statix.kafka_telemetry_queue SYNC;

-- statix.workload_metrics (defined above) is UNCHANGED:
--   ReplacingMergeTree(window_end_ns) natively absorbs batched HTTP inserts;
--   ReplacingMergeTree + FINAL dedups at-least-once WAL replays.
-- Do NOT add async_insert: the synchronous insert ACK is the stall-detection primitive.
--
-- Existing volume (Phase 14): ALTER TABLE statix.workload_metrics
--   ADD COLUMN IF NOT EXISTS cpu_usage_usec UInt64 DEFAULT 0 AFTER sample_count;

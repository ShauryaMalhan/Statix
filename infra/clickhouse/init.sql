-- Phase 3/4: Kafka → ClickHouse pipeline (no Rust consumer)
-- Schema changes on an existing volume require: docker compose down -v && make compose-up
--
-- Billing queries on finops_telemetry must use FINAL (ReplacingMergeTree merges asynchronously):
--   SELECT * FROM finops_telemetry FINAL WHERE node = 'node-1';

CREATE TABLE IF NOT EXISTS finops_telemetry_kafka (
    window_start_ns UInt64,
    window_end_ns UInt64,
    node String,
    cgroup_id UInt64,
    namespace Nullable(String),
    pod Nullable(String),
    container Nullable(String),
    k8s_resolved Bool,
    memory_bytes_max UInt64,
    memory_bytes_last UInt64,
    exec_count UInt32,
    sample_count UInt32
) ENGINE = Kafka
SETTINGS
    kafka_broker_list = 'kafka:29092',
    kafka_topic_list = 'finops-telemetry',
    kafka_group_name = 'clickhouse-consumer',
    kafka_format = 'JSONEachRow',
    -- Drop malformed JSON rows instead of halting the consumer (poison pill)
    kafka_skip_broken_messages = 1000,
    -- Match Kafka partition count in production (1 for local docker)
    kafka_num_consumers = 1;

CREATE TABLE IF NOT EXISTS finops_telemetry (
    window_start_ns UInt64,
    window_end_ns UInt64,
    node LowCardinality(String),
    cgroup_id UInt64,
    namespace LowCardinality(Nullable(String)),
    -- High-cardinality K8s fields: plain String — LowCardinality OOMs on millions of unique pods/IDs
    pod Nullable(String),
    container Nullable(String),
    k8s_resolved Bool,
    memory_bytes_max UInt64,
    memory_bytes_last UInt64,
    exec_count UInt32,
    sample_count UInt32
) ENGINE = ReplacingMergeTree()
PARTITION BY toYYYYMMDD(toDateTime(intDiv(window_start_ns, 1000000000)))
-- Identity for dedupe: (node, window_start_ns, cgroup_id) only — K8s labels are mutable metadata
-- (see ADR 011). Do not include namespace in ORDER BY or retries double-bill when labels arrive late.
ORDER BY (node, window_start_ns, cgroup_id)
TTL toDateTime(intDiv(window_start_ns, 1000000000)) + INTERVAL 30 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS finops_mv
TO finops_telemetry AS
SELECT * FROM finops_telemetry_kafka;

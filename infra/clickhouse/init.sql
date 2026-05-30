-- Phase 3: Kafka → ClickHouse pipeline (no Rust consumer)
-- Schema changes on an existing volume require: docker compose down -v && make compose-up

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
    pod LowCardinality(Nullable(String)),
    container LowCardinality(Nullable(String)),
    k8s_resolved Bool,
    memory_bytes_max UInt64,
    memory_bytes_last UInt64,
    exec_count UInt32,
    sample_count UInt32
) ENGINE = MergeTree()
-- Daily parts: smaller merges under burst ingest; aligns with TTL drops
PARTITION BY toYYYYMMDD(toDateTime(intDiv(window_start_ns, 1000000000)))
-- FinOps billing filters: namespace → pod → node → time
ORDER BY (namespace, pod, node, window_start_ns)
-- Dev/docker disk cap (tune for production retention policy)
TTL toDateTime(intDiv(window_start_ns, 1000000000)) + INTERVAL 30 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS finops_mv
TO finops_telemetry AS
SELECT * FROM finops_telemetry_kafka;

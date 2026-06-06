-- FinOps telemetry: Kafka engine → ReplacingMergeTree (single source of truth)
-- Wire: JSONEachRow on topic `finops-telemetry` (finops_wire::FlatRow)
--
-- Billing: SELECT * FROM finops.workload_metrics FINAL WHERE node = '...';
--
-- Schema change on existing volume: docker compose down -v && make compose-up
--
-- Broker overrides: Compose finops-net = kafka:29092; K8s = kafka-broker.default.svc.cluster.local:9092

CREATE DATABASE IF NOT EXISTS finops;

CREATE TABLE IF NOT EXISTS finops.workload_metrics
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
    sample_count UInt32
)
ENGINE = ReplacingMergeTree(window_end_ns)
PARTITION BY toYYYYMMDD(toDateTime(intDiv(window_start_ns, 1000000000)))
ORDER BY (node, window_start_ns, cgroup_id)
TTL toDateTime(intDiv(window_start_ns, 1000000000)) + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS finops.kafka_telemetry_queue
(
    window_start_ns UInt64,
    window_end_ns UInt64,
    node LowCardinality(String),
    batch_id String,
    agent_version LowCardinality(String),
    cgroup_id UInt64,
    namespace LowCardinality(Nullable(String)),
    pod Nullable(String),
    container Nullable(String),
    k8s_resolved Bool,
    memory_bytes_max UInt64,
    memory_bytes_last UInt64,
    exec_count UInt32,
    sample_count UInt32
)
ENGINE = Kafka
SETTINGS
    kafka_broker_list = 'kafka:29092',
    kafka_topic_list = 'finops-telemetry',
    kafka_group_name = 'clickhouse-ingest-group',
    kafka_format = 'JSONEachRow',
    kafka_skip_broken_messages = 1000,
    kafka_num_consumers = 1;

CREATE MATERIALIZED VIEW IF NOT EXISTS finops.telemetry_mv
TO finops.workload_metrics AS
SELECT * FROM finops.kafka_telemetry_queue;

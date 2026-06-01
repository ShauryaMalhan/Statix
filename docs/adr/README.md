# Architecture Decision Records (ADRs)

Point-in-time notes on **why** we chose something—not polished docs. When code changes, add a new numbered file; don't rewrite history.

**Workflow:** Any architectural change must add or update an ADR and sync [enterprise-latency.md](../enterprise-latency.md) + `.cursor/skills/finops-ebpf-agent/`.

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-use-rustc-hash-for-latency.md) | Use `rustc-hash` (`FxHashMap`) in the aggregator | Accepted |
| [002](002-double-buffer-aggregator.md) | Double-buffered aggregator maps | Accepted |
| [003](003-early-flush-instead-of-cap-eviction.md) | Early flush instead of random key eviction | Accepted |
| [004](004-swap-buffer-before-drain.md) | Flip active buffer before draining on flush | Accepted |
| [005](005-non-blocking-ingest-pipeline.md) | HTTP → mpsc → Kafka; ClickHouse Kafka engine | Accepted |
| [006](006-shared-http-client-for-ingest.md) | Shared `reqwest::Client` + ingest retry worker | Accepted |
| [007](007-clickhouse-mergetree-tuning.md) | Storage layout: partitions, sort key, TTL (see 011) | Accepted |
| [008](008-clickhouse-kafka-engine-resilience.md) | Kafka engine: skip broken messages, `kafka_num_consumers` | Accepted |
| [009](009-finops-api-docker-compose.md) | `finops-api` in Docker Compose (`Dockerfile.api`) | Accepted |
| [010](010-kafka-partition-key-by-node.md) | Kafka partition routing by `node` message key | Accepted |
| [011](011-replacingmergetree-dedupe-identity.md) | ReplacingMergeTree; ORDER BY without `namespace`; `FINAL` reads | Accepted |
| [012](012-finops-api-prometheus-metrics.md) | `GET /metrics`; ingest/Kafka Prometheus instrumentation | Accepted |
| [013](013-configurable-ring-buffer-size.md) | Ring buffer build-time tiers + CPU-based ELF pick | Accepted |
| [014](014-kafka-producer-env-tuning.md) | Kafka mpsc / batch / linger env tuning | Accepted |
| [015](015-cgroup-v2-bootstrap-on-startup.md) | Walk cgroup v2 on startup; inode = cgroup_id | Accepted |
| [016](016-clock-domain-offset.md) | BPF monotonic → wall offset for aggregator windows | Accepted |
| [017](017-batch-lineage-metadata.md) | `batch_id` + `agent_version` on every flush | Accepted |
| [018](018-phase-roadmap-status.md) | Phases 4 & 6 complete; Phase 5 security focus | Accepted |

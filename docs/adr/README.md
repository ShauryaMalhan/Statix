# Architecture Decision Records (ADRs)

Point-in-time notes on **why** we chose something—not polished docs. When code changes, add a new numbered file; don't rewrite history.

<<<<<<< HEAD
=======
**Workflow:** Any architectural change must add or update an ADR and sync [enterprise-latency.md](../enterprise-latency.md) + `.cursor/skills/finops-ebpf-agent/`.

>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
| ADR | Title | Status |
|-----|-------|--------|
| [001](001-use-rustc-hash-for-latency.md) | Use `rustc-hash` (`FxHashMap`) in the aggregator | Accepted |
| [002](002-double-buffer-aggregator.md) | Double-buffered aggregator maps | Accepted |
| [003](003-early-flush-instead-of-cap-eviction.md) | Early flush instead of random key eviction | Accepted |
| [004](004-swap-buffer-before-drain.md) | Flip active buffer before draining on flush | Accepted |
<<<<<<< HEAD
=======
| [005](005-non-blocking-ingest-pipeline.md) | HTTP → mpsc → Kafka; ClickHouse Kafka engine | Accepted |
| [006](006-shared-http-client-for-ingest.md) | Shared `reqwest::Client` for ingest POST | Accepted |
| [007](007-clickhouse-mergetree-tuning.md) | MergeTree daily parts, billing sort key, 30d TTL | Accepted |
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

# ADR 012: Prometheus metrics on `finops-api`

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 4 observability — monitor ingest backpressure, HTTP latency, and Kafka producer health without middleware on the hot path.

## Decision

`finops-api` uses `metrics` + `metrics-exporter-prometheus`:

- **Recorder:** `PrometheusBuilder::new().install_recorder()` at startup (global macros); **not** `install()` (that binds a separate HTTP listener).
- **Scrape:** `GET /metrics` returns `PrometheusHandle::render()` with `text/plain; version=0.0.4` body.
- **Upkeep:** Background task calls `run_upkeep()` every 60s (required when using `install_recorder`).

| Metric | Type | Where |
|--------|------|--------|
| `statix_api_ingest_requests_total{status}` | counter | `ingest.rs` — per HTTP status (`200`, `400`, `503`, …) |
| `statix_api_ingest_duration_seconds` | histogram | `ingest.rs` — handler wall time |
| `statix_api_kafka_channel_full_total` | counter | `ingest.rs` — `try_send` full |
| `statix_api_kafka_channel_depth` | gauge | `ingest.rs` +1 on enqueue; `kafka.rs` −N on `recv` / `recv_many` |
| `statix_api_kafka_produce_duration_seconds` | histogram | `kafka.rs` — around `PartitionClient::produce` |

Instrumentation is explicit in handlers/loops — no Axum metrics middleware.

## Consequences

- **Positive:** Backpressure and producer latency visible to Prometheus/Grafana.
- **Negative:** Channel depth is a gauge proxy (not `mpsc` capacity); can drift if process panics mid-queue.
- **Deferred:** Histogram bucket tuning via `PrometheusBuilder::set_buckets_for_metric`; RED dashboards in runbooks.

## References

- `finops-api/src/main.rs`, `routes/ingest.rs`, `kafka.rs`

# Phase 13 — Part 1 Playbook: Kafka Removal → Direct ClickHouse Ingest

> **Audience:** the Cursor execution engine.
> **Status:** Part 1 **shipped** ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)). Part 2 ingest **shipped** ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)); infra strip open — see [PHASE_13_PART2_PLAYBOOK.md](PHASE_13_PART2_PLAYBOOK.md) and [TODO.md](TODO.md).

## Topology (current)

```
agent → POST /ingest → gateway mpsc(bounded coalescer) → CH insert worker (RowBinary)
        → INSERT INTO statix.workload_metrics
```

Backpressure: `ch_healthy` + mpsc 80% gate → `503` → agent circuit breaker → WAL ([ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)).

---

## Shipped ✅ (Part 1)

| Task | ADR | Items |
|------|-----|-------|
| Part 1 ✅ | [055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md) | Schema drop Kafka MV/table; `clickhouse_writer.rs`; `AppState` + 3-tier 503; delete `kafka.rs` / `rskafka` |

---

## Verification (run after deploy)

- `make check` — workspace + nightly BPF check compiles cleanly with `rskafka` gone.
- `cargo test -p statix-gateway` — readiness/threshold unit tests pass.
- `docker compose down -v && make compose-up` — fresh ClickHouse init: `workload_metrics` only (no Kafka table/MV).
- End-to-end: `sudo -E make run` with `STATIX_INGEST_URL`; rows in `statix.workload_metrics`; `curl :3000/ready` → 200.
- **Backpressure drill:** pause ClickHouse → within `STATIX_CH_INSERT_TIMEOUT_SECS` (3s), `POST /ingest` → 503, `/ready` → 503; agent circuit Open + `statix_wal_frames_written_total` rises; unpause → WAL drains.

## Part 2 — Infra strip (NOT shipped)

- [ ] Remove Kafka/Zookeeper from `docker-compose.yml` and K8s manifests (`deploy/k8s/gateway.yaml` still sets `KAFKA_BROKERS`).
- [x] Ingest zero-alloc collapse — [PHASE_13_PART2_PLAYBOOK.md](PHASE_13_PART2_PLAYBOOK.md) ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).
- [x] Update `README.md`, `docs/guides/enterprise-latency.md`, `run_script.md`, skill files.
- [x] Document env: `STATIX_CH_BATCH_MAX`, `STATIX_CH_LINGER_MS`, `STATIX_CH_INSERT_TIMEOUT_SECS`, `STATIX_INGEST_CHANNEL_SIZE`.

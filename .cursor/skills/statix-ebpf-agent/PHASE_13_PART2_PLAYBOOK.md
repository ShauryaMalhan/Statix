# Phase 13 — Part 2 Playbook: Ingest Zero-Alloc Collapse (single `MetricRow`)

> **Audience:** the Cursor execution engine.
> **Status:** **Shipped** ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)). Compose/K8s Kafka strip remains open — see [TODO.md](TODO.md).
> **Scope:** ingest hot path only — `statix-gateway/src/routes/ingest.rs`,
> `clickhouse_writer.rs`, `main.rs`, `statix-wire/src/lib.rs`. The compose/K8s Kafka
> infra strip is a **separate** Part 2 item — do not touch it here.

## Topology (unchanged — coalescer retained)

```
agent → POST /ingest (JSON IngestBatch) → handler denormalizes
        → mpsc(bounded coalescer) → CH insert worker (RowBinary)
        → INSERT INTO statix.workload_metrics
```

**Decision locked:** the mpsc stays a **cross-request coalescer** ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).
Do **not** replace it with a per-request `clickhouse::inserter`.

---

## Shipped ✅ (ingest zero-alloc)

| Task | ADR | Items |
|------|-----|-------|
| Part 2 ingest ✅ | [056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md) | `MetricRow::from_ingest`; channel typed `MetricRow`; drop `FlatRow` from `statix-wire`; no transient `Vec` in handler; no flush-time conversion |

---

## Verification (run after deploy)

- `make check` — workspace compiles; `grep -rn FlatRow` returns nothing in source (historical ADRs/skills may mention it).
- `cargo test -p statix-gateway` — readiness/threshold unit tests pass.
- `cargo test -p statix-wire` — wire crate builds after `FlatRow` removal.
- End-to-end: rows land in `statix.workload_metrics`; duplicate/WAL replay collapses under `SELECT … FINAL`.
- **Backpressure drill (unchanged):** pause ClickHouse → `POST /ingest` / `/ready` → 503 within `STATIX_CH_INSERT_TIMEOUT_SECS`.

## Part 2 — Infra strip (NOT shipped)

`docker-compose.yml` still defines `kafka`, `kafka-ui`, and stale `KAFKA_BROKERS` / `STATIX_KAFKA_*` on `statix-gateway`. Gateway **Rust code** ignores these — queue-less path is live ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md), [056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).

- [ ] Remove Kafka services from `docker-compose.yml`; drop `kafka-data` volume; fix `clickhouse` / gateway `depends_on`.
- [ ] Remove `KAFKA_BROKERS` from `deploy/k8s/gateway.yaml`; add `STATIX_INGEST_CHANNEL_SIZE`, `STATIX_CH_*`.
- [ ] Update `deploy/docker/README.md`, `deploy/k8s/README.md`, `deploy/clickhouse/README.md`.

## Deferred stretch (optional)

`Arc<str>` for envelope fields on `MetricRow` — verify `clickhouse` 0.13 RowBinary path first ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).

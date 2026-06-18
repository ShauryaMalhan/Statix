# Phase 13 — Part 2 Playbook: Ingest Zero-Alloc + Infra Strip

> **Audience:** the Cursor execution engine.
> **Status:** **Shipped** ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md), [ADR 057](../../../docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md)).

## Topology (current)

```
agent → POST /ingest (JSON IngestBatch) → handler → mpsc(coalescer) → RowBinary → statix.workload_metrics
```

**Decision locked:** the mpsc stays a **cross-request coalescer** ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).

---

## Shipped ✅

| Task | ADR | Items |
|------|-----|-------|
| Part 2 ingest ✅ | [056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md) | `MetricRow::from_ingest`; channel typed `MetricRow`; drop `FlatRow` from `statix-wire` |
| Part 2 infra ✅ | [057](../../../docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md) | Compose/K8s Kafka strip; deploy READMEs; `STATIX_CH_*` env on gateway |

---

## Verification

- `docker compose config` — valid YAML; no `kafka` service.
- `make compose-up` — ClickHouse + Grafana + gateway start; `/ready` → 200.
- `grep -ri kafka deploy/ docker-compose.yml` — no operational Kafka references (historical ADRs excluded).

## Deferred stretch (optional)

`Arc<str>` for envelope fields on `MetricRow` — verify `clickhouse` 0.13 RowBinary path first ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).

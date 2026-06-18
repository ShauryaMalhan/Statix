# ADR 057: Phase 13 Part 2 — Strip Kafka from compose and K8s manifests

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** Gateway code removed Kafka in Part 1 ([ADR 055](055-phase13-part1-kafka-removal-rowbinary.md)); ingest zero-alloc shipped in Part 2 ([ADR 056](056-phase13-part2-ingest-zero-alloc.md)). `docker-compose.yml`, K8s gateway Deployment, and deploy READMEs still referenced dead Kafka services and env vars.

## Decision

### `docker-compose.yml`

- Delete `kafka` and `kafka-ui` services; remove `kafka-data` volume.
- Remove `KAFKA_BROKERS`, `STATIX_KAFKA_CHANNEL_SIZE`, `STATIX_KAFKA_BATCH_MAX`, `STATIX_KAFKA_LINGER_MS` from `statix-gateway`.
- Add gateway writer env: `STATIX_INGEST_CHANNEL_SIZE`, `STATIX_CH_BATCH_MAX`, `STATIX_CH_LINGER_MS`, `STATIX_CH_INSERT_TIMEOUT_SECS`.
- `clickhouse` and `statix-gateway` depend on ClickHouse health only (no Kafka `depends_on`).

### `deploy/k8s/gateway.yaml`

- Remove `KAFKA_BROKERS`.
- Add `STATIX_INGEST_CHANNEL_SIZE`, `STATIX_CH_*` writer tuning env (match compose defaults).

### Deploy documentation

- `deploy/docker/README.md` — gateway run example uses `CLICKHOUSE_*`; no Kafka TLS note.
- `deploy/k8s/README.md` — direct agent → gateway → ClickHouse architecture; no broker note.
- `deploy/clickhouse/README.md` — `workload_metrics` only; RowBinary ingest path; legacy Kafka engine objects documented as dropped.

## Rationale

Infra must match the queue-less application topology. Stale Kafka services waste resources, confuse operators, and imply env vars the gateway no longer reads.

## Consequences

- **Positive:** Compose stack is ClickHouse + Grafana + gateway only; faster startup; docs match code.
- **Negative:** Operators with external Kafka-based tooling must migrate to gateway HTTP ingest (already the live path since ADR 055).
- **Neutral:** Historical ADRs 005/008/010/014 remain as accepted history; not rewritten.

## References

- [ADR 055](055-phase13-part1-kafka-removal-rowbinary.md) — gateway RowBinary ingest
- [ADR 056](056-phase13-part2-ingest-zero-alloc.md) — single `MetricRow` hot path
- [PHASE_13_PART2_PLAYBOOK.md](../../.cursor/skills/statix-ebpf-agent/PHASE_13_PART2_PLAYBOOK.md)

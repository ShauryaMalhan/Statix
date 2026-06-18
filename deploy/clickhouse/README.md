# ClickHouse — single init script

**Canonical file:** `01_init.sql` — used by docker-compose, K8s ClickHouse, and manual bootstrap.

Creates:

| Object | Purpose |
|--------|---------|
| `statix.workload_metrics` | `ReplacingMergeTree` storage (dedupe key: `node`, `window_start_ns`, `cgroup_id`); minmax skip index on `cgroup_id` ([ADR 059](../../docs/adr/phase10/059-phase10-clickhouse-cgroup-skip-index.md)) |

Phase 13 removed the Kafka engine table and materialized view. The script includes `DROP` statements for legacy `kafka_telemetry_queue` / `telemetry_mv` on existing volumes ([ADR 055](../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)).

## Ingest path

```
statix-gateway → RowBinary INSERT → statix.workload_metrics
```

No Kafka broker or ClickHouse Kafka engine required.

## Apply

```bash
set -a && source .env && set +a
clickhouse-client --user default --password "$CLICKHOUSE_PASSWORD" --multiquery < deploy/clickhouse/01_init.sql
```

Compose applies automatically on **first** ClickHouse volume init (`make compose-up`).

After schema changes: `docker compose down -v && make compose-up`.

## Verify

```bash
curl -s -u "default:${CLICKHOUSE_PASSWORD}" \
  'http://localhost:8123/?query=SHOW%20TABLES%20FROM%20statix'
# Expect: workload_metrics

curl -s -u "default:${CLICKHOUSE_PASSWORD}" \
  'http://localhost:8123/?query=SELECT%20count()%20FROM%20statix.workload_metrics%20FINAL'
```

## Production

- Billing always uses `FINAL` ([ADR 011](../../docs/adr/011-replacingmergetree-dedupe-identity.md))
- Gateway micro-batch coalescer avoids small parts ([ADR 055](../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md))

See [ADR 026](../../docs/adr/026-clickhouse-finops-database-init.md).

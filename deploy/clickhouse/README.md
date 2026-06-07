# ClickHouse — single init script

**Canonical file:** `01_init.sql` — used by docker-compose, K8s ClickHouse, and manual bootstrap.

Creates:

| Object | Purpose |
|--------|---------|
| `statix.workload_metrics` | `ReplacingMergeTree` storage (dedupe key: `node`, `window_start_ns`, `cgroup_id`) |
| `statix.kafka_telemetry_queue` | Kafka engine consumer |
| `statix.telemetry_mv` | MV into `workload_metrics` |

## Apply

```bash
set -a && source .env && set +a
clickhouse-client --user default --password "$CLICKHOUSE_PASSWORD" --multiquery < deploy/clickhouse/01_init.sql
```

Compose applies automatically on **first** ClickHouse volume init (`make compose-up`).

After schema changes: `docker compose down -v && make compose-up`.

## Kafka broker

| Environment | `kafka_broker_list` in script |
|-------------|-------------------------------|
| docker-compose (`statix-net`) | `kafka:29092` |
| Host ClickHouse → host Kafka | `localhost:9092` (ALTER TABLE … MODIFY SETTING) |
| K8s | `kafka-broker.default.svc.cluster.local:9092` |

## Verify

```bash
curl -s -u "default:${CLICKHOUSE_PASSWORD}" \
  'http://localhost:8123/?query=SELECT%20count()%20FROM%20statix.workload_metrics%20FINAL'
```

## Production

- `kafka_num_consumers` = topic partition count ([ADR 008](../../docs/adr/008-clickhouse-kafka-engine-resilience.md))
- Billing always uses `FINAL` ([ADR 011](../../docs/adr/011-replacingmergetree-dedupe-identity.md))

See [ADR 026](../../docs/adr/026-clickhouse-finops-database-init.md).

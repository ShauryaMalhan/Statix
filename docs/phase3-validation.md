# Phase 3 validation guide

## Prerequisites

- Phase 2 prerequisites ([phase2-validation.md](phase2-validation.md))
- Docker: `docker.io` + `docker-compose-v2` (`make compose-up` checks for `docker`)
- `make build` includes `finops-api` and `finops-user`

## Stack smoke test

```bash
cd finops-core
make compose-up
docker compose ps   # kafka, kafka-ui, clickhouse, finops-api running
# API in compose listens :3000 — or use `make run-api` on host (not both)
```

Expect: `make compose-up` prints `http://127.0.0.1:3000/health (OK)`.

## Agent → API → Kafka → ClickHouse

```bash
# terminal 2 (root for BPF)
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

Trigger workload activity (`ls /tmp`, pod exec, etc.). Wait one flush window.

### Pass criteria

| Test | Pass |
|------|------|
| API liveness | `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/health` → `200` |
| API ingest | `curl -s -o /dev/null -w "%{http_code}" -X POST http://127.0.0.1:3000/ingest -H 'Content-Type: application/json' -d '{"schema_version":2,"window_start_ns":0,"window_end_ns":1,"node":"test","workloads":[]}'` → `200` |
| Kafka topic | Kafka UI `:8080` shows topic `finops-telemetry` with messages |
| ClickHouse rows | `curl -s -u default:finops_dev 'http://localhost:8123/?query=SELECT%20count()%20FROM%20finops_telemetry'` → count &gt; 0 after traffic |
| Agent non-blocking | Ring buffer loop responsive under load; ingest transport errors only `log::warn` |
| Backpressure signal | Saturate channel (load test) → `POST /ingest` returns `503` with plain-text body (not `200`) |
| Schema gate | `schema_version: 1` in POST body → `400` (not `200`) |
| Stdout fallback | Unset `FINOPS_INGEST_URL` → batched JSON on stdout (Phase 2 behavior) |

## ClickHouse schema check

```bash
curl -s -u default:finops_dev 'http://localhost:8123/?query=SHOW%20TABLES'
# finops_telemetry_kafka, finops_telemetry, finops_mv
```

If tables are missing or schema changed (partition / ORDER BY / TTL), reset the volume:

```bash
docker compose down -v && make compose-up
```

See [ADR 007](adr/007-clickhouse-mergetree-tuning.md) and [ADR 008](adr/008-clickhouse-kafka-engine-resilience.md).

```bash
curl -s -u default:finops_dev "http://localhost:8123/?query=SHOW%20CREATE%20TABLE%20finops_telemetry_kafka" | grep -E 'skip_broken|num_consumers'
# Expect kafka_skip_broken_messages = 1000 and kafka_num_consumers = 1 (local)
```

## Local ports

| Service | Port |
|---------|------|
| Kafka (host) | 9092 |
| Kafka UI | 8080 |
| ClickHouse HTTP | 8123 |
| finops-api | 3000 |

## Enterprise checks

See [enterprise-latency.md](enterprise-latency.md): no handler `await` on Kafka; agent uses shared `reqwest` client (3s timeout) + `tokio::spawn`.

Tear down: `make compose-down`. Rebuild API image: `docker compose build finops-api && docker compose up -d finops-api` ([ADR 009](adr/009-finops-api-docker-compose.md)).

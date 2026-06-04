# Phase 3 validation guide

## Prerequisites

- Phase 2 prerequisites ([phase2-validation.md](phase2-validation.md))
- Docker: `docker.io` + `docker-compose-v2` (`make compose-up` checks for `docker`)
- `make build` includes `finops-api`, `finops-agent`, and `finops-wire`

## Stack smoke test

```bash
cd finops-core
make compose-up
docker compose ps   # kafka, kafka-ui, clickhouse, grafana, finops-api running
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
| API readiness | `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/ready` → `200` after Kafka metadata load (may `503` for first seconds) |
| Prometheus (API) | `curl -s http://127.0.0.1:3000/metrics \| grep finops_api_` → lines present (404 = rebuild API image) |
| Prometheus (agent) | With agent running: `curl -s http://127.0.0.1:9091/metrics \| grep finops_agent_ring_drops` → metric registered ([ADR 023](adr/023-phase5-hot-path-fixes.md)) |
| API ingest | `curl ... -X POST http://127.0.0.1:3000/ingest` (no auth when `FINOPS_API_TOKEN` unset) → `200` |
| Ingest auth | With `FINOPS_API_TOKEN` set on API: missing header → `401`; `curl -H 'Authorization: Bearer <token>' ...` → `200` ([ADR 019](adr/019-ingest-bearer-token-auth.md)) |
| Kafka topic | Kafka UI `:8080` shows topic `finops-telemetry` with messages |
| ClickHouse rows | `curl -s -u default:finops_dev 'http://localhost:8123/?query=SELECT%20count()%20FROM%20finops.workload_metrics%20FINAL'` → count &gt; 0 after traffic |
| Read API | `curl -s 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=24'` → `200` + JSON array (empty `[]` OK before traffic; `500` = check `CLICKHOUSE_*` / rebuild API) ([ADR 027](adr/027-api-read-path-clickhouse.md)) |
| Agent non-blocking | Ring buffer loop responsive under load; ingest retries log `warn` on backoff |
| Backpressure signal | Saturate channel (load test) → `POST /ingest` returns `503` with plain-text body (not `200`) |
| Schema gate | `schema_version: 1` or `4` → `400`; `2` or `3` → `200` ([ADR 020](adr/020-ingest-schema-version-window.md)) |
| Stdout fallback | Unset `FINOPS_INGEST_URL` → batched JSON on stdout (Phase 2 behavior) |

## ClickHouse schema check

```bash
curl -s -u default:finops_dev 'http://localhost:8123/?query=SHOW%20TABLES'
# finops.kafka_telemetry_queue, finops.workload_metrics, finops.telemetry_mv
```

If tables are missing or schema changed (partition / ORDER BY / TTL), reset the volume:

```bash
docker compose down -v && make compose-up
```

See [ADR 007](adr/007-clickhouse-mergetree-tuning.md), [ADR 008](adr/008-clickhouse-kafka-engine-resilience.md), [ADR 011](adr/011-replacingmergetree-dedupe-identity.md).

```bash
curl -s -u default:finops_dev "http://localhost:8123/?query=SHOW%20CREATE%20TABLE%20finops.workload_metrics" | grep -E 'ReplacingMergeTree|ORDER BY'
# Expect ReplacingMergeTree and ORDER BY (node, window_start_ns, cgroup_id)
```

```bash
curl -s -u default:finops_dev "http://localhost:8123/?query=SHOW%20CREATE%20TABLE%20finops.kafka_telemetry_queue" | grep -E 'skip_broken|num_consumers'
# Expect kafka_skip_broken_messages = 1000 and kafka_num_consumers = 1 (local)
```

## Local ports

| Service | Port |
|---------|------|
| Kafka (host) | 9092 |
| Kafka UI | 8080 |
| ClickHouse HTTP | 8123 |
| finops-api | 3000 |
| Grafana | 3001 |

## Enterprise checks

See [enterprise-latency.md](enterprise-latency.md): no handler `await` on Kafka; agent uses retry worker + env-tuned `reqwest` — [ADR 006](adr/006-shared-http-client-for-ingest.md).

Tear down: `make compose-down`. Rebuild API after code changes: `docker compose build finops-api && docker compose up -d finops-api` ([ADR 009](adr/009-finops-api-docker-compose.md)).

# Phase 3 validation guide

## Prerequisites

- Phase 2 prerequisites ([phase2-validation.md](phase2-validation.md))
- Docker: `docker compose` v2
- `make build` includes `statix-gateway`, `statix`, and `statix-wire`

## Stack smoke test

```bash
cd /root/ebpf-shaurya/Statix
cp .env.example .env
set -a && source .env && set +a
make compose-up
docker compose ps   # clickhouse, grafana, statix-gateway running
```

Expect: `make compose-up` prints `http://127.0.0.1:3000/health (OK)`.

## Agent → API → ClickHouse

```bash
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
```

Trigger workload activity. Wait one flush window.

### Pass criteria

| Test | Pass |
|------|------|
| API liveness | `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/health` → `200` |
| API readiness | `curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/ready` → `200` when ClickHouse healthy + mpsc &lt;80% |
| Prometheus (API) | `curl -s http://127.0.0.1:3000/metrics \| grep statix_api_` → lines present |
| Prometheus (agent) | `curl -s http://127.0.0.1:9091/metrics \| grep statix_ring_drops` → present |
| API ingest | `POST /ingest` (no auth when token unset) → `200` |
| Ingest auth | With `STATIX_API_TOKEN`: missing → `401`; valid Bearer → `200` ([ADR 019](../adr/019-ingest-bearer-token-auth.md)) |
| ClickHouse rows | `SELECT count() FROM statix.workload_metrics FINAL` → &gt; 0 after traffic |
| Read API | `GET /api/v1/workloads/summary?hours=24` → `200` + JSON ([ADR 027](../adr/027-api-read-path-clickhouse.md)) |
| Backpressure | Pause ClickHouse → within `STATIX_CH_INSERT_TIMEOUT_SECS`, `/ingest` and `/ready` → `503` ([ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)) |
| Schema gate | `schema_version` 2 or 3 → `200`; outside range → `400` ([ADR 020](../adr/020-ingest-schema-version-window.md)) |
| Stdout fallback | Unset `STATIX_INGEST_URL` → batched JSON on stdout |

## ClickHouse schema check

```bash
curl -s -u default:${CLICKHOUSE_PASSWORD} 'http://localhost:8123/?query=SHOW%20TABLES%20FROM%20statix'
# Expect: workload_metrics only (Phase 13 — no kafka_telemetry_queue / telemetry_mv)
```

Reset volume after schema change:

```bash
docker compose down -v && make compose-up
```

See [ADR 007](../adr/007-clickhouse-mergetree-tuning.md), [ADR 011](../adr/011-replacingmergetree-dedupe-identity.md), [ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md).

```bash
curl -s -u default:${CLICKHOUSE_PASSWORD} "http://localhost:8123/?query=SHOW%20CREATE%20TABLE%20statix.workload_metrics" | grep -E 'ReplacingMergeTree|ORDER BY'
```

## Local ports

| Service | Port |
|---------|------|
| ClickHouse HTTP | 8123 |
| statix-gateway | 3000 |
| Grafana | 3001 |
| Agent metrics | 9091 |

## Enterprise checks

See [enterprise-latency.md](enterprise-latency.md): no handler `await` on ClickHouse; agent WAL on `503` ([ADR 054](../adr/phase11/054-phase11-wal-spillway.md)).

Tear down: `make compose-down`. Rebuild gateway: `docker compose build statix-gateway && docker compose up -d statix-gateway`.

## Phase 14 — CPU time (`cpu_usage_usec`)

Agent emits schema **v3** with per-window CPU microseconds ([ADR 058](../adr/phase14/058-phase14-cpu-usage-tracking.md)).

```bash
make verify-phase14-cpu
# With stack + agent running:
# STATIX_PHASE14_E2E=1 make verify-phase14-cpu
```

| Gate | Pass |
|------|------|
| Priming | First sample per cgroup primes baseline only — unit test rejects lifetime spike on first billable delta |
| Conservation | Sum of per-sample deltas equals `usage_usec` span — unit test |
| Soft miss | Missing `cpu.stat` → CPU skipped; `memory.current` still parses — unit test |
| Backward compat | v2 JSON without `cpu_usage_usec` → `0` — `cargo test -p statix-wire` |
| Live E2E (optional) | `stress-ng --cpu 1` → `SELECT max(cpu_usage_usec) FROM statix.workload_metrics FINAL` &gt; 0; summary API returns `total_cpu_usec` |

Schema check:

```bash
curl -s -u default:${CLICKHOUSE_PASSWORD} \
  "http://localhost:8123/?query=DESCRIBE%20TABLE%20statix.workload_metrics" | grep cpu_usage_usec
```

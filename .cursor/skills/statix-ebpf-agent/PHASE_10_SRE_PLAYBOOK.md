# Phase 10 — SRE Golden-Signal Saturation Metrics

> **Status:** **Shipped** ([ADR 060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md)). Golden-Signal saturation series for the queue-less, WAL-backed pipeline.

## Topology (current)

```
agent emit_batch ──POST /ingest──► gateway mpsc (terminal buffer)
                                      │
                                      ├─ statix_gateway_mpsc_depth (background sampler)
                                      ├─ statix_api_ingest_503_total (flat backpressure counter)
                                      └─ clickhouse_writer → RowBinary → ClickHouse

agent retry/WAL spillway ──► statix_wal_bytes_current (seeded at init_wal)
```

**Physics:** all three are **Saturation** signals — pair with existing **Latency** (`statix_api_ingest_duration_seconds`, `statix_api_ch_insert_duration_seconds`, `statix_wal_fsync_seconds`), **Errors** (`statix_api_ch_insert_errors_total`, `statix_wal_dropped_*`, `statix_ring_drops_total`), and **Traffic** (`statix_api_ingest_requests_total`, `statix_api_ch_insert_rows_total`). Series seeded at startup so idle `0` is visible ([Pattern 17](PATTERNS.md)).

---

## Shipped ✅

| Task | ADR | Items |
|------|-----|-------|
| P10-1 ✅ | [060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md) | `statix_gateway_mpsc_depth` — background sampler in `statix-gateway/src/main.rs` (`capacity − tx.capacity()`) |
| P10-2 ✅ | [060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md) | `statix_api_ingest_503_total` — `record_ingest_metrics` in `statix-gateway/src/routes/ingest.rs` |
| P10-3 ✅ | [060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md) | `statix_wal_bytes_current` startup seed — `output::init_wal` in `statix/src/output.rs` |
| P10-4 ✅ | [060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md) | HELP text (`describe_*`) + startup seeds — gateway/agent `main.rs` |
| P10-5 ✅ | [060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md) | Docs — [observability-metrics.md](../../../docs/guides/observability-metrics.md); skills/TODO sync |

---

## Verification (run after deploy)

```bash
make build && make check
cargo test -p statix-gateway
```

**Gateway** (rebuild image after code changes):

```bash
docker compose build statix-gateway && docker compose up -d statix-gateway
curl -s http://127.0.0.1:3000/metrics | grep -E 'statix_gateway_mpsc_depth|statix_api_ingest_503_total'
# → both present; depth 0 at idle. Pause ClickHouse or flood /ingest → 503 counter climbs, depth rises.
```

**Agent** (root; rebuild after code changes — `make build-agent`, then restart):

```bash
sudo killall statix   # stop stale agent
make build-agent
sudo make run       # passes STATIX_INGEST_URL by default
curl -s http://127.0.0.1:9091/metrics | grep statix_wal_bytes_current
# → statix_wal_bytes_current 0 at idle. Stop gateway + generate load → value rises; drains to 0 on recovery.
```

WAL gauges are seeded at agent startup (before `init_wal`); you must restart the agent after rebuilding — a running process won't pick up the new binary.

## Reference

- **Env:** `STATIX_MPSC_DEPTH_SAMPLE_MS` (default `1000`, gateway mpsc depth sampler).
- **PromQL:** see [observability-metrics.md](../../../docs/guides/observability-metrics.md) — buffer saturation %, `rate(statix_api_ingest_503_total[5m])`, WAL spillway growth.
- **Pattern:** [PATTERNS.md](PATTERNS.md) Pattern 17 (background gauge sampler + startup seed).
- **Open (Phase 10 remainder):** flush duration, retry depth, cache size, drain budget hits — [TODO.md](TODO.md).

# Observability — Golden-Signal saturation metrics (Phase 10)

SRE dashboards for the queue-less, WAL-backed pipeline. Full decision record: [ADR 060](../adr/phase10/060-phase10-golden-signal-saturation-metrics.md).

## Endpoints

| Component | URL |
|-----------|-----|
| statix-gateway | `http://<host>:3000/metrics` |
| statix agent | `http://<host>:9091/metrics` |

## Saturation series (Phase 10)

| Metric | Type | Host | Description |
|--------|------|------|-------------|
| `statix_gateway_mpsc_depth` | gauge | gateway | Occupied ingest mpsc slots (`capacity − free permits`) |
| `statix_api_ingest_503_total` | counter | gateway | Flat count of `POST /ingest` HTTP 503 responses |
| `statix_wal_bytes_current` | gauge | agent | On-disk WAL spillway size (bytes) |

All three are **seeded to 0 at startup** so idle hosts export the series immediately.

### Related (cause breakdown / existing)

| Metric | Host |
|--------|------|
| `statix_api_ingest_requests_total{status="503"}` | gateway — label-based equivalent of 503 counter |
| `statix_api_ch_unhealthy_reject_total` | gateway — Tier 1 `!ch_healthy` |
| `statix_api_ingest_channel_full_total` | gateway — Tier 2 mpsc full |
| `statix_wal_segments_current` | agent — WAL segment count |

## PromQL examples

**Gateway buffer saturation (%):**

```promql
statix_gateway_mpsc_depth / 8192 * 100
```

Replace `8192` with your `STATIX_INGEST_CHANNEL_SIZE`. Alert when > 80 (matches `/ready` gate).

**Backpressure rate:**

```promql
rate(statix_api_ingest_503_total[5m])
```

**WAL spillway growth:**

```promql
statix_wal_bytes_current
deriv(statix_wal_bytes_current[10m])
```

Pair with `statix_wal_dropped_bytes_total` for true loss.

## Environment

| Variable | Default | Component | Purpose |
|----------|---------|-----------|---------|
| `STATIX_MPSC_DEPTH_SAMPLE_MS` | `1000` | gateway | mpsc depth sampler period |
| `STATIX_INGEST_CHANNEL_SIZE` | `8192` | gateway | mpsc capacity (denominator for saturation %) |
| `STATIX_WAL_MAX_BYTES` | `536870912` | agent | WAL hard cap (alert threshold) |

## Verify

```bash
make build && make check
curl -s http://127.0.0.1:3000/metrics | grep -E 'statix_gateway_mpsc_depth|statix_api_ingest_503_total'
curl -s http://127.0.0.1:9091/metrics | grep statix_wal_bytes_current
```

See [phase3-validation.md](phase3-validation.md) for stack smoke tests.

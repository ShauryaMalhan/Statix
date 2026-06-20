# ADR 060: Phase 10 — Golden-Signal saturation metrics

**Status:** Accepted  
**Date:** 2026-06-18  
**Context:** [PHASE_10_SRE_PLAYBOOK.md](../../.cursor/skills/statix-ebpf-agent/PHASE_10_SRE_PLAYBOOK.md) — queue-less ingest (Phase 13) + agent WAL (Phase 11) needed three saturation series for SRE dashboards without hot-path coupling.

## Decision

### Gateway terminal buffer — `statix_gateway_mpsc_depth` (gauge)

- Background sampler in `statix-gateway/src/main.rs` (not ingest handler, not `/ready`).
- Depth = `ingest_channel_capacity − tx.capacity()` (includes reserved-but-unsent permits).
- Seeded to `0` at sampler start; env `STATIX_MPSC_DEPTH_SAMPLE_MS` (default `1000`).

### Backpressure tripwire — `statix_api_ingest_503_total` (counter)

- One increment in `record_ingest_metrics` when status is `503` — covers `!ch_healthy`, mpsc `Full`, and mpsc `Closed`.
- Seeded at gateway startup (`increment(0)`).
- Existing reason counters (`statix_api_ch_unhealthy_reject_total`, `statix_api_ingest_channel_full_total`) retained for cause breakdown.

### Agent spillway — `statix_wal_bytes_current` (gauge)

- **Already emitted** on spill/GC (`wal/mod.rs`); gap was startup visibility.
- Seed `statix_wal_bytes_current` and `statix_wal_segments_current` to `0` in `output::init_wal` before the disabled early-return.

### HELP text convention

- First use of `metrics::describe_gauge!` / `describe_counter!` at agent and gateway startup for the affected series.

## Consequences

- **Positive:** Dashboards show `0` instead of "No data" at idle; flat 503 counter simplifies alerts; mpsc depth visible under sustained backpressure.
- **Negative:** One background task per gateway pod; negligible cost (one atomic read + gauge set per sample period).
- **Operational:** Alert examples in playbook §8 — mpsc depth > 80% capacity, `rate(statix_api_ingest_503_total[5m]) > 0`, WAL bytes approaching cap.
- **Agent exporter fix:** Upgraded `metrics-exporter-prometheus` 0.12 → 0.17 (0.12 depended on `metrics` 0.21 internally while agent uses 0.24 — recordings never rendered). Added explicit `[[bin]] name = "statix"` and renamed lib to `statix_bpf` so `cargo build -p statix` compiles the agent binary, not library-only.

## References

- [ADR 012](../012-finops-api-prometheus-metrics.md) — gateway metrics baseline
- [ADR 029](../029-ready-channel-depth-gate.md) — `/ready` 80% threshold
- [ADR 054](../phase11/054-phase11-wal-spillway.md) — WAL spillway
- [ADR 055](../phase13/055-phase13-part1-kafka-removal-rowbinary.md) — queue-less ingest

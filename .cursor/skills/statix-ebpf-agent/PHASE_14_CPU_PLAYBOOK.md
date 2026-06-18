# Phase 14 — CPU Time Tracking (`cpu_usage_usec`)

> **Audience:** the Cursor execution engine.
> **Status:** **Shipped** ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)). P14-10 docs + verify script complete.

## Topology (current)

```
sched_process_exec → ring buffer → agent ─┐
cgroup memory.current ── sampler ───────┼─ aggregator → emit_batch (schema v3)
cgroup cpu.stat (delta) ── sampler ─────┘       → POST /ingest → MetricRow → RowBinary
        → statix.workload_metrics.cpu_usage_usec
```

**Physics:** `cpu.stat` `usage_usec` is cumulative — store per-window **delta**, baseline in `Sampler.cpu_baseline` (survives flushes). **Priming:** first read per cgroup sets baseline only (delta 0). **No BPF surface** — user-space cgroupfs only ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)).

---

## Shipped ✅

| Task | ADR | Items |
|------|-----|-------|
| P14-1 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `WorkloadRow.cpu_usage_usec` (`#[serde(default)]`, last field) — `statix-wire` |
| P14-2 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `cpu_stat_paths`, `for_each_sample_target`, `read_cpu_usage_usec_at` — `attribution/` |
| P14-3 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `ingest_cpu_sample`, flush emit — `aggregator.rs` |
| P14-4 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | Stateful `Sampler`, prime-aware delta, one `spawn_blocking` — `memory_sampler.rs` |
| P14-5 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | Lifetime `Sampler` in `main.rs` sample tick |
| P14-6 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `SCHEMA_VERSION` 3 — `output.rs` |
| P14-7 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `MetricRow.cpu_usage_usec` (last field) — `clickhouse_writer.rs` |
| P14-8 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `cpu_usage_usec UInt64` + ALTER note — `deploy/clickhouse/01_init.sql` |
| P14-9 ✅ | [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md) | `total_cpu_usec` summary — `routes/query.rs` |

---

## Verification (run after deploy)

- `make build && make check` — BPF untouched; no `make verify-btf` required.
- `cargo test -p statix-wire` — v2 missing field → `cpu_usage_usec = 0`; v3 round-trip.
- `cargo test -p statix-gateway` — existing gateway tests pass.
- `make verify-phase14-cpu` — priming, conservation, soft miss unit gates + wire/gateway smoke.
- **Optional E2E:** `STATIX_PHASE14_E2E=1 make verify-phase14-cpu` (stack + agent running).
- **CH migration:** dev → `docker compose down -v && make compose-up`; prod → `ALTER TABLE statix.workload_metrics ADD COLUMN IF NOT EXISTS cpu_usage_usec UInt64 DEFAULT 0 AFTER sample_count`.
- **Live drill:** `stress-ng --cpu 1 --timeout 30s` → `SELECT cgroup_id, cpu_usage_usec FROM statix.workload_metrics FINAL ORDER BY cpu_usage_usec DESC LIMIT 5` → busy cgroup &gt; 0.
- **Read API:** `curl -s 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=1' | jq '.[].total_cpu_usec'`.

## Reference

- Env: reuses `STATIX_SAMPLE_INTERVAL_SECS`, `STATIX_CGROUP_ROOT` (no new required vars).
- Metrics: `statix_cpu_sampler_errors_total` (agent `:9091`); `statix_memory_sampler_errors_total` on JoinError.
- Pattern: [PATTERNS.md](PATTERNS.md) Pattern 6d; full decision record in [ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md).

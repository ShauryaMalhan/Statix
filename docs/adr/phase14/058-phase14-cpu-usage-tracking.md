# ADR 058: Phase 14 — CPU time tracking (`cpu_usage_usec`)

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** [PHASE_14_CPU_PLAYBOOK.md](../../.cursor/skills/statix-ebpf-agent/PHASE_14_CPU_PLAYBOOK.md) — FinOps billing needed actual CPU time per workload window, not only `exec_count`.

## Decision

### Userspace-only (no BPF surface)

- Read cgroup v2 `cpu.stat` field `usage_usec` (cumulative counter) from user space on the existing sample tick.
- Same pattern as `memory.current`: precomputed `Arc<PathBuf>` per cgroup in `AttributionCache`; one `spawn_blocking` per tick reads both files.
- **`statix-common` / `statix-ebpf` unchanged** — no ring-buffer or verifier impact.

### Delta + priming

- Store per-window **delta** `usage_usec(t_end) − usage_usec(t_start)`, not the raw counter.
- **Baseline map** (`Sampler.cpu_baseline: FxHashMap<u64, u64>`) lives in the sampler (survives aggregator window flips).
- **First observation primes** baseline only (delta 0) — prevents lifetime CPU spikes on boot/bootstrap.
- Subsequent samples use `current.saturating_sub(last)` for monotonic guard.

### Wire + schema version

- `statix_wire::WorkloadRow.cpu_usage_usec: u64` with `#[serde(default)]` (last field).
- Agent emits **schema_version 3**; gateway already accepts 2..=3 (ADR 020). v2/WAL batches default CPU to 0.

### Storage + gateway

- `MetricRow.cpu_usage_usec` appended last; matches `deploy/clickhouse/01_init.sql` column order for RowBinary.
- Read API: `sum(cpu_usage_usec) AS total_cpu_usec` on summary query (no `FINAL` — same WAL double-count caveat as `total_execs`).

## Consequences

- **Positive:** Real compute signal for FinOps; reuses sample interval; backward-compatible ingest.
- **Negative:** Requires cpu controller enabled in cgroup subtree (`cpu.stat` soft-miss if absent).
- **Operational:** Recommend `STATIX_SAMPLE_INTERVAL_SECS <= STATIX_WINDOW_SECS` for smooth per-window attribution (sums remain correct either way).

## References

- [ADR 016](../016-clock-domain-offset.md) — window timestamps
- [ADR 020](../020-ingest-schema-version-window.md) — schema 2..=3
- [ADR 056](../phase13/056-phase13-part2-ingest-zero-alloc.md) — `MetricRow` positional insert

# Phase 14 — CPU Time Tracking (`cpu_usage_usec`) — Cursor Playbook

> **Audience:** the Cursor execution engine implementing Phase 14.
> Strict instruction manual: each task is **What / Why / How**, in dependency order.
> Run `cargo check --workspace` after each task. One ADR for the wave:
> `docs/adr/phase14/058-phase14-cpu-usage-tracking.md` (next free number is **058**;
> highest shipped is 057 / Phase 13).
> **Status:** **Shipped** ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)). P14-10 full docs deferred.

---

## Problem

We bill and right-size workloads from telemetry, but the only compute signal we
persist is `exec_count` — the number of `sched:sched_process_exec` events. That is
a **proxy, not a measurement**: a workload that `exec`s once and then pins a core
for ten minutes looks *cheaper* than one that forks a thousand short-lived helpers.
For FinOps (cost attribution) and OOM/right-sizing, we need the **actual CPU time
consumed per workload per flush window** — `cpu_usage_usec` (microseconds) — plumbed
end to end and stored next to memory in `statix.workload_metrics`.

## The one physics fact that drives the whole design

cgroup v2 exposes per-cgroup CPU accounting at `<cgroup>/cpu.stat`. First line:

```
usage_usec 95855377913      <- CUMULATIVE microseconds since cgroup creation
user_usec  72339309632
system_usec 23516068281
...
```

`usage_usec` is a **monotonic cumulative counter** (like a Prometheus counter),
**not** an instantaneous gauge like `memory.current`. Therefore:

- We do **not** store `usage_usec` directly. We store the **delta** consumed during
  the window: `usage_usec(t_end) − usage_usec(t_start)`.
- The "last observed counter value" (the baseline) must **survive window flips**.
  The aggregator is double-buffered and `.clear()`-ed every flush, so the baseline
  **cannot** live in the per-window map — it lives in the long-lived sampler.

Get this wrong and billing is wrong. Two correctness traps, both mandatory:

1. **Priming.** The *first* time we see a cgroup, `last = 0`, so a naive delta is
   `current − 0 = current` = the cgroup's *entire lifetime* CPU. For workloads that
   existed before the agent started (`bootstrap_existing_cgroups` registers them with
   hours/days of accrued CPU), this dumps a massive spurious spike into the first
   window. **The first observation must only prime the baseline and contribute delta 0.**
2. **Reset / regression defense.** A recreated cgroup gets a new inode → new
   `cgroup_id` → fresh baseline naturally. Within one live cgroup `usage_usec` is
   monotonic, but defend against any rare accounting regression with
   `current.saturating_sub(last)` (yields 0, never a wrap-around spike).

## Architecture decision: CPU is a pure user-space change (no BPF surface)

CPU usage is read from cgroupfs in user space — exactly like the existing
`memory.current` sampler — and **never traverses the BPF ring buffer**. Consequences,
stated loudly because they shrink the blast radius:

- **`statix-common` and `statix-ebpf` are UNTOUCHED.** No new `StatixEvent` field, no
  new event kind, no change to the 64-byte ring record, no `bpf_*` helper calls.
- **`make verify-btf` and the kernel CI verifier matrix (5.10 / 5.15 / 6.1 / 6.8) are
  not in scope** — Phase 9's "existential" verifier-compatibility risk is not touched.
- The model is *identical* to memory sampling (Pattern 6): precompute an
  `Arc<PathBuf>` per cgroup on identity, read it in a `spawn_blocking` once per tick,
  feed the aggregator. We **reuse the existing sample tick** (`STATIX_SAMPLE_INTERVAL_SECS`)
  and read `cpu.stat` in the **same `spawn_blocking`** that already reads `memory.current`
  — honoring the "one `spawn_blocking` per tick reads all cgroup paths" contract.

Topology after Phase 14:

```
sched_process_exec → ring buffer → agent ─┐
                                          ├─ aggregator (per-window rollup)
cgroup cpu.stat ── sampler (delta) ───────┘        │  memory_bytes_{max,last}, exec_count,
cgroup memory.current ── sampler (gauge) ──────────┘  sample_count, cpu_usage_usec  ← NEW
        → emit_batch (schema_version 3) → POST /ingest → MetricRow → RowBinary
        → statix.workload_metrics.cpu_usage_usec  ← NEW column
```

## Schema-version story (already pre-provisioned — exploit it)

The gateway already accepts `schema_version` **2..=3** (`routes/ingest.rs`,
`MIN=2 MAX=3`; ADR 020 deliberately opened the window). The agent currently emits
**2** (`output.rs: SCHEMA_VERSION = 2`). Phase 14:

- Bump the agent to emit **3** (the payload now carries `cpu_usage_usec`).
- The new wire field is `#[serde(default)]`, so the gateway still parses any lingering
  **v2** batches **and** WAL frames written by a pre-upgrade agent — `cpu_usage_usec`
  defaults to `0`. **No edit to the gateway version gate is required** (3 is already legal).

This makes the rollout forward/backward compatible and the WAL replay (at-least-once,
deduped by `ReplacingMergeTree`) safe across the upgrade boundary.

---

## Tasks (dependency order — implement top to bottom)

### P14-1 (wire) Add `cpu_usage_usec` to `WorkloadRow` — `statix-wire/src/lib.rs`
**What:** add `pub cpu_usage_usec: u64` to `WorkloadRow`, as the **last** field,
with `#[serde(default)]`.
**Why:** it is the carrier for the new metric; leaf dependency for both the aggregator
(producer) and the gateway `MetricRow` (consumer). `#[serde(default)]` = v2/WAL
backward-compat (missing field → 0).
**How:**
```rust
pub struct WorkloadRow {
    // ... existing fields, unchanged order ...
    pub exec_count: u32,
    pub sample_count: u32,
    /// CPU microseconds consumed during this window (delta of cgroup cpu.stat usage_usec).
    #[serde(default)]
    pub cpu_usage_usec: u64,
}
```
Do **not** add `skip_serializing_if` — it is a core billing metric and must always
serialize from a v3 agent. Append last so JSON field order stays stable.

### P14-2 (attribution) Precompute + read `cpu.stat` — `statix/src/attribution/mod.rs` (+ `error.rs`)
**What:** track a per-cgroup `Arc<PathBuf>` to `cpu.stat` parallel to
`memory_current_paths`, and add a stack-buffer reader for `usage_usec`.
**Why:** same hot-path discipline as memory — no per-tick `PathBuf::join`, no
`read_to_string`; the sampler clones an `Arc` only.
**How:**
1. `CacheState`: add `cpu_stat_paths: FxHashMap<u64, Arc<PathBuf>>`.
2. Add `fn precompute_cpu_stat(cgroup_root, rel_path) -> PathBuf` mirroring
   `precompute_memory_current` (`.../cpu.stat`).
3. Populate `cpu_stat_paths` **everywhere** `memory_current_paths` is populated:
   `on_identity_event` and `register_cgroup_directory`.
4. **Evict it in `evict_stale_cgroups`** (cascade-delete alongside the memory path) —
   do not leave a second unbounded map (the V3-4 lesson).
5. Replace `for_each_memory_current_path` with a combined
   `for_each_sample_target(|cgroup_id, mem_path: Arc<PathBuf>, cpu_path: Arc<PathBuf>|)`
   so the sampler builds **one** target list and **one** blocking closure reads both
   files. (Only the memory-sampler call site consumes this — and we are editing it
   anyway in P14-4.)
6. Add `pub fn read_cpu_usage_usec_at(path: &Path) -> Result<u64, AttributionError>`:
   `File::open` → read into a stack `[u8; 256]` (cpu.stat is ~9 short lines; the field
   we want is the **first** line) → take the first line → `strip_prefix("usage_usec ")`
   → `trim()` → `parse::<u64>()`. No `read_to_string`.
7. `attribution/error.rs`: add `EmptyCpuStat { path }`, `InvalidCpuUtf8 { path, source }`,
   `ParseCpuUsage { path, value }`, `NoCpuUsageField { path }` mirroring the
   `memory.current` variants.
**Operational note:** `cpu.stat` exists only when the cpu controller is enabled in the
parent's `cgroup.subtree_control`. When absent/unreadable, treat it as a **soft miss**
(debug-log + skip, no delta) exactly like a failed `memory.current` read — never panic.

### P14-3 (aggregator) Per-window CPU accumulator — `statix/src/aggregator.rs`
**What:** add `cpu_usage_usec: u64` to `WorkloadStats` and an `ingest_cpu_sample`
method; emit the field in `flush`.
**Why:** the window value is the **sum of per-sample deltas** that landed in the
active buffer; this is conserved across early-flush splits because the baseline lives
in the sampler, not here.
**How:**
1. `WorkloadStats`: add `cpu_usage_usec: u64`; `Default` → `0`.
2. New method mirroring `ingest_memory_sample`:
   ```rust
   pub fn ingest_cpu_sample(
       &mut self, cgroup_id: u64, delta_usec: u64,
       cache: &AttributionCache, node: &str,
   ) -> Option<BatchPayload> {
       let entry = self.active_mut().entry(cgroup_id).or_default();
       entry.cpu_usage_usec = entry.cpu_usage_usec.saturating_add(delta_usec);
       entry.labels = cache.labels_for_cgroup(cgroup_id);
       self.try_early_flush(node, cache)
   }
   ```
   (`saturating_add` — never overflow; a window accumulates many deltas.)
3. In `flush`, add `cpu_usage_usec: s.cpu_usage_usec` to the `WorkloadRow { .. }` literal.
4. **Leave the identity-event path (`on_statix_event`, kind=1) alone** — CPU enters
   only via the sampler, exactly as `memory_bytes` does. `WorkloadStats::default()`
   seeds it to 0 for cgroups that exec but are never sampled.

### P14-4 (sampler) Delta computation + priming + baseline — `statix/src/memory_sampler.rs`
**What:** evolve the stateless memory sampler into a **stateful sampler** that, in one
tick, reads both `memory.current` (gauge) and `cpu.stat` (counter→delta), owning the
CPU baseline map across ticks.
**Why:** the cumulative baseline must persist across window flips; the only long-lived
home that isn't the lock-protected cache is the sampler itself. One `spawn_blocking`
per tick reads both files for all cgroups (latency contract).
**How:**
1. Introduce a struct (rename the module concept to a unified sampler; keep the file or
   rename to `sampler.rs` — your call, but do not create a *second* tick):
   ```rust
   pub struct Sampler {
       /// Last observed cumulative cpu.stat usage_usec, per cgroup. Survives flips.
       cpu_baseline: FxHashMap<u64, u64>,
   }
   ```
2. `tick(&mut self, cache, agg, node) -> Vec<BatchPayload>`:
   - Build `targets: Vec<(u64, Arc<PathBuf>, Arc<PathBuf>)>` via `for_each_sample_target`.
   - **Prune** `cpu_baseline` to live cgroups (`retain(|id,_| live.contains(id))`) so it
     can't grow unbounded as cgroups die (mirror the cache-eviction discipline).
   - One `spawn_blocking`: for each target read `memory.current` and `cpu.stat`, return
     `Vec<(cgroup_id, Option<u64> memory_bytes, Option<u64> usage_usec)>`. A failed read
     is `None` for that file only (soft miss), not a dropped row.
   - Back on the async side, per reading:
     - memory: existing `agg.ingest_memory_sample(...)` (unchanged).
     - cpu (prime-aware delta):
       ```rust
       if let Some(current) = usage_usec {
           match self.cpu_baseline.get(&cgroup_id) {
               Some(&last) => {
                   let delta = current.saturating_sub(last);     // monotonic guard
                   self.cpu_baseline.insert(cgroup_id, current);
                   if let Some(b) = agg.ingest_cpu_sample(cgroup_id, delta, cache, node) {
                       early.push(b);
                   }
               }
               None => { self.cpu_baseline.insert(cgroup_id, current); } // PRIME: delta 0
           }
       }
       ```
   - Collect early-flush batches from both ingest paths and return them.
3. On the JoinError path keep the existing `statix_memory_sampler_errors_total` handling
   (V3-10); add `statix_cpu_sampler_errors_total` for cpu-specific read failures if you
   want per-file visibility (optional).

### P14-5 (wiring) Instantiate the stateful sampler — `statix/src/main.rs`
**What:** own one `Sampler` for the process lifetime and call it from the existing
`sample_interval.tick()` arm.
**Why:** the baseline must persist across ticks — it cannot be a per-call local.
**How:**
- Before the loop: `let mut sampler = memory_sampler::Sampler::new();`
- Replace the body of the `_ = sample_interval.tick()` branch:
  ```rust
  for batch in sampler.tick(&cache, &mut agg, &node).await {
      output::emit_batch(batch);
  }
  ```
- No new env var or timer — CPU rides the existing sample interval.
- **Recommend in the ADR/docs:** keep `STATIX_SAMPLE_INTERVAL_SECS <= STATIX_WINDOW_SECS`.
  CPU totals are conserved either way (each delta is counted exactly once), but if the
  sample interval exceeds the window, a window with no sample reports `cpu_usage_usec = 0`
  and the next sampled window carries the full accrued delta (lumpy per-window
  attribution; sums remain correct).

### P14-6 (output) Emit schema v3 — `statix/src/output.rs`
**What:** `pub const SCHEMA_VERSION: u32 = 2;` → `3`.
**Why:** signals that the payload now carries `cpu_usage_usec`.
**How:** one-line change. `emit_batch` already moves `payload.workloads` (now including
`cpu_usage_usec`) into `IngestBatch` — no other edit. The `RawEventJson` debug struct is
for raw *ring* events (not workloads) — leave it untouched.
**WAL note:** frames serialized by a pre-upgrade (v2) agent replay fine post-upgrade —
gateway parses them with `cpu_usage_usec` defaulting to 0 (P14-1 serde default);
`ReplacingMergeTree` dedup still holds.

### P14-7 (gateway) Carry CPU into ClickHouse rows — `statix-gateway/src/clickhouse_writer.rs`
**What:** add `cpu_usage_usec: u64` to `MetricRow` (as the **last** field) and map it in
`from_ingest`.
**Why:** the RowBinary insert path is positional — `MetricRow` field order must match
the table column order. Appending last in both keeps every existing column position
stable.
**How:**
```rust
#[derive(Row, Serialize)]
pub struct MetricRow {
    // ... existing fields, order unchanged ...
    exec_count: u32,
    sample_count: u32,
    cpu_usage_usec: u64,   // NEW — last, matches CREATE TABLE column order
}
// from_ingest:
cpu_usage_usec: w.cpu_usage_usec,
```
`routes/ingest.rs` needs **no change** (it just calls `MetricRow::from_ingest`, and the
schema gate already allows v3).

### P14-8 (storage) Add the column — `deploy/clickhouse/01_init.sql`
**What:** add `cpu_usage_usec UInt64` to `statix.workload_metrics`, **appended last**
(after `sample_count`), and document the migration for existing volumes.
**Why:** `CREATE TABLE IF NOT EXISTS` does **not** alter an existing table — fresh
installs get the column; existing data needs an explicit `ALTER`. Appending last keeps
RowBinary column order aligned with the appended `MetricRow` field (P14-7).
**How:**
1. Fresh schema:
   ```sql
   ...
   exec_count UInt32,
   sample_count UInt32,
   cpu_usage_usec UInt64        -- Phase 14: CPU microseconds consumed per window
   )
   ENGINE = ReplacingMergeTree(window_end_ns) ...
   ```
2. Migration comment for existing deployments (non-destructive, backfills 0):
   ```sql
   -- Existing volume: ALTER TABLE statix.workload_metrics
   --   ADD COLUMN IF NOT EXISTS cpu_usage_usec UInt64 DEFAULT 0 AFTER sample_count;
   ```
   Dev: `docker compose down -v && make compose-up` (wipes data). Prod: run the `ALTER`.
   `DEFAULT 0` means old rows read back 0 CPU; new positional RowBinary inserts line up
   because the added column is last.

### P14-9 (read API) Expose CPU in the summary — `statix-gateway/src/routes/query.rs`
**What:** add `total_cpu_usec` to `SUMMARY_SQL` (`sum(cpu_usage_usec)`) and to
`WorkloadSummaryRow`.
**Why:** the whole point is to make CPU *visible* to FinOps consumers; without this the
metric lands in storage but never surfaces on the read path.
**How:**
- `SUMMARY_SQL`: add `sum(cpu_usage_usec) AS total_cpu_usec` to the SELECT list.
- `WorkloadSummaryRow`: add `pub total_cpu_usec: u64`.
- **Caveat (state in ADR):** the summary runs **without `FINAL`** (operational read), so
  `sum(cpu_usage_usec)` can double-count windows that were replayed from the WAL — the
  same caveat that already applies to `total_execs`. Exact billing must sum over a
  deduped/`FINAL` subquery, consistent with the existing `FINAL` billing guidance.

### P14-10 (project rule) ADR + docs + skills — same PR
**What / Why:** hard project rule (CLAUDE.md): every architectural change ships its ADR,
README/guide updates, and skill-file updates in the same PR.
**How:**
- **ADR:** `docs/adr/phase14/058-phase14-cpu-usage-tracking.md` — record: cumulative
  counter → delta, priming, baseline-in-sampler, no-BPF-surface, schema v3 reuse of the
  2..=3 window, RowBinary positional append, non-FINAL summing caveat.
- **`docs/guides/phase3-ingest-interface.md`:** wire contract changed (new field,
  agent now emits v3).
- **`SKILL.md`:** phase line (add "14 done"); shared-memory/latency tables note the CPU
  sampler reads `cpu.stat` (delta) on the same tick as `memory.current`.
- **`REFERENCE.md`:** roadmap row for Phase 14; sampler note (two files, one
  `spawn_blocking`).
- **`PATTERNS.md`:** new pattern — "CPU sampling: cumulative counter → per-window delta,
  prime-aware, baseline in sampler".
- **`TODO.md`:** Phase 14 section + current-focus header (done in this change set).

---

## Env vars

No new required env. CPU reuses `STATIX_SAMPLE_INTERVAL_SECS` (default 10) and the
existing `STATIX_CGROUP_ROOT`. (Optional, only if you want a kill switch:
`STATIX_CPU_ACCOUNTING=0` to skip cpu.stat reads on hosts without the cpu controller —
but the soft-miss handling in P14-2/P14-4 already degrades gracefully without it.)

## Metrics (`:9091`, agent)

| Metric | Type | Meaning |
|--------|------|---------|
| `statix_cpu_sampler_errors_total` | counter | `cpu.stat` read/parse failures (optional, per-file visibility) |

(Reuse `statix_memory_sampler_errors_total` for the shared `spawn_blocking` JoinError.)

## Verification

```bash
make build && make check          # BPF untouched → make verify-btf NOT required
cargo test -p statix-wire         # serde round-trip incl. cpu; v2 (missing) → 0
cargo test -p statix              # aggregator cpu accumulate; sampler priming (1st = delta 0); monotonic delta
cargo test -p statix-gateway      # MetricRow::from_ingest maps cpu; v2 still parses (cpu=0)
# CH migration: dev → docker compose down -v && make compose-up ; prod → ALTER ADD COLUMN
# E2E busy workload:
#   stress-ng --cpu 1 --timeout 30s   (or `yes > /dev/null &`)
#   curl -s -u "default:${CLICKHOUSE_PASSWORD}" \
#     'http://localhost:8123/?query=SELECT%20cgroup_id,%20cpu_usage_usec%20FROM%20statix.workload_metrics%20FINAL%20ORDER%20BY%20cpu_usage_usec%20DESC%20LIMIT%205'
#   → cpu_usage_usec > 0 for the busy cgroup; idle cgroups ≈ 0
curl -s 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=1' | jq '.[].total_cpu_usec'
```

**Correctness drill (the priming trap):** start the agent on a host with a long-running
busy process. The **first** window for that pre-existing cgroup must report a *small*
`cpu_usage_usec` (one sample interval's worth), **not** its multi-hour lifetime total.
If the first window shows a giant spike, priming (P14-4) is broken.

## Execution order

P14-1 (wire) → P14-2 (attribution + errors) → P14-3 (aggregator) → P14-4 (sampler) →
P14-5 (main wiring) → P14-6 (output schema v3) → P14-7 (gateway MetricRow) →
P14-8 (CH schema + migration) → P14-9 (read API) → P14-10 (ADR + docs + skills).

Rationale: `WorkloadRow` (P14-1) is the leaf both the aggregator and gateway depend on;
the sampler (P14-4) depends on both the attribution reader (P14-2) and the aggregator
method (P14-3); storage (P14-8) must land with the gateway field (P14-7) so RowBinary
column order stays aligned.

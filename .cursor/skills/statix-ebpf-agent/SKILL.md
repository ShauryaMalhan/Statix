---
name: statix-ebpf-agent
description: >-
  Enterprise low-latency standards for the Statix eBPF stack:
  BPF ring buffer, batched agent, HTTP→gateway→ClickHouse RowBinary; Phase 13 queue-less ingest.
  Use when editing statix-common, statix-ebpf, statix-wire, statix-infra, statix, statix-gateway; adding probes;
  ingest, Docker infra, or ADRs. Always read this skill first, then build with make,
  and update docs/adr/skills in the same change.
---

# Statix eBPF Agent

**Enterprise goal:** &lt;0.1% node CPU at idle, **zero blocking** on kernel event drain, **no telemetry loss** on capacity signals.

Phases: **1–4 done** · **5.5 V1/V2/V3 done** · **11 done** (WAL — [ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)) · **13 done** ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)–[057](../../../docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md)) · **14 done** ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)) · **10 partial** (Golden-Signal saturation [ADR 060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md)) · **5 partial** · **6–7 done** · **T1–3 done** · **8–9 partial**

## Mandatory workflow (every change)

1. Read [SKILL.md](SKILL.md) → [REFERENCE.md](REFERENCE.md) → [PATTERNS.md](PATTERNS.md)
2. **For hot-path / performance fixes:** Read [L8-AUDIT-FIXES.md](L8-AUDIT-FIXES.md) — contains exact before/after code, dependency order, and pitfalls. Follow the prescribed approach exactly; do not invent alternatives.
3. Implement using patterns below (do not invent parallel conventions)
4. `make build && make check` (add `make verify-btf` if BPF/deploy changed)
5. **ADR** — new file in `docs/adr/` (Phase 5.5 → `phase55/`; Phase 13 → `phase13/`) ([enterprise-latency.md](../../../docs/guides/enterprise-latency.md))
6. **Docs** — update README, phase validation, `phase5-production-readiness.md` if deploy gates change; `phase3-ingest-interface.md` if wire contract changes
7. **Skills** — update this skill, REFERENCE, PATTERNS, TODO in the **same PR**
8. Deferred work → [TODO.md](TODO.md); mark shipped items `[x]` (keep the line)

## Quick start checklist

```
- [ ] statix-common: StatixEvent / kinds only here
- [ ] BPF: EVENTS map name matches loader; reserve → fill → submit(0); on reserve fail increment `RING_DROPS` ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md))
- [ ] Agent: no await on ring-buffer path; `DRAIN_BUDGET=256` ([ADR 032](../../../docs/adr/phase55/l8/032-phase55-l8-p0-hot-path-fixes.md)); `emit_batch` moves `BatchPayload`; Prometheus `:9091` ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md))
- [ ] Aggregator: FxHashMap, double buffer, early flush (never enforce_cap); `clock_offset_ns` ([ADR 016](../../../docs/adr/016-clock-domain-offset.md))
- [ ] Output: `STATIX_INGEST_URL` → `init_http_client` (+ optional `STATIX_API_TOKEN`) + `init_retry_worker` ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md), [ADR 019](../../../docs/adr/019-ingest-bearer-token-auth.md))
- [ ] API: `Config::from_env()` first in `main` ([ADR 030](../../../docs/adr/030-finops-api-config-struct.md)); GET /health; GET /ready (`ch_healthy` + mpsc &lt;80%); POST /ingest `try_reserve_many`; read API
- [ ] make build && make check
- [ ] docs/adr + skills updated
```

## Workspace contract

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `statix-common` | host + bpf | `StatixEvent`, kind constants, `Pod` via `user` feature |
| `statix-wire` | host lib | `IngestBatch`, `WorkloadRow` ([ADR 028](../../../docs/adr/028-finops-wire-and-agent-rename.md)) |
| `statix-ebpf` | `bpfel-unknown-none` | tracepoint, `cgroup_id`, ring buffer (`STATIX_RING_BUF_BYTES` / [ADR 013](../../../docs/adr/013-configurable-ring-buffer-size.md)) |
| `statix` | host | loader, attribution, aggregator, output; **`:9091/metrics`** ([ADR 022](../../../docs/adr/022-bpf-ring-buffer-drop-counter.md), [ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| `statix-gateway` | host | `clickhouse_writer` RowBinary coalescer; `MetricRow::from_ingest`; ingest + read API; probes ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md), [ADR 029](../../../docs/adr/029-ready-channel-depth-gate.md), [055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md), [056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)) |
| `statix-infra` | lib | `read_env_positive` (via `read_env_u64`/`read_env_usize`), clock helpers ([ADR 035](../../../docs/adr/035-phase7-workspace-restructure.md), [048](../../../docs/adr/048-generic-env-positive-parsing.md)) |

**Infra:** `docker-compose.yml` (ClickHouse, Grafana `:3001`, gateway), `deploy/docker/`, `deploy/k8s/`, `deploy/clickhouse/01_init.sql` ([ADR 057](../../../docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md))

Modules: see [REFERENCE.md](REFERENCE.md).

## Shared memory contract

Ring record: **`StatixEvent`** (64 bytes) with `kind`:

- `EVENT_KIND_WORKLOAD_IDENTITY` (1) — exec via `sched:sched_process_exec`
- `EVENT_KIND_MEMORY_SAMPLE` (2) — user-space `memory.current` sampler

## Latency contract (non-negotiable)

| Layer | Rule |
|-------|------|
| Ring buffer loop | No `.await` on HTTP ingest or blocking I/O |
| `emit_batch` | Serialize + `try_send` to retry worker; on full queue, `try_append` to disk WAL spillway (Phase 11, [ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md)) then last-resort sync drop-oldest; backoff + jitter; 0–5s recovery jitter after outage ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md), [042](../../../docs/adr/phase55/v2/042-phase55-v2-p2-sprint-l8-fixes.md)) |
| Disk WAL | Hot path `try_append` (non-blocking) → dedicated `statix-wal-writer` thread (`fdatasync` group-commit); never disk I/O on the ring-buffer loop ([PLAYBOOK](PHASE_11_WAL_PLAYBOOK.md)) |
| `POST /ingest` | `schema_version` 2 or 3 or `400` ([ADR 020](../../../docs/adr/020-ingest-schema-version-window.md)); Tier 1 `!ch_healthy`→503; Tier 2 `try_reserve_many`→503; 2MB body limit ([ADR 052](../../../docs/adr/phase55/v3/052-phase55-v3-wave4-perf-observability.md)) |
| ClickHouse writer | Background task only — `MetricRow` coalescer; RowBinary micro-batches; sync `insert.end()` ACK ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md), [056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)) |
| Aggregator | Early flush at `max_keys`; flip buffer before drain; BPF timestamp + atomic `clock_offset_ns()` for windows ([ADR 016](../../../docs/adr/016-clock-domain-offset.md), [047](../../../docs/adr/047-atomic-clock-offset-recalibration.md)) |
| Memory sample | Async sampler; cgroupfs via `spawn_blocking` + stack `[u8; 32]`; precomputed paths |
| CPU sample | Same tick: `cpu.stat` cumulative counter → delta in `Sampler.cpu_baseline`; prime first read ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)) |
| Saturation metrics | Gateway: `statix_gateway_mpsc_depth` (background sampler, `STATIX_MPSC_DEPTH_SAMPLE_MS`), `statix_api_ingest_503_total` (flat 503 counter); agent: `statix_wal_bytes_current` seeded at `init_wal` ([ADR 060](../../../docs/adr/phase10/060-phase10-golden-signal-saturation-metrics.md)) |

Full principles: [docs/guides/enterprise-latency.md](../../../docs/guides/enterprise-latency.md)

## BPF verifier

- No `?` after `EVENTS.reserve`
- No `bpf_trace_printk`
- `submit(wakeup_flag)` — `BPF_RB_NO_WAKEUP` (flag `1`) on 63/64 events; 5ms poll drain in agent ([ADR 053](../../../docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md))
- `cgroup_id` from `bpf_get_current_cgroup_id()` on identity events
- **CI matrix (BTF-era only):** Linux **5.10, 5.15, 6.1, 6.8** (mainline LTS tips, not `.0`) — [ADR 037](../../../docs/adr/037-phase9-ebpf-verifier-ci.md); `.github/workflows/ebpf-ci.yml`; `scripts/verify-ebpf-kernel.sh` + `statix-ebpf-verify`
- **5.10 memlock:** `bpf_memlock::bump_memlock_rlimit()` before `Ebpf::load()` — pre-5.11 kernels default 64 KiB `RLIMIT_MEMLOCK`; 512 KiB ringbuf needs infinity bump
- **Attribution hot path (V2):** `on_identity_event` procfs skip when cgroup known; K8s label merge outside write lock ([ADR 039](../../../docs/adr/phase55/v2/039-phase55-v2-wave2-l8-fixes.md))
- **Kafka routing (V2, historical):** removed with `kafka.rs` ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md); was [ADR 039](../../../docs/adr/phase55/v2/039-phase55-v2-wave2-l8-fixes.md))
- **Kafka durability (V2, historical):** removed with `kafka.rs` ([ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md); was [ADR 040](../../../docs/adr/phase55/v2/040-phase55-v2-wave3-l8-fixes.md))
- **K8s eviction (V2):** agent + gateway `preStop sleep 5` + `terminationGracePeriodSeconds: 30`; gateway PDB `minAvailable: 1` ([ADR 040](../../../docs/adr/phase55/v2/040-phase55-v2-wave3-l8-fixes.md))
- **K8s labels (V2):** `watch_k8s_pods` — `kube::runtime::watcher` with node field selector; no 30s list poll ([ADR 041](../../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md))
- **K8s deploy (V2):** digest-pinned images; gateway cross-AZ `topologySpreadConstraints` ([ADR 041](../../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md))

## User-space (Phase 2)

- Batched JSON `schema_version: 3` (agent emit); gateway accepts `2..=3`; `batch_id` + `agent_version` per flush ([ADR 017](../../../docs/adr/017-batch-lineage-metadata.md), [020](../../../docs/adr/020-ingest-schema-version-window.md), [058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md))
- `STATIX_RAW_EVENTS=1` debug only
- K8s: `tokio::spawn` + `watch_k8s_pods` stream — never `await` API in main `select!` ([ADR 041](../../../docs/adr/phase55/v2/041-phase55-v2-wave4-l8-fixes.md))
- Startup: `bootstrap_existing_cgroups` before event loop ([ADR 015](../../../docs/adr/015-cgroup-v2-bootstrap-on-startup.md))
- Memory: precomputed `{CGROUP_ROOT}/…/memory.current`
- Env: `STATIX_WINDOW_SECS`, `STATIX_SAMPLE_INTERVAL_SECS`, `STATIX_NODE_NAME`, `STATIX_CGROUP_ROOT`

### Hot-path heap discipline

| Avoid | Use |
|-------|-----|
| `read_to_string` on `memory.current` or `/proc/{pid}/cgroup` | `File::read` into stack buffer (`[u8; 32]` / `[u8; 1024]`) |
| `PathBuf::join` / `to_path_buf` per sample tick | Precompute `Arc<PathBuf>` on identity; sampler clones `Arc` only |
| `Vec` of all cgroup IDs per tick | `for_each_sample_target` (memory + cpu.stat) |
| `HashMap` for `cgroup_id` | `FxHashMap` ([ADR 001](../../../docs/adr/001-use-rustc-hash-for-latency.md)) |

### Aggregator

| Rule | Detail |
|------|--------|
| Map | `rustc_hash::FxHashMap` |
| Buffers | Two maps; flip before drain ([ADR 004](../../../docs/adr/004-swap-buffer-before-drain.md)) |
| Cap | Early flush — never random eviction ([ADR 003](../../../docs/adr/003-early-flush-instead-of-cap-eviction.md)) |
| Clock | `AtomicU64` offset in `statix-infra::clock`; hot-path `Relaxed` load; hourly recalibration task ([ADR 016](../../../docs/adr/016-clock-domain-offset.md), [047](../../../docs/adr/047-atomic-clock-offset-recalibration.md)) |

### Attribution

| Rule | Detail |
|------|--------|
| Locks | `parking_lot::RwLock`; **procfs before `write()`** on identity ([ADR 023](../../../docs/adr/023-phase5-hot-path-fixes.md)) |
| Labels | `DEFAULT_LABELS` `LazyLock`; cache K8s/path merges in `cgroup_labels` |
| cgroup v2 | `split_once("::")` not `split_once(':')` |
| Paths | `Path::components()` — no full-path `to_string_lossy()` |

## Ingest pipeline (Phase 13 — queue-less)

| Component | Rule |
|-----------|------|
| Agent | `init_http_client`; `init_retry_worker`; circuit breaker + WAL on 503 ([ADR 054](../../../docs/adr/phase11/054-phase11-wal-spillway.md), [006](../../../docs/adr/006-shared-http-client-for-ingest.md)) |
| API | `GET /health` (liveness); `GET /ready` = `ch_healthy` + mpsc &lt;80% ([ADR 021](../../../docs/adr/021-ingest-ready-probe.md), [ADR 029](../../../docs/adr/029-ready-channel-depth-gate.md), [055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)); `POST /ingest` Tier 1/2 503; `statix_api_ingest_lag_seconds` |
| Writer | `clickhouse_writer.rs` — `MetricRow` coalescer → RowBinary INSERT; `MetricRow::from_ingest` in handler ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)); `STATIX_CH_*` env; no `async_insert` |
| CH storage | `statix.workload_metrics`; `ReplacingMergeTree(window_end_ns)`; `INDEX cgroup_idx` minmax on `cgroup_id`; billing `FINAL`; init [deploy/clickhouse/01_init.sql](../../../deploy/clickhouse/01_init.sql) ([ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md), [026](../../../docs/adr/026-clickhouse-finops-database-init.md), [059](../../../docs/adr/phase10/059-phase10-clickhouse-cgroup-skip-index.md)) |
| Prod deploy | `deploy/docker/Dockerfile.{gateway,agent}`; `deploy/k8s/*.yaml` ([ADR 024](../../../docs/adr/024-agent-production-container.md), [025](../../../docs/adr/025-kubernetes-gateway-and-agent.md)) |

Spec: [docs/guides/phase3-ingest-interface.md](../../../docs/guides/phase3-ingest-interface.md)  
Validate: [docs/guides/phase3-validation.md](../../../docs/guides/phase3-validation.md)

## Build (always via Makefile)

```bash
make deps          # first time
make build         # ebpf + statix + statix-gateway
make check
make verify-phase14-cpu   # Phase 14 CPU gates
make verify-btf    # when BPF / kernel portability touched
# CI parity (needs KVM + virtme-ng): scripts/verify-ebpf-kernel.sh 5.15 statix-ebpf/target/.../statix-ebpf target/release/statix-ebpf-verify
make compose-up    # Dev stack (API in Docker on :3000); Phase 5: add STATIX_API_TOKEN in prod
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent on host (root)
make compose-down  # tear down stack
# Host-only API dev (not with compose-up): make run-api
# After gateway code changes in Docker: docker compose build statix-gateway && docker compose up -d statix-gateway
# After CH schema change: docker compose down -v && make compose-up
# Billing check: SELECT count() FROM statix.workload_metrics FINAL
curl -s http://127.0.0.1:3000/metrics | grep statix_api_
curl -s http://127.0.0.1:3000/metrics | grep -E 'statix_gateway_mpsc_depth|statix_api_ingest_503_total'
curl -s http://127.0.0.1:9091/metrics | grep statix_ring_drops   # agent (root)
curl -s http://127.0.0.1:9091/metrics | grep statix_wal_bytes_current
```

Observability: [docs/guides/observability-metrics.md](../../../docs/guides/observability-metrics.md) · Phase 10 playbook: [PHASE_10_SRE_PLAYBOOK.md](PHASE_10_SRE_PLAYBOOK.md)

Phase 2 validation: [docs/guides/phase2-validation.md](../../../docs/guides/phase2-validation.md)  
ADRs: [docs/adr/](../../../docs/adr/)  
Deferred: [TODO.md](TODO.md)

## L8 Audit Fixes (Phase 5.5)

**P0-SHIP shipped:** [ADR 032](../../../docs/adr/phase55/l8/032-phase55-l8-p0-hot-path-fixes.md) — agent hot path.

**P1-WEEK shipped:** [ADR 033](../../../docs/adr/phase55/l8/033-phase55-l8-p1-week-gateway-fixes.md) — `Bytes` retry body, Kafka producer alloc fixes, cached `kube::Client`, metadata refresh, `argMax` summary query.

**P2-SPRINT shipped (historical):** [ADR 034](../../../docs/adr/phase55/l8/034-phase55-l8-p2-ingest-zero-copy.md) — superseded by gateway `MetricRow` path ([ADR 056](../../../docs/adr/phase13/056-phase13-part2-ingest-zero-alloc.md)).

**L8 playbook:** [L8-AUDIT-FIXES.md](L8-AUDIT-FIXES.md) — all fixes shipped (ADR index).

**L8 V2 playbook:** [L8_AUDIT_V2_FIXES.md](L8_AUDIT_V2_FIXES.md) — all V2 items shipped for GA ([ADR 038](../../../docs/adr/phase55/v2/038-phase55-v2-wave1-l8-fixes.md)–[042](../../../docs/adr/phase55/v2/042-phase55-v2-p2-sprint-l8-fixes.md)).

**L8/L9 V3 playbook:** [L8_POST_GA_FIXES.md](L8_POST_GA_FIXES.md) — all V3 waves shipped ([ADR 049](../../../docs/adr/phase55/v3/049-phase55-v3-wave1-silent-deaths.md)–[053](../../../docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md)).

**Phase 13 playbooks:** [PHASE_13_PART1_PLAYBOOK.md](PHASE_13_PART1_PLAYBOOK.md) · [PHASE_13_PART2_PLAYBOOK.md](PHASE_13_PART2_PLAYBOOK.md) — [ADR 055](../../../docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)–[057](../../../docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md).

**Phase 14 playbook:** [PHASE_14_CPU_PLAYBOOK.md](PHASE_14_CPU_PLAYBOOK.md) — shipped ([ADR 058](../../../docs/adr/phase14/058-phase14-cpu-usage-tracking.md)).

## OOM-safe remediation (Phases 4–5)

```
requests = p99 × 1.20
limits   = requests × 1.25
```

See Pattern 8 in [PATTERNS.md](PATTERNS.md).

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Statix is an eBPF workload-telemetry platform. A kernel BPF program captures
process-exec identity events; a host agent attributes them to cgroups (+ K8s
labels), samples cgroup `memory.current` and `cpu.stat`, rolls up time windows,
and emits batched JSON. A gateway ingests batches, coalesces them into
ClickHouse RowBinary inserts, and serves a read API.

Data flow (queue-less since Phase 13 — **there is no Kafka**):
`sched:sched_process_exec` → BPF ring buffer → agent (attribute + aggregate)
→ `POST /ingest` (JSON, schema v3) → gateway `MetricRow` mpsc + coalescer
→ RowBinary `INSERT` → ClickHouse `statix.workload_metrics`
→ `GET /api/v1/workloads/summary`.

When the agent cannot reach the gateway, batches spill to a bounded local-disk
WAL and replay on recovery (Phase 11).

## Read this first

Before editing any crate, read `.cursor/skills/statix-ebpf-agent/SKILL.md`
(then `REFERENCE.md`, `PATTERNS.md`). It is the source of truth for conventions.
**Every architectural change must, in the same PR:** add an ADR under
`docs/adr/<topic>/` (numbering is global-sequential and the number is the ADR's
permanent identity — highest is `065`, so the next is `066`). ADRs are filed by
topic: `ebpf/ agent/ ingest/ gateway/ storage/ observability/ ui/ deploy/
fixes/ meta/ kafka-legacy/` — see [`docs/adr/INDEX.md`](docs/adr/INDEX.md).
Audit/fix waves go in `fixes/` because they cross-cut by nature. Also update
README/relevant `docs/guides/*`, and the skill files (`SKILL.md`/
`REFERENCE.md`/`PATTERNS.md`/`TODO.md`). This is a hard project rule, not a
suggestion.

Anything Kafka-shaped in `docs/adr/kafka-legacy/` or the older skill playbooks is
**historical**. Do not reintroduce it.

There is **one** gateway Dockerfile, `deploy/docker/Dockerfile.gateway`; Compose
builds from it too ([ADR 065](docs/adr/deploy/065-single-gateway-dockerfile.md)).
Do not add a separate dev copy.

## Build / check / run (always via Makefile)

```bash
./scripts/bootstrap.sh  # bare Linux machine: installs make, then runs `make deps`
make deps          # toolchain: clang/llvm, rust stable+nightly+rust-src, rustfmt, pinned bpf-linker
make deps-check    # read-only: verifies tools, BTF, cgroup v2
make build         # ebpf (3 ELF variants) + statix agent + statix-gateway
make check         # cargo check across all crates incl. nightly BPF check
make verify-btf    # when BPF or kernel portability is touched
make fmt           # cargo fmt (host) + cargo +nightly fmt (ebpf)
make enterprise-check   # build + check gate before claiming a change is done
```

Dev pipeline (ClickHouse + Grafana + gateway in Docker, agent on host):

```bash
cp .env.example .env          # set CLICKHOUSE_PASSWORD; never commit .env
make compose-up               # frees :3000, starts stack, health-checks gateway
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run              # agent (needs root / CAP_BPF+CAP_PERFMON)
make compose-down             # tear down
```

Compose services: `clickhouse` (:8123/:9000), `grafana` (:3001), `statix-gateway`
(:3000). `make run` **already posts to ingest** — the Makefile defaults
`STATIX_INGEST_URL` to `http://127.0.0.1:3000/ingest`. For true stdout-only mode,
run the binary with the variable genuinely unset:
`sudo RUST_LOG=info STATIX_BPF_DIR=target/bpf ./target/release/statix`.

`make run-api` (alias of `run-gateway`) is host-only gateway dev and must NOT be
combined with `compose-up` (port :3000 conflict). After gateway code changes in
Docker: `docker compose build statix-gateway && docker compose up -d statix-gateway`.
After a CH schema change: `docker compose down -v && make compose-up`.

**Host is macOS (arm64); eBPF builds and the agent only run on Linux.** Build and
run inside the Colima/Lima Ubuntu VM — `make build`, `make check`'s nightly BPF
leg, and `make run` will all fail on the Mac host.

## Tests

Standard host crates run under the root workspace:

```bash
cargo test -p statix-gateway              # also: statix, statix-wire, statix-infra
cargo test -p statix-gateway <test_name>  # single test by name
cargo test -p statix -- --nocapture       # show stdout
make wal-test                             # WAL integrity suite (cargo test -p statix wal)
make wal-faultfs                          # WAL ENOSPC drill on a tmpfs (root, Linux)
make verify-phase14-cpu                   # CPU priming/conservation/soft-miss gates
```

CI (`.github/workflows/ebpf-ci.yml`) runs `cargo check --workspace` + tests for
`statix-gateway`, `statix`, `statix-wire`, then a BPF verifier matrix on kernels
5.10/5.15/6.1/6.8 via virtme-ng (`scripts/verify-ebpf-kernel.sh` loading the
`statix-ebpf-verify` binary). Only BTF-era kernels are supported.

## Workspace layout (the BPF crate is special)

`Cargo.toml` workspace = host crates only: `statix-common`, `statix-wire`,
`statix-infra`, `statix`, `statix-gateway`. **`statix-ebpf` is intentionally
excluded** — it compiles to `bpfel-unknown-none` (BPF bytecode), so a root
`cargo build` does NOT build it. Build/check it via the Makefile, or directly
with `cargo +nightly ... -Z build-std=core --target bpfel-unknown-none` inside
`statix-ebpf/` (it has its own `target/`). All crates are version `1.2.0`.

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `statix-common` | host + bpf | `StatixEvent` (64-byte ring record) + kind constants — define event layout ONLY here; `user` feature adds `aya::Pod` |
| `statix-wire` | host | wire/ingest types: `IngestBatch`, `WorkloadRow` (`cpu_usage_usec` is `#[serde(default)]` for v2 compat) |
| `statix-infra` | host | `env::var` (+ legacy `FINOPS_*` fallback), `read_env_u64`/`read_env_usize` (reject ≤ 0), `clock::wall_unix_ns` |
| `statix-ebpf` | bpf | tracepoint, `cgroup_id`, ring buffer (size via `STATIX_RING_BUF_BYTES`) |
| `statix` | host | agent: loader, attribution, aggregator, samplers, WAL, output; metrics on `:9091` |
| `statix-gateway` | host | `Config::from_env()`, ingest→ClickHouse writer, read path, health/ready/metrics on `:3000` |

Agent module map (`statix/src/`): `loader.rs` (load ELF, attach tracepoint,
ring buffer + `RING_DROPS` monitor), `ebpf_select.rs` (CPU-tier ELF pick),
`bpf_memlock.rs` (pre-5.11 `RLIMIT_MEMLOCK` bump), `attribution/` (cgroup_id→path
via procfs, cgroupfs readers, K8s pod watcher), `aggregator.rs` (double-buffered
FxHashMap rollups), `memory_sampler.rs` (`memory.current` + `cpu.stat` polling),
`output.rs` (JSON batch, HTTP retry worker, WAL wiring), `wal/` (`mod.rs` store +
circuit breaker, `writer.rs` thread, `drainer.rs` replay, `recovery.rs` boot
repair, `segment.rs` frame codec), `bin/verify_ebpf.rs` (`statix-ebpf-verify`).
Gateway (`statix-gateway/src/`): `main.rs` (router, probes, graceful drain),
`config.rs`, `clickhouse_writer.rs`, `routes/ingest.rs`, `routes/query.rs`,
`error.rs`.

## eBPF build specifics

One BPF source produces three ELFs via the compile-time env
`STATIX_RING_BUF_BYTES` (see `statix-ebpf/build.rs`): `statix-ebpf-small`
(512 KiB, ≤8 CPUs), `-large` (4 MiB, 9–64), `-xlarge` (8 MiB, 65+), dropped in
`target/bpf/`. The agent auto-selects by CPU count; override with
`STATIX_EBF_PATH` or point `STATIX_BPF_DIR` at the bundle. Pre-5.11 kernels need
`bpf_memlock::bump_memlock_rlimit()` before load (default 64 KiB RLIMIT_MEMLOCK
is too small for the ring buffer).

## Non-negotiable hot-path latency contract

The ring-buffer drain path and `emit_batch` must never block. Concretely:

- No `.await` on HTTP/blocking I/O in the ring-buffer loop. Two drain arms —
  `AsyncFd::readable_mut()` and a 5 ms `poll_interval` fallback — each with a
  `DRAIN_BUDGET` of 256.
- `emit_batch` serializes + `try_send`s to the retry worker (capacity 60). On a
  full queue it `try_append`s to the disk WAL (non-blocking `try_send` to the
  WAL writer thread); only if the WAL is disabled/full does it drop oldest
  synchronously — never spawn on the hot path. Retries use backoff + 30% jitter,
  plus a node-hashed 0–35 s recovery spread to avoid a post-outage stampede.
- Aggregator uses `rustc_hash::FxHashMap`, double-buffered (flip before drain),
  and **early-flushes at `max_keys`** (4096) — never random/cap eviction.
- Window bounds come from `wall_unix_ns()`, read directly in `flush` — one reading
  per flush, passed to `reset_window`, so one window ends exactly where the next
  begins. There is no cached monotonic→wall offset: it was removed in
  [ADR 063](docs/adr/agent/063-wall-clock-window-bounds.md) because it went stale
  on any host pause (laptop sleep, hypervisor pause, live migration), stamping
  rows minutes or hours in the past.
- cgroupfs / procfs reads use stack buffers + precomputed `Arc<PathBuf>` paths,
  via `spawn_blocking` — never `read_to_string` or per-tick `PathBuf::join`.
- K8s pod labels are watched on a background `tokio::spawn` stream (node field
  selector) — never `await` the kube API inside the main `select!`.

BPF verifier rules: no `?` after `EVENTS.reserve` (increment `RING_DROPS` on
fail), no `bpf_trace_printk`, `submit` with `BPF_RB_NO_WAKEUP` on 63 of every
64 events.

## Agent WAL spillway (Phase 11)

Bounded, segmented, append-only log at `STATIX_WAL_DIR` (default
`/var/lib/statix/wal`). Hot path never touches disk: `try_append` hands the
payload to a dedicated writer thread that group-commits with `fdatasync`
(`STATIX_WAL_FSYNC_FRAMES` / `_INTERVAL_MS`). Segments rotate at
`STATIX_WAL_SEGMENT_BYTES`; at `STATIX_WAL_MAX_BYTES` the oldest segment is
dropped (FIFO, metered — bounded loss, never unbounded growth). Recovery
self-heals torn tails via per-frame CRC32 at boot. A drainer replays
oldest-first through the normal retry path, gated by a circuit breaker
(Closed/HalfOpen/Open) driven by real POST outcomes — 3 consecutive retryable
failures trip it Open. Disable with `STATIX_WAL_ENABLED=0` (falls back to
drop-oldest).

## Gateway / storage notes

- `POST /ingest` accepts `schema_version` 2 or 3 (else 400), 2 MB body limit,
  optional bearer auth via `STATIX_API_TOKEN`. Two backpressure tiers, both 503:
  Tier 1 `!ch_healthy`, Tier 2 `try_reserve_many` on the bounded `MetricRow`
  mpsc (default 8192). The handler denormalizes the batch envelope into per-row
  `MetricRow`s — no intermediate allocation of a second batch type.
- `GET /ready` = ingest channel open AND ClickHouse healthy AND mpsc < 80% full;
  `GET /health` = channel open (liveness only).
- The ClickHouse writer is a background task: coalesce up to
  `STATIX_CH_BATCH_MAX` (1024) rows or `STATIX_CH_LINGER_MS` (50 ms), then
  RowBinary `INSERT` with a synchronous ACK (`STATIX_CH_INSERT_TIMEOUT_SECS`,
  default 3). Up to 5 retries with backoff + jitter, then drop + increment
  `statix_api_ch_insert_dropped_total`. On shutdown the writer drains with a
  10 s timeout. **Do not add `async_insert`** — the sync ACK is the
  stall-detection primitive.
- Storage is `ReplacingMergeTree(window_end_ns)`, `PARTITION BY` hour,
  `ORDER BY (node, window_start_ns, cgroup_id)`, 30-day TTL, minmax skip index
  on `cgroup_id`. Billing/dedup queries use `FINAL`; the operational read path
  (`/api/v1/workloads/summary`) deliberately does not. Schema/init:
  `deploy/clickhouse/01_init.sql`.

## Metrics

Agent `:9091/metrics`, gateway `:3000/metrics` (Prometheus). Key saturation
series: `statix_gateway_mpsc_depth`, `statix_api_ingest_503_total`,
`statix_wal_bytes_current` — all seeded to 0 at startup so idle hosts still
export them. Also `statix_ring_drops_total` (BPF ring overflow — always
investigate), `statix_api_ingest_lag_seconds`, `statix_api_ch_insert_*`,
`statix_wal_*`. Details: `docs/guides/observability-metrics.md`.

Env vars and prod deploy (Docker/K8s) are documented in `README.md` and
`deploy/*/README.md`; don't duplicate that table here.

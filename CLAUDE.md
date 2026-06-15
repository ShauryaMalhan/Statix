# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Statix is an eBPF workload-telemetry platform. A kernel BPF program captures
process-exec identity events; a host agent attributes them to cgroups (+ K8s
labels), samples cgroup `memory.current`, rolls up time windows, and emits
batched JSON. A gateway ingests batches to Kafka; ClickHouse's Kafka engine
loads them; a read API serves summaries.

Data flow:
`sched:sched_process_exec` → BPF ring buffer → agent (attribute + aggregate)
→ `POST /ingest` → Kafka (`statix-telemetry`, keyed by node) → ClickHouse
`statix.workload_metrics` → `GET /api/v1/workloads/summary`.

## Read this first

Before editing any crate, read `.cursor/skills/statix-ebpf-agent/SKILL.md`
(then `REFERENCE.md`, `PATTERNS.md`). It is the source of truth for conventions.
**Every architectural change must, in the same PR:** add an ADR under
`docs/adr/` (current wave: `docs/adr/phase55/v3/`), update README/relevant
`docs/guides/*`, and update the skill files (`SKILL.md`/`REFERENCE.md`/
`PATTERNS.md`/`TODO.md`). This is a hard project rule, not a suggestion.

## Build / check / run (always via Makefile)

```bash
make deps          # one-time: nightly toolchain, rust-src, bpf-linker, clang
make build         # ebpf (3 ELF variants) + statix agent + statix-gateway
make check         # cargo check across all crates incl. nightly BPF check
make verify-btf    # when BPF or kernel portability is touched
make fmt           # cargo fmt (host) + cargo +nightly fmt (ebpf)
```

Dev pipeline (Kafka + ClickHouse + gateway in Docker, agent on host):

```bash
cp .env.example .env          # set CLICKHOUSE_PASSWORD; never commit .env
make compose-up               # frees :3000, starts stack, health-checks gateway
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run              # agent (needs root / CAP_BPF+CAP_PERFMON)
make compose-down             # tear down
```

`make run` alone = Phase 2 stdout-only (no ingest). `make run-api` is host-only
gateway dev and must NOT be combined with `compose-up` (port :3000 conflict).
After gateway code changes in Docker: `docker compose build statix-gateway &&
docker compose up -d statix-gateway`. After a CH schema change:
`docker compose down -v && make compose-up`.

## Tests

Standard host crates run under the root workspace:

```bash
cargo test -p statix-gateway              # also: statix, statix-wire
cargo test -p statix-gateway <test_name>  # single test by name
cargo test -p statix -- --nocapture       # show stdout
```

CI (`.github/workflows/ebpf-ci.yml`) runs `cargo check --workspace` + tests for
`statix-gateway`, `statix`, `statix-wire`, then a BPF verifier matrix on kernels
5.10/5.15/6.1/6.8 via virtme-ng. Only BTF-era kernels are supported.

## Workspace layout (the BPF crate is special)

`Cargo.toml` workspace = host crates only: `statix-common`, `statix-wire`,
`statix-infra`, `statix`, `statix-gateway`. **`statix-ebpf` is intentionally
excluded** — it compiles to `bpfel-unknown-none` (BPF bytecode), so a root
`cargo build` does NOT build it. Build/check it via the Makefile, or directly
with `cargo +nightly ... -Z build-std=core --target bpfel-unknown-none` inside
`statix-ebpf/` (it has its own `target/`).

| Crate | Target | Responsibility |
|-------|--------|----------------|
| `statix-common` | host + bpf | `StatixEvent` (64-byte ring record) + kind constants — define event layout ONLY here |
| `statix-wire` | host | wire/ingest types: `IngestBatch`, `WorkloadRow` |
| `statix-infra` | host | `read_env_*` helpers, clock-offset utilities |
| `statix-ebpf` | bpf | tracepoint, `cgroup_id`, ring buffer (size via `STATIX_RING_BUF_BYTES`) |
| `statix` | host | agent: loader, attribution, aggregator, memory sampler, output; metrics on `:9091` |
| `statix-gateway` | host | `Config::from_env()`, ingest→Kafka, ClickHouse read path, health/ready/metrics on `:3000` |

Agent module map (`statix/src/`): `loader.rs` (load ELF, attach tracepoint,
drain ring buffer), `attribution/` (cgroup_id→path via procfs + K8s labels),
`aggregator.rs` (windowed FxHashMap rollups), `memory_sampler.rs`
(`memory.current` polling), `output.rs` (JSON batch + HTTP retry worker),
`ebpf_select.rs` (CPU-tier ELF pick). Gateway (`statix-gateway/src/`):
`routes/ingest.rs`, `routes/query.rs`, `kafka.rs`, `config.rs`, `error.rs`.

## eBPF build specifics

One BPF source produces three ELFs via the compile-time env
`STATIX_RING_BUF_BYTES` (see `statix-ebpf/build.rs`): `statix-ebpf-small`
(512 KiB), `-large` (4 MiB), `-xlarge` (8 MiB), dropped in `target/bpf/`. The
agent auto-selects by CPU count; override with `STATIX_EBF_PATH` or point
`STATIX_BPF_DIR` at the bundle. Pre-5.11 kernels need
`bpf_memlock::bump_memlock_rlimit()` before load (default 64 KiB RLIMIT_MEMLOCK
is too small for the ring buffer).

## Non-negotiable hot-path latency contract

The ring-buffer drain path and `emit_batch` must never block. Concretely:

- No `.await` on HTTP/blocking I/O in the ring-buffer loop; drain budget is 256.
- `emit_batch` serializes + `try_send`s to the retry worker; on a full queue it
  drops oldest synchronously (no spawn) with backoff+jitter.
- Aggregator uses `rustc_hash::FxHashMap`, double-buffered (flip before drain),
  and **early-flushes at `max_keys`** — never random/cap eviction.
- Window times come from the BPF monotonic timestamp + an atomic
  `clock_offset_ns()` (hourly recalibration), not wall-clock syscalls per event.
- cgroupfs / procfs reads use stack buffers + precomputed `Arc<PathBuf>` paths,
  via `spawn_blocking` — never `read_to_string` or per-tick `PathBuf::join`.
- K8s pod labels are watched on a background `tokio::spawn` stream — never
  `await` the kube API inside the main `select!`.

BPF verifier rules: no `?` after `EVENTS.reserve` (increment `RING_DROPS` on
fail), no `bpf_trace_printk`, `submit` with `BPF_RB_NO_WAKEUP` on 63/64 events.

## Gateway / storage notes

- `POST /ingest` accepts `schema_version` 2 or 3 (else 400), `try_send`s to a
  bounded mpsc (200/`503` on full), 2 MB body limit, optional bearer auth via
  `STATIX_API_TOKEN`.
- `GET /ready` = Kafka connected AND ingest mpsc < 80% full; `GET /health` =
  channel open. Kafka producer micro-batches (linger + max batch, configurable).
- There is **no Rust Kafka consumer** — ClickHouse's Kafka engine table loads
  rows. Schema/init: `deploy/clickhouse/01_init.sql`. Storage is
  `ReplacingMergeTree`; billing/dedup queries use `FINAL`.

Env vars and prod deploy (Docker/K8s) are documented in `README.md` and
`deploy/*/README.md`; don't duplicate that table here.

# Statix eBPF Platform

Kernel-side workload identity + cgroup memory telemetry, rolled up in user space and emitted as batched JSON.

## Status

| Phase / target | State | What ships |
|----------------|--------|------------|
| **1–4** | Done | eBPF agent, attribution, ingest E2E, scale/reliability (partition routing, dedupe, lineage, bootstrap) |
| **5** | **Partial** | P0 security/hot-path + TLS at ALB shipped; Kafka ops cancelled Phase 13 ([ADR 055](docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)) |
| **5.5 V3** | Done | Post-GA audit ([ADR 049](docs/adr/phase55/v3/049-phase55-v3-wave1-silent-deaths.md)–[053](docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md)) |
| **11** | Done | Agent WAL + circuit breaker ([ADR 054](docs/adr/phase11/054-phase11-wal-spillway.md)) |
| **13** | **Done** | Queue-less RowBinary + `MetricRow` + infra strip ([ADR 055](docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)–[057](docs/adr/phase13/057-phase13-part2-infra-kafka-strip.md)) |
| **6** | Done | Mechanical sympathy / hot-path micro-opts ([ADR 018](docs/adr/018-phase-roadmap-status.md)) |
| **7** | Done | `statix-wire`, `statix-infra`, typed errors, read-only labels ([ADR 028](docs/adr/028-finops-wire-and-agent-rename.md)–[036](docs/adr/036-phase7-typed-errors-labels-read-path.md)) |
| **8** | Partial | K8s manifests, informer, drain, digest pins shipped; stronger cgroup→pod mapping open |
| **9** | Partial | eBPF verifier CI matrix shipped ([ADR 037](docs/adr/037-phase9-ebpf-verifier-ci.md)); arm64 CI + cgroup v1 detection open |
| **T1–3** | Done | Prod deploy, CH init, `GET /api/v1/workloads/summary` ([ADR 024](docs/adr/024-agent-production-container.md)–[027](docs/adr/027-api-read-path-clickhouse.md)) |

## What’s in the repo

Five host crates + BPF + infra:

- **`statix-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`statix-common`** — shared event layout (`StatixEvent`, kinds, sizes)
- **`statix-wire`** — shared ingest types (`IngestBatch`, `WorkloadRow`)
- **`statix-infra`** — shared `read_env_*` and clock utilities ([ADR 035](docs/adr/035-phase7-workspace-restructure.md))
- **`statix`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, stdout or HTTP ingest
- **`statix-gateway`** — `POST /ingest` → RowBinary coalescer → ClickHouse; `GET /api/v1/workloads/summary`; probes
- **`docker-compose.yml`** — ClickHouse, Grafana, gateway (queue-less stack)

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups flushed to stdout or HTTP ingest

Phase 3+ ingest: HTTP JSON batches → gateway coalescer → RowBinary INSERT → `statix.workload_metrics` (billing: `FINAL`). Schema: [deploy/clickhouse/01_init.sql](deploy/clickhouse/01_init.sql). [ADR 055](docs/adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md).

**Enterprise low-latency contract:** [docs/guides/enterprise-latency.md](docs/guides/enterprise-latency.md)  
Design decisions (ADRs): [docs/adr/](docs/adr/)  
Contributing: read `.cursor/skills/statix-ebpf-agent/SKILL.md` first; update ADR + docs + skills with every architectural change.

## CI

[![eBPF Verifier CI](https://github.com/ShauryaMalhan/Statix/actions/workflows/ebpf-ci.yml/badge.svg)](https://github.com/ShauryaMalhan/Statix/actions/workflows/ebpf-ci.yml)

On every push/PR to `main`, [`.github/workflows/ebpf-ci.yml`](.github/workflows/ebpf-ci.yml) runs:

1. **Userspace** — `cargo check --workspace` + tests for `statix-gateway`, `statix`, `statix-wire`
2. **eBPF verifier matrix** — kernels **5.10, 5.15, 6.1, 6.8** via virtme-ng + `statix-ebpf-verify` ([ADR 037](docs/adr/037-phase9-ebpf-verifier-ci.md))

Pre-BTF / legacy kernels are **not** supported.

## Prerequisites

- Linux 5.10+ (CI matrix: 5.10, 5.15, 6.1, 6.8) with BTF (`/sys/kernel/btf/vmlinux`)
- cgroup v2 unified hierarchy
- **Rust:** stable (user agent) + nightly (eBPF)
- **Tools:** `clang`, `bpf-linker`, `bpftool` (optional, for `make verify-btf`)
- **Docker:** dev stack (`docker.io` + `docker-compose-v2`)
- **Privileges:** root or `CAP_BPF` + `CAP_PERFMON` to load programs

## Install & build

```bash
cd Statix   # repo root
make deps
make build
```

Binaries:

- eBPF bundle: `target/bpf/statix-ebpf-{small,large,xlarge}` (auto-selected by CPU count; override `STATIX_EBF_PATH`)
- Agent: `target/release/statix`
- Gateway: `target/release/statix-gateway`

## Run

**Phase 2 (stdout only):**

```bash
sudo RUST_LOG=info make run
```

**Ingest pipeline (dev):**

```bash
cp .env.example .env   # set CLICKHOUSE_PASSWORD locally (never commit .env)
make compose-up    # one command — frees :3000, starts stack, recreates API if needed
export STATIX_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent only (separate terminal)
```

Use **`make run-api`** only for host-only API dev (not with `compose-up`). Tear down: `make compose-down`.

Rebuild gateway image: `docker compose build statix-gateway && docker compose up -d statix-gateway`

| Variable | Default | Purpose |
|----------|---------|---------|
| `STATIX_INGEST_URL` | (unset) | HTTP ingest URL; unset = stdout |
| `STATIX_EBF_PATH` | (auto) | Override path to BPF ELF; else CPU-tier pick from `STATIX_BPF_DIR` (`target/bpf`) |
| `STATIX_BPF_DIR` | `target/bpf` | Directory with `statix-ebpf-{small,large,xlarge}` |
| `STATIX_WINDOW_SECS` | `10` | Aggregation flush interval (must be &gt; 0; invalid → default) |
| `STATIX_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval (must be &gt; 0; invalid → default) |
| `STATIX_NODE_NAME` | hostname | Node id in batches |
| `STATIX_HTTP_TIMEOUT_SECS` | `5` | Agent `reqwest` request timeout (entire POST) |
| `STATIX_HTTP_POOL_IDLE_SECS` | `55` | Agent connection pool idle timeout (&lt; ALB 60s default) |
| `STATIX_BACKOFF_INITIAL_SECS` | `1` | Agent retry base backoff (seconds) |
| `STATIX_BACKOFF_MAX_SECS` | `30` | Agent retry max backoff (seconds); 30% jitter on sleep |
| `STATIX_WAL_ENABLED` | `true` | Disk WAL spillway (Phase 11); `0/false/no/off` → legacy drop-oldest |
| `STATIX_WAL_DIR` | `/var/lib/statix/wal` | WAL segment directory (use a writable ephemeral volume) |
| `STATIX_WAL_MAX_BYTES` | `536870912` | WAL hard disk cap (512 MiB; ≥ one segment); drop-oldest at cap |
| `STATIX_WAL_SEGMENT_BYTES` | `8388608` | WAL segment rotation size (8 MiB) |
| `STATIX_WAL_FSYNC_FRAMES` | `64` | Max frames between `fdatasync` group-commits |
| `STATIX_WAL_FSYNC_INTERVAL_MS` | `200` | Max time between `fdatasync` group-commits |
| `STATIX_INGEST_CHANNEL_SIZE` | `8192` | Gateway ingest mpsc depth (min 1024) |
| `STATIX_CH_BATCH_MAX` | `1024` | Gateway RowBinary micro-batch size (64–16384) |
| `STATIX_CH_LINGER_MS` | `50` | Gateway coalesce linger ms (1–1000) |
| `STATIX_CH_INSERT_TIMEOUT_SECS` | `3` | Gateway insert ACK timeout (1–30; &lt; agent HTTP timeout) |
| `CLICKHOUSE_URL` | `http://localhost:8123` | Gateway read-path HTTP endpoint |
| `CLICKHOUSE_USER` | `default` | ClickHouse user |
| `CLICKHOUSE_PASSWORD` | (empty) | ClickHouse password (Compose: set in `.env` — copy from `.env.example`) |

## Validate

```bash
make check
make verify-btf
# Optional local verifier (KVM + virtme-ng): see scripts/verify-ebpf-kernel.sh
```

- Phase 2: [docs/guides/phase2-validation.md](docs/guides/phase2-validation.md)
- Phase 3: [docs/guides/phase3-validation.md](docs/guides/phase3-validation.md)

## Production deploy

```bash
docker build -f deploy/docker/Dockerfile.gateway -t statix-gateway:latest .
docker build -f deploy/docker/Dockerfile.statix -t statix:latest .
kubectl apply -f deploy/k8s/gateway.yaml -f deploy/k8s/statix-daemonset.yaml
```

See [deploy/docker/README.md](deploy/docker/README.md), [deploy/k8s/README.md](deploy/k8s/README.md), [deploy/clickhouse/README.md](deploy/clickhouse/README.md).

## Layout

```
Statix/
├── statix-ebpf/
├── statix-common/
├── statix-wire/
├── statix-infra/
├── statix/
├── statix-gateway/  # `src/config.rs` — gateway env
├── deploy/          # docker, k8s, clickhouse (prod)
├── docker-compose.yml
├── Dockerfile.gateway   # dev Compose gateway only
├── .github/workflows/ebpf-ci.yml
├── scripts/verify-ebpf-kernel.sh
├── docs/
├── Makefile
└── README.md
```

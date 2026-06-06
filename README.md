# Statix eBPF Platform

Kernel-side workload identity + cgroup memory telemetry, rolled up in user space and emitted as batched JSON.

## Status

| Phase | State | What ships |
|-------|--------|------------|
| **1** | Done | `sched:sched_process_exec` tracepoint, ring buffer, basic agent loop |
| **2** | Done | cgroup attribution, K8s labels (optional), `memory.current` sampling, `schema_version: 2` batches |
| **3** | Done | HTTP ingest → Kafka → ClickHouse ([spec](docs/phase3-ingest-interface.md)) |
| **4** | Done | Partition routing, retry/jitter, dedupe, Prometheus, ring tiers, clock offset, lineage, cgroup bootstrap |
| **5** | **Active** | TLS; prod CH/Kafka ops ([guide](docs/phase5-production-readiness.md)) — P0 security/hot-path shipped ([ADR 023](docs/adr/023-phase5-hot-path-fixes.md)) |
| **6** | Done | L8 hot path ([ADR 018](docs/adr/018-phase-roadmap-status.md), [ADR 023](docs/adr/023-phase5-hot-path-fixes.md)) |
| **T1–3** | Done | Prod deploy, CH init, `GET /api/v1/workloads/summary` ([ADR 024](docs/adr/024-agent-production-container.md)–[027](docs/adr/027-api-read-path-clickhouse.md)) |

## What’s in the repo

Five host crates + BPF + infra:

- **`statix-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`statix-common`** — shared event layout (`StatixEvent`, kinds, sizes)
- **`statix-wire`** — shared ingest types (`IngestBatch`, `WorkloadRow`, `FlatRow`)
- **`statix-infra`** — shared `read_env_*` and clock utilities ([ADR 035](docs/adr/035-phase7-workspace-restructure.md))
- **`statix`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, stdout or HTTP ingest
- **`statix-gateway`** — `POST /ingest` → Kafka; `GET /api/v1/workloads/summary` → ClickHouse; `GET /health`, `GET /ready`, `GET /metrics`
- **`docker-compose.yml`** — Kafka KRaft, Kafka UI, ClickHouse with Kafka engine table

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups flushed to stdout or HTTP ingest

Phase 3 adds HTTP ingest, keyed Kafka by `node`, ClickHouse Kafka engine → `statix.workload_metrics` (billing: `FINAL`). Schema: [deploy/clickhouse/01_init.sql](deploy/clickhouse/01_init.sql). Production packaging: [deploy/](deploy/).

**Enterprise low-latency contract:** [docs/enterprise-latency.md](docs/enterprise-latency.md)  
Design decisions (ADRs): [docs/adr/](docs/adr/)  
Contributing: read `.cursor/skills/statix-ebpf-agent/SKILL.md` first; update ADR + docs + skills with every architectural change.

## CI

[![eBPF Verifier CI](https://github.com/ShauryaMalhan/finops-core/actions/workflows/ebpf-ci.yml/badge.svg)](https://github.com/ShauryaMalhan/finops-core/actions/workflows/ebpf-ci.yml)

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
cd statix-core
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
| `STATIX_WINDOW_SECS` | `10` | Aggregation flush interval |
| `STATIX_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval |
| `STATIX_NODE_NAME` | hostname | Node id in batches |
| `STATIX_HTTP_TIMEOUT_SECS` | `5` | Agent `reqwest` request timeout (entire POST) |
| `STATIX_HTTP_POOL_IDLE_SECS` | `55` | Agent connection pool idle timeout (&lt; ALB 60s default) |
| `STATIX_BACKOFF_INITIAL_SECS` | `1` | Agent retry base backoff (seconds) |
| `STATIX_BACKOFF_MAX_SECS` | `30` | Agent retry max backoff (seconds); 30% jitter on sleep |
| `KAFKA_BROKERS` | `localhost:9092` | Gateway → Kafka (`statix-gateway/src/config.rs`) |
| `STATIX_API_PORT` | `3000` | Gateway listen port (invalid value exits at startup) |
| `STATIX_KAFKA_CHANNEL_SIZE` | `8192` | Gateway ingest mpsc depth (min 1024) |
| `STATIX_KAFKA_BATCH_MAX` | `1024` | Gateway Kafka micro-batch size (64–16384) |
| `STATIX_KAFKA_LINGER_MS` | `50` | Gateway partial-batch linger ms (1–1000) |
| `CLICKHOUSE_URL` | `http://localhost:8123` | Gateway read-path HTTP endpoint |
| `CLICKHOUSE_USER` | `default` | ClickHouse user |
| `CLICKHOUSE_PASSWORD` | (empty) | ClickHouse password (Compose: `statix_dev`) |

## Validate

```bash
make check
make verify-btf
# Optional local verifier (KVM + virtme-ng): see scripts/verify-ebpf-kernel.sh
```

- Phase 2: [docs/phase2-validation.md](docs/phase2-validation.md)
- Phase 3: [docs/phase3-validation.md](docs/phase3-validation.md)

## Production deploy

```bash
docker build -f deploy/docker/Dockerfile.gateway -t statix-gateway:latest .
docker build -f deploy/docker/Dockerfile.statix -t statix:latest .
kubectl apply -f deploy/k8s/gateway.yaml -f deploy/k8s/statix-daemonset.yaml
```

See [deploy/docker/README.md](deploy/docker/README.md), [deploy/k8s/README.md](deploy/k8s/README.md), [deploy/clickhouse/README.md](deploy/clickhouse/README.md).

## Layout

```
finops-core/
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

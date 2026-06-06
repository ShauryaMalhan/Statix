# FinOps eBPF Agent

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

- **`finops-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`finops-common`** — shared event layout (`FinopsEvent`, kinds, sizes)
- **`finops-wire`** — shared ingest types (`IngestBatch`, `WorkloadRow`, `FlatRow`)
- **`finops-infra`** — shared `read_env_*` and clock utilities ([ADR 035](docs/adr/035-phase7-workspace-restructure.md))
- **`finops-agent`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, stdout or HTTP ingest
- **`finops-gateway`** — `POST /ingest` → Kafka; `GET /api/v1/workloads/summary` → ClickHouse; `GET /health`, `GET /ready`, `GET /metrics`
- **`docker-compose.yml`** — Kafka KRaft, Kafka UI, ClickHouse with Kafka engine table

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups flushed to stdout or HTTP ingest

Phase 3 adds HTTP ingest, keyed Kafka by `node`, ClickHouse Kafka engine → `finops.workload_metrics` (billing: `FINAL`). Schema: [deploy/clickhouse/01_init.sql](deploy/clickhouse/01_init.sql). Production packaging: [deploy/](deploy/).

**Enterprise low-latency contract:** [docs/enterprise-latency.md](docs/enterprise-latency.md)  
Design decisions (ADRs): [docs/adr/](docs/adr/)  
Contributing: read `.cursor/skills/finops-ebpf-agent/SKILL.md` first; update ADR + docs + skills with every architectural change.

## Prerequisites

- Linux 5.8+ with BTF (`/sys/kernel/btf/vmlinux`)
- cgroup v2 unified hierarchy
- **Rust:** stable (user agent) + nightly (eBPF)
- **Tools:** `clang`, `bpf-linker`, `bpftool` (optional, for `make verify-btf`)
- **Docker:** dev stack (`docker.io` + `docker-compose-v2`)
- **Privileges:** root or `CAP_BPF` + `CAP_PERFMON` to load programs

## Install & build

```bash
cd finops-core
make deps
make build
```

Binaries:

- eBPF bundle: `target/bpf/finops-ebpf-{small,large,xlarge}` (auto-selected by CPU count; override `FINOPS_EBF_PATH`)
- Agent: `target/release/finops-agent`
- Gateway: `target/release/finops-gateway`

## Run

**Phase 2 (stdout only):**

```bash
sudo RUST_LOG=info make run
```

**Ingest pipeline (dev):**

```bash
make compose-up    # one command — frees :3000, starts stack, recreates API if needed
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent only (separate terminal)
```

Use **`make run-api`** only for host-only API dev (not with `compose-up`). Tear down: `make compose-down`.

Rebuild gateway image: `docker compose build finops-gateway && docker compose up -d finops-gateway`

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_INGEST_URL` | (unset) | HTTP ingest URL; unset = stdout |
| `FINOPS_EBF_PATH` | (auto) | Override path to BPF ELF; else CPU-tier pick from `FINOPS_BPF_DIR` (`target/bpf`) |
| `FINOPS_BPF_DIR` | `target/bpf` | Directory with `finops-ebpf-{small,large,xlarge}` |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation flush interval |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `FINOPS_HTTP_TIMEOUT_SECS` | `5` | Agent `reqwest` request timeout (entire POST) |
| `FINOPS_HTTP_POOL_IDLE_SECS` | `55` | Agent connection pool idle timeout (&lt; ALB 60s default) |
| `FINOPS_BACKOFF_INITIAL_SECS` | `1` | Agent retry base backoff (seconds) |
| `FINOPS_BACKOFF_MAX_SECS` | `30` | Agent retry max backoff (seconds); 30% jitter on sleep |
| `KAFKA_BROKERS` | `localhost:9092` | Gateway → Kafka (`finops-gateway/src/config.rs`) |
| `FINOPS_API_PORT` | `3000` | Gateway listen port (invalid value exits at startup) |
| `FINOPS_KAFKA_CHANNEL_SIZE` | `8192` | Gateway ingest mpsc depth (min 1024) |
| `FINOPS_KAFKA_BATCH_MAX` | `1024` | Gateway Kafka micro-batch size (64–16384) |
| `FINOPS_KAFKA_LINGER_MS` | `50` | Gateway partial-batch linger ms (1–1000) |
| `CLICKHOUSE_URL` | `http://localhost:8123` | Gateway read-path HTTP endpoint |
| `CLICKHOUSE_USER` | `default` | ClickHouse user |
| `CLICKHOUSE_PASSWORD` | (empty) | ClickHouse password (Compose: `finops_dev`) |

## Validate

```bash
make check
make verify-btf
```

- Phase 2: [docs/phase2-validation.md](docs/phase2-validation.md)
- Phase 3: [docs/phase3-validation.md](docs/phase3-validation.md)

## Production deploy

```bash
docker build -f deploy/docker/Dockerfile.gateway -t finops-gateway:latest .
docker build -f deploy/docker/Dockerfile.agent -t finops-agent:latest .
kubectl apply -f deploy/k8s/gateway.yaml -f deploy/k8s/agent-daemonset.yaml
```

See [deploy/docker/README.md](deploy/docker/README.md), [deploy/k8s/README.md](deploy/k8s/README.md), [deploy/clickhouse/README.md](deploy/clickhouse/README.md).

## Layout

```
finops-core/
├── finops-ebpf/
├── finops-common/
├── finops-wire/
├── finops-infra/
├── finops-agent/
├── finops-gateway/  # `src/config.rs` — gateway env
├── deploy/          # docker, k8s, clickhouse (prod)
├── docker-compose.yml
├── Dockerfile.gateway   # dev Compose gateway only
├── docs/
├── Makefile
└── README.md
```

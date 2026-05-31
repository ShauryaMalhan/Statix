# FinOps eBPF Agent

Kernel-side workload identity + cgroup memory telemetry, rolled up in user space and emitted as batched JSON.

## Status

| Phase | State | What ships |
|-------|--------|------------|
| **1** | Done | `sched:sched_process_exec` tracepoint, ring buffer, basic agent loop |
| **2** | **Done** | cgroup attribution, K8s labels (optional), `memory.current` sampling, `schema_version: 2` batches on stdout |
| **3** | **Done** | HTTP ingest API → Kafka → ClickHouse ([spec](docs/phase3-ingest-interface.md)) |

## What’s in the repo

Four crates + infra:

- **`finops-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`finops-common`** — shared event layout (`FinopsEvent`, kinds, sizes)
- **`finops-user`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, stdout or HTTP ingest
- **`finops-api`** — `POST /ingest` → Kafka (`mpsc` + background producer)
- **`docker-compose.yml`** — Kafka KRaft, Kafka UI, ClickHouse with Kafka engine table

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups flushed to stdout or HTTP ingest

Phase 3 adds fire-and-forget `POST` to `finops-api` (3s HTTP timeout), one Kafka JSON row per workload, ClickHouse ingestion via Kafka engine + materialized view (skip broken messages; tune `kafka_num_consumers` to partition count in prod).

**Enterprise low-latency contract:** [docs/enterprise-latency.md](docs/enterprise-latency.md)  
Design decisions (ADRs): [docs/adr/](docs/adr/)  
Contributing: read `.cursor/skills/finops-ebpf-agent/SKILL.md` first; update ADR + docs + skills with every architectural change.

## Prerequisites

- Linux 5.8+ with BTF (`/sys/kernel/btf/vmlinux`)
- cgroup v2 unified hierarchy
- **Rust:** stable (user agent) + nightly (eBPF)
- **Tools:** `clang`, `bpf-linker`, `bpftool` (optional, for `make verify-btf`)
- **Docker:** Phase 3 (`docker.io` + `docker-compose-v2`)
- **Privileges:** root or `CAP_BPF` + `CAP_PERFMON` to load programs

## Install & build

```bash
cd finops-core
make deps
make build
```

Binaries:

- eBPF: `finops-ebpf/target/bpfel-unknown-none/release/finops-ebpf`
- Agent: `target/release/finops-user`
- API: `target/release/finops-api`

## Run

**Phase 2 (stdout only):**

```bash
sudo RUST_LOG=info make run
```

**Phase 3 (ingest pipeline):**

```bash
make compose-up    # one command — frees :3000, starts stack, recreates API if needed
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run   # agent only (separate terminal)
```

Use **`make run-api`** only for host-only API dev (not with `compose-up`). Tear down: `make compose-down`.

Rebuild API image: `docker compose build finops-api && docker compose up -d finops-api`

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_INGEST_URL` | (unset) | HTTP ingest URL; unset = stdout |
| `FINOPS_EBF_PATH` | set by `make run` | Path to compiled BPF ELF |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation flush interval |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `KAFKA_BROKERS` | `localhost:9092` | API → Kafka |
| `FINOPS_API_PORT` | `3000` | API listen port |

## Validate

```bash
make check
make verify-btf
```

- Phase 2: [docs/phase2-validation.md](docs/phase2-validation.md)
- Phase 3: [docs/phase3-validation.md](docs/phase3-validation.md)

## Layout

```
finops-core/
├── finops-ebpf/
├── finops-common/
├── finops-user/
├── finops-api/
├── infra/clickhouse/
├── docker-compose.yml
├── docs/
├── Makefile
└── README.md
```

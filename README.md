# FinOps eBPF Agent

Kernel-side workload identity + cgroup memory telemetry, rolled up in user space and emitted as batched JSON.

## Status

| Phase | State | What ships |
|-------|--------|------------|
| **1** | Done | `sched:sched_process_exec` tracepoint, ring buffer, basic agent loop |
| **2** | **Done** | cgroup attribution, K8s labels (optional), `memory.current` sampling, `schema_version: 2` batches on stdout |
<<<<<<< HEAD
| **3** | Planned | gRPC ingest (spec: [docs/phase3-ingest-interface.md](docs/phase3-ingest-interface.md)) |

## What’s in the repo

Three crates:

- **`finops-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`finops-common`** — shared event layout (`FinopsEvent`, kinds, sizes)
- **`finops-user`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, prints JSON
=======
| **3** | **Done** | HTTP ingest API → Kafka → ClickHouse ([spec](docs/phase3-ingest-interface.md)) |

## What’s in the repo

Four crates + infra:

- **`finops-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`finops-common`** — shared event layout (`FinopsEvent`, kinds, sizes)
- **`finops-user`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, stdout or HTTP ingest
- **`finops-api`** — `POST /ingest` → Kafka (`mpsc` + background producer)
- **`docker-compose.yml`** — Kafka KRaft, Kafka UI, ClickHouse with Kafka engine table
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups (`exec_count`, memory max/last, sample counts) flushed to stdout

<<<<<<< HEAD
Design notes for hot-path choices: [docs/adr/](docs/adr/).
=======
Phase 3 adds fire-and-forget `POST` to `finops-api`, one Kafka JSON row per workload, ClickHouse ingestion via materialized view.

**Enterprise low-latency contract:** [docs/enterprise-latency.md](docs/enterprise-latency.md)  
Design decisions (ADRs): [docs/adr/](docs/adr/)  
Contributing: read `.cursor/skills/finops-ebpf-agent/SKILL.md` first; update ADR + docs + skills with every architectural change.
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## Prerequisites

- Linux 5.8+ with BTF (`/sys/kernel/btf/vmlinux`)
- cgroup v2 unified hierarchy
- **Rust:** stable (user agent) + nightly (eBPF)
- **Tools:** `clang`, `bpf-linker`, `bpftool` (optional, for `make verify-btf`)
- **Privileges:** root or `CAP_BPF` + `CAP_PERFMON` to load programs

## Install & build

```bash
# Toolchain (first time)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
cargo install bpf-linker   # if missing

# Build
cd finops-core
make deps    # check clang, bpf-linker, nightly
<<<<<<< HEAD
make build   # eBPF ELF + finops-user binary
=======
make build   # eBPF ELF + finops-user + finops-api
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
```

Binaries:

- eBPF: `finops-ebpf/target/bpfel-unknown-none/release/finops-ebpf`
- Agent: `target/release/finops-user`
<<<<<<< HEAD

## Run

=======
- API: `target/release/finops-api`

## Run

**Phase 2 (stdout only):**

>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
```bash
sudo RUST_LOG=info make run
```

<<<<<<< HEAD
=======
**Phase 3 (ingest pipeline):** requires Docker (`docker.io` + `docker-compose-v2` on Ubuntu).

```bash
make compose-up                    # Kafka :9092, UI :8080, ClickHouse :8123
make run-api                       # terminal 1 — ingest API :3000
sudo FINOPS_INGEST_URL=http://localhost:3000/ingest make run   # terminal 2 — agent
```

>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
Useful env vars:

| Variable | Default | Purpose |
|----------|---------|---------|
<<<<<<< HEAD
=======
| `FINOPS_INGEST_URL` | (unset) | HTTP ingest URL; unset = stdout |
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
| `FINOPS_EBF_PATH` | set by `make run` | Path to compiled BPF ELF |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation flush interval |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `FINOPS_CGROUP_ROOT` | `/sys/fs/cgroup` | cgroup v2 root |
| `FINOPS_RAW_EVENTS` | off | Per-event debug JSON |
<<<<<<< HEAD
=======
| `KAFKA_BROKERS` | `localhost:9092` | API → Kafka (finops-api) |
| `FINOPS_API_PORT` | `3000` | API listen port |
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## Validate

```bash
make check
make verify-btf
```

<<<<<<< HEAD
Smoke test and pass criteria: [docs/phase2-validation.md](docs/phase2-validation.md).
=======
Validation:

- Phase 2: [docs/phase2-validation.md](docs/phase2-validation.md)
- Phase 3: [docs/phase3-validation.md](docs/phase3-validation.md)
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)

## Layout

```
finops-core/
├── finops-ebpf/      # kernel program
├── finops-common/    # shared types
├── finops-user/      # daemon
<<<<<<< HEAD
=======
├── finops-api/       # ingest API
├── infra/clickhouse/ # init.sql
├── docker-compose.yml
>>>>>>> 57e6b31 (Fixed merge conflict and added boiler for phase 3)
├── docs/             # validation, ingest spec, ADRs
├── Makefile
└── README.md
```

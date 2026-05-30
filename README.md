# FinOps eBPF Agent

Kernel-side workload identity + cgroup memory telemetry, rolled up in user space and emitted as batched JSON.

## Status

| Phase | State | What ships |
|-------|--------|------------|
| **1** | Done | `sched:sched_process_exec` tracepoint, ring buffer, basic agent loop |
| **2** | **Done** | cgroup attribution, K8s labels (optional), `memory.current` sampling, `schema_version: 2` batches on stdout |
| **3** | Planned | gRPC ingest (spec: [docs/phase3-ingest-interface.md](docs/phase3-ingest-interface.md)) |

## What’s in the repo

Three crates:

- **`finops-ebpf`** — BPF program (nightly, `bpfel-unknown-none`)
- **`finops-common`** — shared event layout (`FinopsEvent`, kinds, sizes)
- **`finops-user`** — loads BPF, reads ring buffer, attributes cgroups, aggregates, prints JSON

Phase 2 behavior in short:

- Tracepoint on process exec → `cgroup_id` + workload identity events
- Periodic read of cgroup v2 `memory.current` for tracked cgroups
- Optional in-cluster K8s pod list → namespace / pod / container labels
- Time-windowed rollups (`exec_count`, memory max/last, sample counts) flushed to stdout

Design notes for hot-path choices: [docs/adr/](docs/adr/).

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
make build   # eBPF ELF + finops-user binary
```

Binaries:

- eBPF: `finops-ebpf/target/bpfel-unknown-none/release/finops-ebpf`
- Agent: `target/release/finops-user`

## Run

```bash
sudo RUST_LOG=info make run
```

Useful env vars:

| Variable | Default | Purpose |
|----------|---------|---------|
| `FINOPS_EBF_PATH` | set by `make run` | Path to compiled BPF ELF |
| `FINOPS_WINDOW_SECS` | `10` | Aggregation flush interval |
| `FINOPS_SAMPLE_INTERVAL_SECS` | `10` | `memory.current` poll interval |
| `FINOPS_NODE_NAME` | hostname | Node id in batches |
| `FINOPS_CGROUP_ROOT` | `/sys/fs/cgroup` | cgroup v2 root |
| `FINOPS_RAW_EVENTS` | off | Per-event debug JSON |

## Validate

```bash
make check
make verify-btf
```

Smoke test and pass criteria: [docs/phase2-validation.md](docs/phase2-validation.md).

## Layout

```
finops-core/
├── finops-ebpf/      # kernel program
├── finops-common/    # shared types
├── finops-user/      # daemon
├── docs/             # validation, ingest spec, ADRs
├── Makefile
└── README.md
```

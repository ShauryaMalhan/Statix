# Phase 2 validation guide

## Prerequisites

- Linux 5.8+ with BTF (`/sys/kernel/btf/vmlinux` present)
- cgroup v2 unified hierarchy (`mount | grep cgroup2`)
- Root or `CAP_BPF` + `CAP_PERFMON` (+ read access to cgroup fs)
- `make build` succeeds (nightly eBPF + stable user agent)

## Local smoke test

```bash
cd finops-core
make build
sudo RUST_LOG=info FINOPS_WINDOW_SECS=5 FINOPS_SAMPLE_INTERVAL_SECS=5 make run
```

In another terminal, trigger exec events:

```bash
ls /tmp
```

Expect batched JSON lines with `"schema_version":2` every 5 seconds after workloads appear in the window.

## Debug: raw per-event stream

```bash
FINOPS_RAW_EVENTS=1 sudo make run
```

## cgroup-only mode (no Kubernetes)

Run on a bare VM without `KUBERNETES_SERVICE_HOST`. Batches should include `cgroup_id` and `k8s_resolved:false`.

## kind / minikube (in-cluster)

1. Load agent image or bind-mount binary + eBPF ELF into a privileged DaemonSet.
2. Set env:
   - `FINOPS_EBF_PATH=/path/to/finops-ebpf`
   - `FINOPS_NODE_NAME` from downward API `spec.nodeName`
   - ServiceAccount with `get`, `list`, `watch` on `pods`
3. Mount host `/sys/fs/cgroup` read-only at `/sys/fs/cgroup`.
4. Deploy a known pod; exec into it; confirm batch row shows correct `namespace`, `pod`, `container`, `k8s_resolved:true`.

## Overhead check

Compare node CPU with agent on vs off (idle cluster). Target &lt;0.1% per core at idle; investigate if sample interval is too aggressive.

## Verifier / BTF

```bash
make verify
make verify-btf   # confirms BTF is available on this kernel
```

## Pass criteria

| Test | Pass |
|------|------|
| Tracepoint attach | Ready line shows `sched:sched_process_exec` |
| Batched output | `schema_version: 2`, `workloads` array |
| Memory fields | `memory_bytes_max` / `memory_bytes_last` populated after sample tick |
| K8s (optional) | `k8s_resolved: true` for pod workloads |
| Build | `make build` and `make check` clean |

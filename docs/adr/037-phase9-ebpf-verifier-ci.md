# ADR 037: Phase 9 eBPF verifier CI (kernel matrix)

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 9 existential risk — a verifier rejection on a customer kernel (5.10–6.8) means silent total telemetry loss. Pre-BTF legacy kernels are out of scope.

## Decision

### GitHub Actions (`.github/workflows/ebpf-ci.yml`)

1. **`build-and-test-userspace`** — `ubuntu-latest`, stable Rust + cache:
   - `cargo check --workspace`
   - `cargo test -p finops-gateway -p finops-agent -p finops-wire`

2. **`ebpf-verifier-matrix`** — depends on (1); `fail-fast: false`; kernels **5.10, 5.15, 6.1, 6.8**:
   - Build eBPF ELF: `cargo +nightly build --release -Z build-std=core --target bpfel-unknown-none` in `finops-ebpf/`
   - Build `finops-ebpf-verify` (agent bin)
   - Per matrix cell: `scripts/verify-ebpf-kernel.sh` boots kernel via **virtme-ng** and loads ELF through **Aya**

### Verifier harness (not `bpftool prog load`)

- **`finops-agent/src/bin/verify_ebpf.rs`** — `Ebpf::load(&bytes)` runs the **in-kernel BPF verifier** without attaching probes.
- **`scripts/verify-ebpf-kernel.sh`** — `vng -r <LTS-tip> --rw -- finops-ebpf-verify <elf>`. Matrix labels (`5.10` … `6.8`) map to Ubuntu mainline **point releases** (e.g. `5.10` → `v5.10.258`), not `.0` trees — bare `v5.10` is `5.10.0`, which fails ringbuf load and panics on noble userspace.

**Rejected approach:** `bpftool prog load` — libbpf v1.0+ rejects Aya ELFs with legacy `maps` section definitions.

### Support matrix

| Kernel | CI | Production |
|--------|-----|------------|
| 5.10 | yes | yes (EKS/AKS LTS) |
| 5.15 | yes | yes |
| 6.1 | yes | yes |
| 6.8 | yes | yes (ubuntu-latest class) |
| &lt; 5.8 / no BTF | **no** | **unsupported** |

Default CI ring tier: `FINOPS_RING_BUF_BYTES=524288` (512 KiB — matches agent small ELF).

## Rationale

- **Real verifier:** virtme-ng + Aya exercises the same `bpf()` load path as production; BTFHub + `bpftool` only validates libbpf object layout, not Aya objects.
- **Matrix isolation:** `fail-fast: false` surfaces all failing kernels in one PR.
- **KVM:** GitHub `ubuntu-latest` runners expose `/dev/kvm` after udev rule (workflow step).

## Consequences

- **Positive:** Any verifier regression fails CI on all four LTS-class kernels before customer deploy.
- **Negative:** Matrix job is slow (~kernel download + VM boot per cell); `bpf-linker` + nightly build cached per lockfile.
- **Deferred:** arm64 matrix (Phase 9 TODO); verify all three ring-buffer ELF variants in CI.

## References

- [TODO.md](../../.cursor/skills/finops-ebpf-agent/TODO.md) — Phase 9
- [ADR 013](013-configurable-ring-buffer-size.md) — ring tiers
- [ADR 024](024-agent-production-container.md) — agent BPF bundle in prod image

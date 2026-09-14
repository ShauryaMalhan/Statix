# ADR 024: Production agent container (`Dockerfile.statix`)

**Status:** Accepted  
**Date:** 2026-06-04  
**Context:** Target 1 (packaging & deployment) — ship `finops-user` as an image alongside [ADR 009](009-finops-api-docker-compose.md) gateway image.

## Decision

1. **`deploy/docker/Dockerfile.statix`** — multi-stage:
   - **Builder:** `rust:1.86-bookworm`; clang/llvm/libelf; nightly + `bpf-linker`; `make build-ebpf` equivalent → `target/bpf/{small,large,xlarge}`; `cargo build --release -p finops-user`.
   - **Runtime:** `debian:bookworm-slim`; `ca-certificates`; **no non-root user** (BPF load requires root/CAPs).
2. **Binary name in image:** `/usr/local/bin/statix` (crate `finops-user`).
3. **BPF bundle:** copied to `/app/bpf`; `STATIX_BPF_DIR=/app/bpf` (compile-time default in `ebpf_select.rs` points at build tree — runtime must set env in container).
4. **Deploy:** DaemonSet `securityContext.privileged: true` or `capabilities` add `BPF`, `PERFMON`, `SYS_ADMIN`; hostPID/hostPath for cgroup v2 as needed (Phase 8 YAML still TODO).

## Rationale

- Single image build reproduces host `make build-ebpf && make build-user` without relying on stale `target/` in git.
- Gateway remains non-root; agent isolation matches kernel requirements.

## Consequences

- **Positive:** ECS/K8s can schedule agent from registry; three ELF tiers baked in.
- **Negative:** Large builder layer; image build ~10–20 min cold.
- **Negative:** Privileged pod required — stricter cluster policy than gateway.

## References

- `deploy/docker/Dockerfile.statix`, `deploy/docker/README.md`
- `finops-user/src/ebpf_select.rs`, `Makefile` `build-ebpf`

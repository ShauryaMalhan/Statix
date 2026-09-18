# ADR 062: Pin build-toolchain versions

**Status:** Accepted
**Date:** 2026-09-19
**Context:** On 2026-09-14 the eBPF verifier CI matrix failed on all four kernels at `Install bpf-linker`, on a commit that changed only markdown. Triage found four separate unpinned tool installs and, in the production agent image, a second latent failure behind the first. This ADR records the policy so it stops recurring, and supersedes the base-image details in [ADR 009](../deploy/009-finops-api-docker-compose.md) and [ADR 024](../deploy/024-agent-production-container.md) (those remain accurate as records of what was decided then).

## What broke

`cargo install bpf-linker` was unpinned. On 2026-08-12 bpf-linker 0.11.0 dropped `aya-rustc-llvm-proxy`, the shim that let it build against **rustc's bundled LLVM**. From 0.11 it requires a **system LLVM 21+** (`llvm-sys` 211/221/231).

Measured in the builder image on 2026-09-19:

| LLVM available | Version |
|----------------|---------|
| Bundled inside `rustc` (`rust:1.97.1-bookworm`) | **22.1.6** |
| System, via `apt-get install llvm` on Debian bookworm | **14.0** |
| Required by bpf-linker 0.11+ | **21+** |

The irony is worth recording: the image *does* contain a new enough LLVM (22.1.6) — it is just inside rustc, not exposed as a system library. `aya-rustc-llvm-proxy` existed precisely to reach it. Removing the shim sent bpf-linker looking for a system LLVM, where it found 14.0 and stopped. The CI runner (Ubuntu 24.04, LLVM 18) is likewise below 21.

Two properties turned a routine upstream release into a broken release artifact:

- **`--locked` does not pin the installed crate's version.** It pins the dependency *tree* of whatever version resolves. `cargo install foo --locked` still installs the newest `foo`.
- **A cache hid it for months.** The CI step was `if ! command -v bpf-linker; then cargo install …; fi` with `~/.cargo/bin` cached. While a good binary sat in the cache the install never ran. GitHub evicts caches after roughly a week unused; with five weeks between pushes the cache was gone, the step executed for real, and the latent breakage surfaced on an unrelated commit.

Fixing the pin alone was **not sufficient** for the agent image. `bpf-linker` publishes no `Cargo.lock`, so even the pinned 0.10.4 resolves its dependency tree fresh at install time, and that tree had moved past the builder's Rust:

```
rustc 1.86.0 is not supported by the following packages:
  cargo-platform@0.3.2 requires rustc 1.88
  libloading@0.9.0     requires rustc 1.88.0
  time@0.3.47          requires rustc 1.88.0
```

CI never hit this because it uses `dtolnay/rust-toolchain@stable` (always current) rather than a pinned old image — so the same defect presented differently in two places.

## Decision

### Pin every externally-fetched build tool to an exact version

Applies to `cargo install`, `pip install`, and any tool fetched at build time, in CI, Dockerfiles and the Makefile alike.

- `bpf-linker` is pinned to **0.10.4**, the last release that builds against rustc's bundled LLVM. Pinned in `.github/workflows/ebpf-ci.yml` (`BPF_LINKER_VERSION`) and `deploy/docker/Dockerfile.statix` (`ARG BPF_LINKER_VERSION`). **These two must be changed together.**
- Moving off 0.10.4 means provisioning LLVM 21+ and managing `LLVM_SYS_*_PREFIX`. That is a deliberate future decision, not a drive-by upgrade.

### Never let a cache decide which version is installed

A presence check (`command -v`) accepts any cached binary and silently tolerates drift. Check the **exact version** and reinstall with `--force` when it differs. A pinned dependency behind a cache is not pinned — it is a delayed failure.

### One pinned Rust toolchain across all images

All three images build with **`rust:1.97.1`** — the toolchain verified to build this project end to end on aarch64. Previously the agent image moved to 1.97.1 while both gateway images stayed on 1.86.

The workspace shares one `Cargo.lock`. Building parts of it with different compilers means the next dependency that raises an MSRV breaks only one image, later, with no related change to point at — which is precisely the failure above. "It still builds today" is a reason not to panic, not a reason to leave it.

## Consequences

- **Positive:** CI and image builds are reproducible against upstream churn. Failures now arrive when someone changes the pin, not at a random later push. One compiler across all artifacts removes a class of skew.
- **Negative:** Pins go stale silently — nothing tells us a newer bpf-linker or Rust is available. Upgrades become deliberate work. This is the trade we want: predictable staleness over unpredictable breakage.
- **Carrying cost:** the bpf-linker pin is duplicated between CI and the Dockerfile, and the two gateway Dockerfiles duplicate each other. Both are drift hazards tracked in [TODO.md](../../../.cursor/skills/statix-ebpf-agent/TODO.md).
- **Verified:** agent image builds through all 15 steps (173MB, all three ring-buffer ELF tiers present); both gateway images build and start (160MB production).

## References

- [ADR 024](../deploy/024-agent-production-container.md) — production agent container (base image details superseded)
- [ADR 009](../deploy/009-finops-api-docker-compose.md) — dev Compose gateway image (base image details superseded)
- [ADR 037](../ebpf/037-phase9-ebpf-verifier-ci.md) — eBPF verifier CI kernel matrix
- Commits `9856d7e` (CI pin), `9b9bd81` (agent image), `c932c81` (gateway alignment)

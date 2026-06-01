# ADR 013: Build-time ring buffer tiers + CPU-based ELF selection

**Status:** Accepted  
**Date:** 2026-06-01  
**Context:** Fixed 512KB `EVENTS` ring buffer drops events on high-core / high-churn nodes (TODO 1.2 / HC-01). Map size must be a compile-time constant for `RingBuf::with_byte_size`.

## Decision

1. **`finops-ebpf/build.rs`** — reads `FINOPS_RING_BUF_BYTES` at compile time; emits `RING_BUF_BYTES` in `OUT_DIR/ring_config.rs`; `cargo:rerun-if-env-changed`.
2. **`finops-ebpf/src/main.rs`** — `include!(…/ring_config.rs)`; `RingBuf::with_byte_size(RING_BUF_BYTES, 0)`.
3. **`make build-ebpf`** — builds three release ELFs into `target/bpf/`:
   - `finops-ebpf-small` — 524288 (512KB), ≤8 cores
   - `finops-ebpf-large` — 4194304 (4MB), 9–64 cores
   - `finops-ebpf-xlarge` — 8388608 (8MB), 65+ cores
4. **`finops-user/src/ebpf_select.rs`** — `num_cpus::get()` picks variant; `FINOPS_EBF_PATH` overrides; `FINOPS_BPF_DIR` defaults to `target/bpf` (compile-time path from crate root).

## Rationale

- BPF map sizing is fixed at load time; one ELF per size avoids runtime `unsafe` or map resize.
- Host autoscaling: agent adapts footprint on boot without manual env per machine class.
- Override paths preserved for CI and debugging.

## Consequences

- **Positive:** Fewer ring-buffer drops on large nodes; small nodes keep 512KB kernel RAM.
- **Negative:** `build-ebpf` ~3× compile time; three artifacts to ship in images.
- **Negative:** Tier thresholds (8 / 64 cores) are heuristic — tune with production drop metrics.

## References

- `finops-ebpf/build.rs`, `Makefile`, `finops-user/src/ebpf_select.rs`

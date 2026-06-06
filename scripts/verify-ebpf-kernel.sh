#!/usr/bin/env bash
# Run the kernel BPF verifier for a target kernel version using virtme-ng.
#
# Usage: verify-ebpf-kernel.sh <kernel-version> <elf-path> <verify-binary>
# Example: verify-ebpf-kernel.sh 5.15 finops-ebpf/target/.../finops-ebpf target/release/finops-ebpf-verify
#
# Support matrix (BTF-era cloud kernels): 5.10, 5.15, 6.1, 6.8
set -euo pipefail

KERNEL_VERSION="${1:?kernel version required (e.g. 5.15)}"
ELF_PATH="${2:?eBPF ELF path required}"
VERIFY_BIN="${3:?finops-ebpf-verify binary path required}"

if [[ ! -f "${ELF_PATH}" ]]; then
  echo "error: eBPF ELF not found: ${ELF_PATH}" >&2
  exit 1
fi

if [[ ! -x "${VERIFY_BIN}" ]]; then
  echo "error: verify binary not executable: ${VERIFY_BIN}" >&2
  exit 1
fi

if ! command -v vng &>/dev/null; then
  echo "error: virtme-ng (vng) not installed — required for per-kernel verifier tests" >&2
  exit 1
fi

ELF_ABS="$(realpath "${ELF_PATH}")"
VERIFY_ABS="$(realpath "${VERIFY_BIN}")"

echo "==> BPF verifier matrix: kernel ${KERNEL_VERSION}"
echo "    ELF:    ${ELF_ABS}"
echo "    loader: ${VERIFY_ABS}"

# virtme-ng downloads Ubuntu mainline builds when -r is prefixed with "v" (e.g. v5.15).
VNG_KERNEL="v${KERNEL_VERSION}"

# Boot the requested kernel in a CoW VM and load the ELF through Aya (kernel verifier).
vng -v -r "${VNG_KERNEL}" --rw -- "${VERIFY_ABS}" "${ELF_ABS}"

echo "==> Kernel ${KERNEL_VERSION}: verifier accepted program"

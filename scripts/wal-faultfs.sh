#!/usr/bin/env bash
# Phase 11 — WAL disk-degradation harness.
#
# Mounts a small, size-limited tmpfs and points the WAL at it, then runs the
# `#[ignore]`d ENOSPC integration test to prove the WAL surfaces a full disk as a
# recoverable error (drop-oldest / Err — never a panic) and stays usable.
#
# Usage:  sudo scripts/wal-faultfs.sh          # default 4 MiB tmpfs
#         sudo WAL_FAULTFS_SIZE=8m scripts/wal-faultfs.sh
#
# Requires root (mount/umount). Auto-cleans the mount on exit.
set -euo pipefail

SIZE="${WAL_FAULTFS_SIZE:-4m}"
MNT="$(mktemp -d /tmp/statix-wal-faultfs.XXXXXX)"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cleanup() {
  mountpoint -q "$MNT" && umount "$MNT" || true
  rmdir "$MNT" 2>/dev/null || true
}
trap cleanup EXIT

if [ "$(id -u)" -ne 0 ]; then
  echo "ERROR: must run as root to mount tmpfs (try: sudo $0)" >&2
  exit 1
fi

echo "==> Mounting ${SIZE} tmpfs at ${MNT}"
mount -t tmpfs -o "size=${SIZE}" tmpfs "$MNT"

echo "==> Running WAL ENOSPC degradation test against ${MNT}"
cd "$ROOT"
STATIX_WAL_TEST_DIR="$MNT" \
  cargo test -p statix --release wal::tests::enospc_is_handled_without_panic -- --ignored --nocapture

echo "==> WAL disk-degradation test passed (ENOSPC handled without panic)."

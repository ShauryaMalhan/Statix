#!/usr/bin/env bash
# Bootstrap a *bare* Linux machine to the point where `make` exists, then hand
# off to `make deps` for the real toolchain install.
#
# Why this file exists: `make` ships in build-essential, which `make deps`
# installs — so on a genuinely fresh Ubuntu, `make deps` cannot run at all.
# This is the only entry point that assumes nothing but bash + sudo + apt.
#
#   fresh machine:  ./scripts/bootstrap.sh
#   afterwards:     make deps        (idempotent; re-run any time)
set -euo pipefail

if [ "$(uname -s)" != "Linux" ]; then
    echo "ERROR: the eBPF toolchain only builds on Linux; this is $(uname -s)." >&2
    echo "       On macOS, run it inside the VM:" >&2
    echo "         colima ssh -- bash -lc 'cd \"\$PWD\" && ./scripts/bootstrap.sh'" >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> [0/3] bootstrapping make (needs sudo)"
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends make

echo "==> handing off to 'make deps'"
exec make -C "$REPO_ROOT" deps

#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Stopping Statix dev environment"
echo "  agent..."

colima ssh -- sudo pkill -x statix 2> /dev/null || true

echo "  gateway..."
colima ssh -- pkill -x statix-gateway 2> /dev/null || true

if [ "${1:-}" = "--all" ] ; then
    echo "  clickhouse..."
    docker-compose -f "$REPO_ROOT/docker-compose.yml" stop clickhouse
fi
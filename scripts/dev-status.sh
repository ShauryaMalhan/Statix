#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CH_PASSWORD=$(grep '^CLICKHOUSE_PASSWORD=' "$REPO_ROOT/.env" | cut -d= -f2-)
echo "Statix dev status"

check() {
    label="$1"
    shift
    if "$@" > /dev/null 2>&1; then
        echo "$label is healthy"
    else
        echo "$label is not healthy"
    fi
}

check "colima" colima status

check "clickhouse" curl -s -f -m 3 \
    -H "X-ClickHouse-User: default" \
    -H "X-ClickHouse-Key: $CH_PASSWORD" \
    --data-binary "SELECT 1" \
    http://127.0.0.1:8123/

check "Gateway" curl -s -f -m 3 http://127.0.0.1:3000/ready

check "Statix" colima ssh -- pgrep -x statix

AGE=$(curl -s -f -m 3 \
    -H "X-ClickHouse-User: default" \
    -H "X-ClickHouse-Key: $CH_PASSWORD" \
    --data-binary "SELECT round(now() - toDateTime(intDiv(max(window_end_ns), 1000000000))) FROM statix.workload_metrics" \
    http://127.0.0.1:8123/)

WORKLOADS=$(curl -s -f -m 3 \
    -H "X-ClickHouse-User: default" \
    -H "X-ClickHouse-Key: $CH_PASSWORD" \
    --data-binary "SELECT uniqExact(cgroup_id) FROM statix.workload_metrics WHERE window_start_ns >= (toUnixTimestamp(now()) - 300) * 1000000000" \
    http://127.0.0.1:8123/)

echo "Data: $WORKLOADS workloads active, newest ${AGE}s ago"
#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CH_PASSWORD=$(grep '^CLICKHOUSE_PASSWORD=' "$REPO_ROOT/.env" | cut -d= -f2-)

echo "starting Statix dev environment"

if colima status > /dev/null 2>&1; then
    echo "colima is already running"
else
    echo "starting colima..."
    colima start
fi

echo "starting clickhouse..."

docker-compose -f "$REPO_ROOT/docker-compose.yml" up -d clickhouse

for i in $(seq 1 30); do
    if docker ps --filter "name = clickhouse" --format '{{.Status}}' | grep -q "healthy"; then
        echo "clickhouse is up"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "ERROR: clickhouse never became healthy" >&2
        exit 1
    fi
    sleep 1
done

echo "starting statix-gateway..."
colima ssh -- bash -lc "cd '$REPO_ROOT' && setsid --fork env \
    CLICKHOUSE_URL=http://127.0.0.1:8123/ \
    CLICKHOUSE_USER=default \
    CLICKHOUSE_PASSWORD='$CH_PASSWORD' \
    STATIX_API_PORT=3000 RUST_LOG=info \
    ./target/release/statix-gateway > /tmp/statix-gateway.log 2>&1 < /dev/null"

for i in $(seq 1 30); do
    if curl -s -f -m 3 http://127.0.0.1:3000/ready > /dev/null 2>&1; then
        echo "statix-gateway is up"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "ERROR: statix-gateway never became healthy" >&2
        exit 1
    fi
    sleep 1
done

echo "  agent..."
colima ssh -- bash -lc "cd '$REPO_ROOT' && sudo setsid --fork env \
    STATIX_BPF_DIR='$REPO_ROOT/target/bpf' \
    STATIX_INGEST_URL=http://127.0.0.1:3000/ingest \
    STATIX_NODE_NAME=colima-vm \
    RUST_LOG=info \
    ./target/release/statix > /tmp/statix-agent.log 2>&1 < /dev/null"

for i in $(seq 1 30); do
    FRESH=$(curl -s -f -m 3 \
        -H "X-ClickHouse-User: default" \
        -H "X-ClickHouse-Key: $CH_PASSWORD" \
        --data-binary "SELECT count() FROM statix.workload_metrics WHERE window_start_ns >= (toUnixTimestamp(now()) - 60) * 1000000000" \
        http://127.0.0.1:8123/ || echo 0)
    if [ "$FRESH" -gt 0 ]; then
        echo "    agent is reporting ($FRESH rows in the last minute)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "    ERROR: agent started but no data arrived" >&2
        exit 1
    fi
    sleep 1
done

echo ""
echo "Dashboard: http://127.0.0.1:3000/"
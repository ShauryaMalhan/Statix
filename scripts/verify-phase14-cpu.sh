#!/usr/bin/env bash
# Phase 14 CPU correctness gates — unit tests + optional live stack drill.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Phase 14 gate 1: wire backward compat (v2 default cpu=0)"
cargo test -p statix-wire --quiet

echo "==> Phase 14 gates 2–4: priming, conservation, soft miss (agent unit tests)"
cargo test -p statix phase14_ --quiet

echo "==> Phase 14 gateway smoke"
cargo test -p statix-gateway --quiet

if [[ "${STATIX_PHASE14_E2E:-}" == "1" ]]; then
  echo "==> Phase 14 optional E2E (STATIX_PHASE14_E2E=1)"
  if ! curl -sf -o /dev/null http://127.0.0.1:3000/health; then
    echo "ERROR: gateway not reachable on :3000 — run make compose-up first" >&2
    exit 1
  fi
  if ! pgrep -x statix >/dev/null; then
    echo "ERROR: statix agent not running — export STATIX_INGEST_URL and sudo -E make run" >&2
    exit 1
  fi
  : "${CLICKHOUSE_PASSWORD:=}"
  stress-ng --cpu 1 --timeout 12s >/dev/null 2>&1 || true
  sleep 8
  busy="$(
    curl -s -u "default:${CLICKHOUSE_PASSWORD}" \
      "http://localhost:8123/?query=SELECT%20max(cpu_usage_usec)%20FROM%20statix.workload_metrics%20FINAL"
  )"
  if [[ "${busy:-0}" -lt 1000 ]]; then
    echo "ERROR: expected busy cgroup cpu_usage_usec > 1000, got ${busy:-empty}" >&2
    exit 1
  fi
  echo "    E2E: max cpu_usage_usec in CH = ${busy}"
  curl -sf 'http://127.0.0.1:3000/api/v1/workloads/summary?hours=1' | grep -q total_cpu_usec \
    || { echo "ERROR: summary API missing total_cpu_usec" >&2; exit 1; }
  echo "    E2E: summary API total_cpu_usec present"
fi

echo "==> Phase 14 CPU gates OK"

# Phase 13 — Queue-less architecture

| ADR | Title | Status |
|-----|-------|--------|
| [055](055-phase13-part1-kafka-removal-rowbinary.md) | Part 1 — Kafka removal; gateway RowBinary → ClickHouse | Accepted |
| [056](056-phase13-part2-ingest-zero-alloc.md) | Part 2 — single `MetricRow`; drop `FlatRow` double-buffer | Accepted |

**Playbooks:** [PHASE_13_PART1_PLAYBOOK.md](../../../.cursor/skills/statix-ebpf-agent/PHASE_13_PART1_PLAYBOOK.md) · [PHASE_13_PART2_PLAYBOOK.md](../../../.cursor/skills/statix-ebpf-agent/PHASE_13_PART2_PLAYBOOK.md)

**Open (infra, not ingest code):** strip Kafka from `docker-compose.yml`, `deploy/k8s/gateway.yaml`, and deploy READMEs — see [TODO.md](../../../.cursor/skills/statix-ebpf-agent/TODO.md).

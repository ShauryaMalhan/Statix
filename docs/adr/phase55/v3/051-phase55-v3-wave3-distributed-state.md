# ADR 051: Phase 5.5 V3 Wave 3 — distributed state physics

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** L8/L9 Post-GA audit Wave 3 ([L8_POST_GA_FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8_POST_GA_FIXES.md)) — ClickHouse partition storms at UTC midnight, single-threaded Kafka ingest, and post-outage agent recovery thundering herd.

## Decision

### V3-11 — Hour-aligned ClickHouse partitions (`deploy/clickhouse/01_init.sql`)

- Replace `PARTITION BY toYYYYMMDD(...)` with `PARTITION BY toStartOfHour(...)` on `workload_metrics`.
- Reduces midnight UTC boundary crossings when agent clocks drift across day boundaries.

### V3-12 — Scale Kafka consumers (`deploy/clickhouse/01_init.sql`)

- Set `kafka_num_consumers = 4` on `kafka_telemetry_queue` (minimum for production; align with topic partition count).

### V3-15 — Deterministic recovery spread (`statix/src/output.rs`)

- On first `Success` after elevated backoff, sleep `hash(node) % 30s` + 0–5s PRNG jitter before draining backlog.
- Node identity from `STATIX_NODE_NAME` → `NODE_NAME` → `/etc/hostname` → `localhost`.
- Uses `DefaultHasher` per playbook; replaces PRNG-only 0–5s recovery jitter.

## Rationale

- Day-boundary partitions + NTP drift split windows across two parts → merge pressure spike at scale.
- One Kafka consumer caps ingest ~50k rows/s; fleet telemetry can exceed 100k rows/s.
- Thousands of agents recovering within ~5s can overwhelm gateway replicas; 30s deterministic spread flattens burst.

## Consequences

- **Positive:** Lower midnight merge storms; parallel Kafka consumption; gateway-friendly recovery after outages.
- **Negative:** Hourly partitions increase part count vs daily — acceptable for telemetry TTL; existing CH volumes need `docker compose down -v` or migration for partition key change.
- **Operational:** Monitor `system.kafka_consumers` and `system.parts` merge pressure after deploy.

## References

- [ADR 050](../v3/050-phase55-v3-wave2-cache-eviction.md) — Wave 2
- [TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md) — V3-11, V3-12, V3-15

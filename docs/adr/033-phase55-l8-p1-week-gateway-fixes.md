# ADR 033: Phase 5.5 L8 audit — P1-WEEK gateway and agent fixes

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 5.5 P1-WEEK ([TODO.md](../../.cursor/skills/statix-ebpf-agent/TODO.md)) — eliminate moderate-effort allocations and connection overhead on agent retry path, K8s refresh, Kafka producer, and operational ClickHouse reads. P0-SHIP already landed in [ADR 032](032-phase55-l8-p0-hot-path-fixes.md).

## Decision

| Fix | File | Change |
|-----|------|--------|
| F1 | `statix/src/output.rs` | Retry channel holds `bytes::Bytes`; `post_ingest` uses `body.clone()` (O(1) refcount) on retries |
| F2 | `finops-api/src/kafka.rs` | Hoist `by_partition` `HashMap` into `run_producer_loop`; `.clear()` between batches |
| F3 | `finops-api/src/kafka.rs` | One `Utc::now()` per produce chunk passed to `bytes_to_record` |
| F4 | `statix/src/main.rs` + `attribution.rs` | `kube::Client::try_default()` once before K8s interval; `refresh_k8s_pods(cache, &client)` |
| F5 | `finops-api/src/kafka.rs` | `tokio::time::interval(300s)` metadata refresh + refresh on `produce` error |
| F7 | `finops-api/src/routes/query.rs` | Remove `FINAL`; `argMax(memory_bytes_max, window_start_ns)` + `sum(exec_count)` for summary |

**Dependencies:** `bytes = "1"` in `statix/Cargo.toml`.

## Rationale

- **F1:** Eliminates per-retry heap `String` allocation on 10–50 KB ingest bodies.
- **F2–F3:** Cuts HashMap churn and `clock_gettime` syscalls in the Kafka micro-batch loop.
- **F4:** Avoids TLS + TCP handshake to the K8s API every 30s per agent.
- **F5:** Survives broker partition topology changes without gateway restart.
- **F7:** Operational dashboard reads avoid `FINAL` merge cost; `argMax` dedupes by latest `window_start_ns` per cgroup/pod tuple. Billing exports still use `FINAL` ([ADR 011](011-replacingmergetree-dedupe-identity.md)).

## Consequences

- **Positive:** Lower agent memory churn on ingest retries; lower gateway CPU on Kafka produce and CH summary queries.
- **Negative:** Summary `sum(exec_count)` may briefly over-count duplicate rows before background merge — acceptable for dashboards ([ADR 027](027-api-read-path-clickhouse.md) operational vs billing split).
- **Deferred (now shipped):** P2 ingest zero-copy in [ADR 034](034-phase55-l8-p2-ingest-zero-copy.md).

## References

- [L8-AUDIT-FIXES.md](../../.cursor/skills/statix-ebpf-agent/L8-AUDIT-FIXES.md)
- [ADR 032](032-phase55-l8-p0-hot-path-fixes.md)

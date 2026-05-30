# Enterprise low-latency architecture

FinOps telemetry is **billing-adjacent**: dropped samples or blocked kernel drains can miss OOM spikes and corrupt capacity signals. This document is the non-negotiable contract for all phases.

## Design principles

| Principle | Implementation |
|-----------|----------------|
| **Never block the ring buffer** | Agent `emit_batch` uses `tokio::spawn` for HTTP; no `.await` on ingest from the event loop |
| **Never block the ingest handler** | API uses `mpsc::try_send`; Kafka `produce` only in a background task |
| **Bounded memory** | Aggregator early flush at `max_keys`; double-buffered maps with `.clear()` (retain capacity) |
| **No hot-path heap churn** | Stack reads for `memory.current`; precomputed paths; `FxHashMap` for internal `u64` keys |
| **Explicit backpressure** | Channel full → log + drop row, still HTTP 200 (agent must not stall) |
| **Raw bytes on the wire** | `serde_json` only; no ORM; ClickHouse Kafka engine consumes `JSONEachRow` |
| **Shared I/O pools** | One `reqwest::Client` via `OnceLock` (3s timeout, 90s pool idle); one Kafka producer task per API process |
| **Bounded background work** | Agent HTTP tasks must not hang on black-hole TCP; ClickHouse Kafka engine skips broken rows ([ADR 008](adr/008-clickhouse-kafka-engine-resilience.md)) |

## Latency budget (targets)

| Stage | Target | Notes |
|-------|--------|-------|
| BPF → ring buffer | μs | `reserve` / `submit` only; no printk |
| Agent event drain | μs per event | `on_finops_event` + map insert; flush work off hot path where possible |
| `emit_batch` (HTTP path) | &lt; 1 ms on caller | Serialize + `spawn` only |
| `POST /ingest` handler | &lt; 1 ms typical | Deserialize + `try_send` per row |
| Kafka produce | async | Isolated to background task |
| Node overhead | &lt; 0.1% CPU/core idle | See phase2-validation overhead check |

## What we do not do (anti-patterns)

- `std::collections::HashMap` on hot `cgroup_id` paths (SipHash cost, no benefit)
- `enforce_cap` / random key eviction (data loss)
- `read_to_string` on `memory.current` in the sample loop
- `await` Kafka or HTTP inside Axum handlers or the ring-buffer `select!` arm
- New `reqwest::Client` per batch
- `reqwest::Client` without request timeout (unbounded `tokio::spawn` on network black holes)
- Blocking K8s API in the event loop (30s interval refresh only)

## Change workflow (required)

Every feature or optimization must:

1. Read `.cursor/skills/finops-ebpf-agent/` (`SKILL.md` → `REFERENCE.md` → `PATTERNS.md`)
2. Run `make build` and `make check` (and `make verify-btf` when touching BPF/deploy)
3. Add or update an **ADR** in `docs/adr/` when the decision is architectural
4. Update **skills** and **docs** in the same change (README, phase validation, this file if principles shift)
5. Record deferred work only in `TODO.md` (open items only)

## Related

- [docs/adr/](../adr/) — point-in-time decisions
- [docs/phase3-ingest-interface.md](phase3-ingest-interface.md) — HTTP ingest contract
- [.cursor/skills/finops-ebpf-agent/](../.cursor/skills/finops-ebpf-agent/) — agent coding standards

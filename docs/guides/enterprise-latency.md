# Enterprise low-latency architecture

FinOps telemetry is **billing-adjacent**: dropped samples or blocked kernel drains can miss OOM spikes and corrupt capacity signals. This document is the non-negotiable contract for all phases.

## Design principles

| Principle | Implementation |
|-----------|----------------|
| **Never block the ring buffer** | Agent `emit_batch` enqueues to retry worker (`try_send`); no `.await` on HTTP from the event loop |
| **Never block the ingest handler** | API uses `mpsc::try_send`; Kafka `produce` only in a background task |
| **Bounded memory** | Aggregator early flush at `max_keys`; double-buffered maps with `.clear()` (retain capacity) |
| **No hot-path heap churn** | Stack reads: `[u8; 32]` `memory.current`, `[u8; 1024]` `/proc/{pid}/cgroup`; `spawn_blocking` for sampler; `FxHashMap` for `u64` keys |
| **Explicit backpressure** | Channel full → `503` + plain text (handler never blocks); tune `STATIX_KAFKA_CHANNEL_SIZE` for burst ([ADR 014](adr/014-kafka-producer-env-tuning.md)) |
| **No telemetry loss on outage** | Agent retry queue full / gateway down → spill to bounded disk WAL (segmented append-only log) via non-blocking `try_append` to a dedicated writer thread; background drainer replays oldest-first when the gateway recovers; FIFO loss only at the disk hard cap. At-least-once; deduped by `ReplacingMergeTree` ([ADR 054](../adr/phase11/054-phase11-wal-spillway.md), [PLAYBOOK](../../.cursor/skills/statix-ebpf-agent/PHASE_11_WAL_PLAYBOOK.md)) |
| **Raw bytes on the wire** | `serde_json` only; no ORM; ClickHouse Kafka engine consumes `JSONEachRow` |
| **Shared I/O pools** | One `reqwest::Client` via `OnceLock` (`STATIX_HTTP_TIMEOUT_SECS` default 5s, `STATIX_HTTP_POOL_IDLE_SECS` default 55s); one Kafka producer task per API process |
| **Partition by node** | Kafka record key = `node`; producer hashes to broker partition count ([ADR 010](adr/010-kafka-partition-key-by-node.md)) |
| **Storage dedupe** | `ReplacingMergeTree`; sort key `(node, window_start_ns, cgroup_id)`; billing reads use `FINAL` ([ADR 011](adr/011-replacingmergetree-dedupe-identity.md)) |
| **API metrics** | `GET /metrics`; explicit `metrics!` macros on ingest/Kafka — no middleware ([ADR 012](adr/012-finops-api-prometheus-metrics.md)) |
| **Bounded background work** | Agent HTTP tasks must not hang on black-hole TCP; ClickHouse Kafka engine skips broken rows ([ADR 008](adr/008-clickhouse-kafka-engine-resilience.md)) |

## Latency budget (targets)

| Stage | Target | Notes |
|-------|--------|-------|
| BPF → ring buffer | μs | `reserve` / `submit` only; no printk; map size tiered by host cores ([ADR 013](adr/013-configurable-ring-buffer-size.md)) |
| Agent event drain | μs per event | `on_statix_event` + map insert; flush work off hot path where possible |
| cgroup `memory.current` sample | async | Path snapshot + per-file `spawn_blocking` — never sync `File::open` on the runtime worker |
| `emit_batch` (HTTP path) | &lt; 1 ms on caller | Serialize + `try_send` to retry queue only |
| `POST /ingest` handler | &lt; 1 ms typical | Deserialize + `try_send` per row; one `Vec<u8>` key per batch; produce moves vecs (no `Bytes::to_vec`) |
| Kafka produce | async | Isolated to background task |
| Node overhead | &lt; 0.1% CPU/core idle | See phase2-validation overhead check |

## What we do not do (anti-patterns)

- `std::collections::HashMap` on hot `cgroup_id` paths (SipHash cost, no benefit)
- `enforce_cap` / random key eviction (data loss)
- `read_to_string` on `memory.current` or `/proc/{pid}/cgroup` on the exec hot path
- Sync `File::open` / `read` for all cgroups inside `tokio::select!` without `spawn_blocking` (starves ring-buffer drain)
- `await` Kafka or HTTP inside Axum handlers or the ring-buffer `select!` arm
- New `reqwest::Client` per batch
- Fire-and-forget `tokio::spawn` per batch POST (loses data on failure — use retry worker)
- `reqwest::Client` without request timeout (retry worker stuck on black-hole TCP)
- `await` Kubernetes API refresh inside the main `select!` loop (use `tokio::spawn` + `AttributionCache::clone`)
- `fuser -k 3000` or `pkill -f` paths containing `statix-gateway` while Docker maps `:3000` (kills port-forward or the Make shell)
- `make run-api` and `make compose-up` both binding `:3000` (use compose API + host agent only)

## Change workflow (required)

Every feature or optimization must:

1. Read `.cursor/skills/statix-ebpf-agent/` (`SKILL.md` → `REFERENCE.md` → `PATTERNS.md`)
2. Run `make build` and `make check` (and `make verify-btf` when touching BPF/deploy)
3. Add or update an **ADR** in `docs/adr/` when the decision is architectural
4. Update **skills** and **docs** in the same change (README, phase validation, this file if principles shift)
5. Record deferred work only in `TODO.md` (open items only)

## Related

- [docs/adr/](../adr/) — point-in-time decisions
- [docs/guides/phase3-ingest-interface.md](phase3-ingest-interface.md) — HTTP ingest contract
- [.cursor/skills/statix-ebpf-agent/](../.cursor/skills/statix-ebpf-agent/) — agent coding standards

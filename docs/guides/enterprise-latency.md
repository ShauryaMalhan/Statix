# Enterprise low-latency architecture

FinOps telemetry is **billing-adjacent**: dropped samples or blocked kernel drains can miss OOM spikes and corrupt capacity signals. This document is the non-negotiable contract for all phases.

## Design principles

| Principle | Implementation |
|-----------|----------------|
| **Never block the ring buffer** | Agent `emit_batch` enqueues to retry worker (`try_send`); no `.await` on HTTP from the event loop |
| **Never block the ingest handler** | API uses `try_reserve_many`; ClickHouse RowBinary insert only in background writer ([ADR 055](../adr/phase13/055-phase13-part1-kafka-removal-rowbinary.md)) |
| **Explicit backpressure** | `!ch_healthy` → instant 503; mpsc full → 503; tune `STATIX_INGEST_CHANNEL_SIZE` |
| **No telemetry loss on outage** | Agent WAL + circuit breaker on 503 ([ADR 054](../adr/phase11/054-phase11-wal-spillway.md)) |
| **Raw bytes on the wire** | Agent JSON envelope; gateway RowBinary to ClickHouse (no ORM) |
| **Shared I/O pools** | One `reqwest::Client` (agent); one `clickhouse::Client` (gateway read + write) |
| **Storage dedupe** | `ReplacingMergeTree`; billing `FINAL` ([ADR 011](../adr/011-replacingmergetree-dedupe-identity.md)) |
| **API metrics** | `GET /metrics`; `statix_api_ch_*` insert health ([ADR 012](../adr/012-finops-api-prometheus-metrics.md)) |
| **Synchronous insert ACK** | No `async_insert` — `insert.end()` timeout flips `ch_healthy` for honest backpressure |

## Latency budget (targets)

| Stage | Target | Notes |
|-------|--------|-------|
| BPF → ring buffer | μs | `reserve` / `submit` only; no printk; map size tiered by host cores ([ADR 013](adr/013-configurable-ring-buffer-size.md)) |
| Agent event drain | μs per event | `on_statix_event` + map insert; flush work off hot path where possible |
| cgroup `memory.current` sample | async | Path snapshot + per-file `spawn_blocking` — never sync `File::open` on the runtime worker |
| `emit_batch` (HTTP path) | &lt; 1 ms on caller | Serialize + `try_send` to retry queue only |
| `POST /ingest` handler | &lt; 1 ms typical | Tier 1 health check + `try_reserve_many`; no RowBinary on HTTP thread |
| ClickHouse insert | async | Isolated to `clickhouse_writer` background task |
| Node overhead | &lt; 0.1% CPU/core idle | See phase2-validation overhead check |

## What we do not do (anti-patterns)

- `std::collections::HashMap` on hot `cgroup_id` paths (SipHash cost, no benefit)
- `enforce_cap` / random key eviction (data loss)
- `read_to_string` on `memory.current` or `/proc/{pid}/cgroup` on the exec hot path
- Sync `File::open` / `read` for all cgroups inside `tokio::select!` without `spawn_blocking` (starves ring-buffer drain)
- `await` ClickHouse insert inside Axum handlers or the ring-buffer `select!` arm
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

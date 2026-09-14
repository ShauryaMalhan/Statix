# ADR 061: Phase 15 — Dashboard read tier (v1a)

**Status:** Accepted  
**Date:** 2026-09-15  
**Context:** The write path (BPF → agent → WAL → gateway → ClickHouse) carries ~15 ADRs of hardening. The read path is one endpoint — `GET /api/v1/workloads/summary` ([ADR 027](../gateway/027-api-read-path-clickhouse.md)) — and there is no UI anywhere in the repo; Grafana is provisioned in Compose but ships zero dashboards. Phase 15 builds the missing serving tier: a live workload dashboard (active workloads, CPU, memory) served by `statix-gateway`.

**Scope (v1a):** pipeline health strip, summary tiles, live workload table, server-side sort/filter, time-range selector, attribution status. Per-workload drill-down charts are v1b. Management/control actions are explicitly out of scope — Statix stays read-only.

## Decision

### Two endpoints, split by failure domain

- `GET /api/v1/dashboard/state` — tiles + table in one payload. Both derive from the same latest-window row set, so bundling is one round trip **and** half the database work versus splitting them.
- `GET /api/v1/dashboard/health` — `/ready` state, `ch_healthy`, mpsc depth/capacity, per-node freshness.
- **`/health` returns `200` even when components are unhealthy.** It *reports* status; it must not *fail* on it. A 503 here would be indistinguishable from "gateway down".
- Rationale for the split: bundling health into `/state` means a ClickHouse outage destroys the very signal that diagnoses the outage.
- `/health` depends almost entirely on `AppState` (`ready`, `ch_healthy`, `tx.capacity()`). Only the node list touches ClickHouse, and its failure degrades to an empty list, not a failed response. Freshness is derived as `now − max(window_end_ns)` rather than read from the Prometheus registry — no text-format parsing, no new counters, no change to `record_ingest_metrics`.

### Time range is a lookback horizon, not an aggregation period

- `range_secs` controls *how long since a workload last reported before we stop listing it*. Values always come from each workload's **most recent** window.
- Rejected: averaging/summing across the range. On a live table that would show a workload's hour-average CPU while labelling it "live" — actively misleading.

### Derived values in SQL; formatting in the browser

- `cpu_millicores = cpu_usage_usec * 1e6 / (window_end_ns − window_start_ns)`, computed **in SQL**. Forced by `ORDER BY`: the table sorts server-side, so the sort key must exist inside the query. Computing it in Rust after the rows return would only sort the rows already fetched — i.e. the wrong rows.
- Aggregates (`sum`, `count`, `argMax`) in SQL. Never fetch rows into Rust to total them.
- The API emits raw numeric types only. Humanizing bytes/durations server-side (`"12.4 GiB"`) would break sorting, discard precision, and freeze the unit choice.
- No server-computed `age_secs`: the client derives it from `generated_at_ns − window_end_ns`, which is immune to both cache staleness and browser clock skew.

### Response cache keyed on query parameters

- Key = normalized (`range_secs · sort · order · q · node · unattributed · limit`). **Not** keyed on caller identity: Statix is single-tenant and every viewer sees the same fleet, so identical parameters have identical answers. Keying per caller would cache nothing.
- TTL via `STATIX_DASHBOARD_CACHE_MS` (default `2000`); bounded LRU; `state` and `health` cached independently.
- Single-flight deliberately **not** implemented in v1a. The TTL alone collapses N clients into one query pair per interval in steady state; the stampede window is only the instant after expiry. Revisit if measurement shows it matters.
- **Forward constraint:** if per-viewer authorization scoping is ever added, the scope must join the cache key on day one — otherwise cached rows leak across scopes.

### Adaptive client polling

- `state` ~3s, `health` ~5s; paused when `document.visibilityState === 'hidden'`; exponential backoff + jitter on error.
- Same physics as agent retry ([ADR 006](../ingest/006-shared-http-client-for-ingest.md), [ADR 042](../fixes/042-phase55-v2-p2-sprint-l8-fixes.md)): don't hammer a down service, don't let N clients synchronize into a stampede.
- Loading state on **first paint only**; subsequent polls update in place. A spinner every 3s makes the UI unreadable.

### Dual-mode UI delivery

- `STATIX_DASHBOARD_DIR` set → serve from disk (dev hot reload, no rebuild per pixel).
- Unset → embedded asset (single deployable artifact; binary and UI cannot drift out of sync).

### No pagination

- Top-N + server-side sort + filter. The response carries the matched count so the UI can state `showing top 100 of 3,412`.
- Offset paging is *incorrect* on live data — rows shift between requests, silently skipping or duplicating entries — and `OFFSET` degrades at depth. Cursor paging fixes correctness but adds complexity for random access nobody performs on a live view.

### Unauthenticated by default, with input bounding

- Read endpoints honour `STATIX_API_TOKEN` when set, but auth stays **off by default**: Statix is self-hosted and single-tenant, and requiring a token to view your own telemetry is friction without benefit.
- **Read-only limits the blast radius of a breach, not the cost of a request.** Because `q` is free text and participates in the cache key, an unauthenticated caller can vary it to force cache misses (2 ClickHouse queries each) and evict hot entries from the bounded LRU — degrading ingest, which shares the same ClickHouse.
- Mitigations are input bounding and rate limiting, not authentication:
  - `q` clamped to 64 chars and normalized (trim + lowercase) before it becomes a cache key.
  - The default/unfiltered view is protected from eviction by arbitrary filters.
  - Rate limit on both dashboard routes. **Implemented as a global fixed-window
    cap, not per-IP** (revised during implementation): in Compose the gateway
    sees the Docker bridge address for every request, and behind the ALB
    ([ADR 043](../deploy/043-kubernetes-alb-tls-termination.md)) it sees the load
    balancer — so a `ConnectInfo<SocketAddr>` limiter would read as per-client
    while silently being global. A genuine per-client limit needs
    `X-Forwarded-For` plus a trusted-proxy list, which is its own decision.
    A global cap is honest and still protects ClickHouse, which is the goal.
    Env: `STATIX_DASHBOARD_RATE_LIMIT_PER_MIN` (default 600).
- Documented posture: the dashboard is unauthenticated by default and must not be exposed to the internet.

## Consequences

- **Positive:** First UI for the platform. Health strip distinguishes "pipeline broken" from "genuinely zero workloads" — otherwise identical empty tables. The `k8s_resolved` flag surfaces the still-open *"stronger cgroup → pod mapping"* item (Phase 8, [TODO.md](../../../.cursor/skills/statix-ebpf-agent/TODO.md)) and makes the K8s milestone visible: unattributed count should collapse once attribution works in-cluster.
- **Zero hot-path risk:** entirely additive on the read side. No change to `statix-ebpf`, `statix-common`, `statix` (agent), `statix-wire`, the WAL, or the ingest handler. The latency contract is untouched.
- **Negative:** Read queries share ClickHouse with the insert path — the cache is the only thing keeping dashboard load off the writer. An uncached `state` request costs two ClickHouse queries (rows + unlimited-set totals; totals must cover all matches, not just the returned page). No single-flight in v1a.
- **Known limit, accepted:** `pod` is a high-cardinality plain `String` with no index ([ADR 007](../storage/007-clickhouse-mergetree-tuning.md) rejected `LowCardinality` for OOM reasons), so text filtering is a scan *within the pruned partition*. Acceptable at current scale; first thing to hurt at large scale.
- **Operational:** new env — `STATIX_DASHBOARD_ENABLED` (default `true`), `STATIX_DASHBOARD_DIR`, `STATIX_DASHBOARD_CACHE_MS` (`2000`), `STATIX_DASHBOARD_MAX_LIMIT` (`500`).
- **Deferred:** drill-down time-series charts (v1b); agent-side signals `statix_ring_drops_total` / `statix_wal_bytes_current` (they live on each agent's `:9091` and need a scraper or a new agent→gateway health channel); SSE/WebSocket push; K8s requests/limits and right-sizing; cost attribution; process/`comm` detail (captured in the 64-byte ring record but dropped at the aggregator); rollup tables for long ranges.

## Implementation notes (added on delivery)

- **Aggregate aliases must not shadow source columns.** `max(window_start_ns) AS
  window_start_ns` makes ClickHouse resolve the inner `WHERE window_start_ns >= …`
  to the aggregate and fail with `ILLEGAL_AGGREGATION` (code 184). The inner
  query therefore aliases to `w_start` / `w_end` / `ns` / `pod_name` / …, and the
  outer projection renames back to the API names. Caught by running the SQL
  before writing the Rust; guarded by a unit test.
- **`argMax` drops the `LowCardinality` wrapper** — `argMax(namespace, …)` over
  `LowCardinality(Nullable(String))` returns plain `Nullable(String)`. `node` is
  selected through `CAST(n AS String)` so the `#[derive(Row)]` structs see plain
  types and the strict RowBinary deserializer cannot mismatch at runtime.
- Sort and order are whitelisted enums mapped to SQL literals; every user value
  (`q`, `node`, `limit`, `cutoff_ns`) is a bound `{name:Type}` parameter. A unit
  test asserts an injection attempt in `sort` falls back to the default column.

## References

- [ADR 027](../gateway/027-api-read-path-clickhouse.md) — existing read path (`/api/v1/workloads/summary`, retained unchanged)
- [ADR 007](../storage/007-clickhouse-mergetree-tuning.md) — MergeTree tuning; `pod` as plain `String`
- [ADR 059](../storage/059-phase10-clickhouse-cgroup-skip-index.md) — `cgroup_idx` minmax skip index (serves v1b drill-down)
- [ADR 060](../observability/060-phase10-golden-signal-saturation-metrics.md) — saturation series surfaced by the health strip
- [ADR 006](../ingest/006-shared-http-client-for-ingest.md), [ADR 042](../fixes/042-phase55-v2-p2-sprint-l8-fixes.md) — backoff + jitter, reused as client polling policy
- [ADR 055](../ingest/055-phase13-part1-kafka-removal-rowbinary.md) — queue-less ingest; why reads and writes share one ClickHouse

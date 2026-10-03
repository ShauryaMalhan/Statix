# ADR 075: Requests and limits per row (schema v4); upgrade by re-running `01_init.sql`

**Status:** Accepted  
**Date:** 2026-10-04  
**Extends:** [ADR 020](020-ingest-schema-version-window.md) — the accepted schema window becomes `2..=4`.  
**Serves:** Goal 1 — cost is what a service *reserves* ([docs/PRODUCT.md](../../PRODUCT.md))

## Context

The report's cost line is requests × price, and its savings line is (request − recommendation) × price. Since [ADR 074](../agent/074-match-cgroups-by-container-id.md) each container cgroup is tied to its container, and the agent's labels carry that container's requests and limits — but nothing left the agent.

Every layer has to agree on the shape: agent labels → aggregator row → wire JSON (`WorkloadRow`) → gateway (`MetricRow`) → ClickHouse columns. With clickhouse-rs 0.15 the client checks the row struct against the table, so a struct field without a column fails **every** insert. Order of rollout matters.

Until now a schema change on an existing install meant `docker compose down -v` — wiping all data. A self-hosting company cannot upgrade that way.

## Decision

1. **Four `u64` fields on every row:** `cpu_request_millicores`, `memory_request_bytes`, `cpu_limit_millicores`, `memory_limit_bytes`. **0 = not set**; the pause sandbox, CRI-O conmon and non-Kubernetes cgroups carry zeros (they reserve nothing). Storing them per row makes cost a plain sum over time — `Σ request × window length` — with pods starting, stopping and changing requests handled for free; no extra table, no joins.
2. **Schema version 3 → 4.** The fields are `#[serde(default)]`, so v2/v3 rows from older agents still parse (as zeros). The bump exists for the other direction: a v4 agent talking to a v3 gateway gets a **400** (retry, then WAL) instead of having its numbers silently dropped. The gateway's 400 message now reads the accepted range from its constants, so it cannot go stale.
3. **Upgrades by re-running `deploy/clickhouse/01_init.sql`.** The new columns are in `CREATE TABLE` (fresh installs) **and** in `ALTER TABLE … ADD COLUMN IF NOT EXISTS … AFTER …` lines at the end of the file (existing installs). Every statement is idempotent, so the one file is both installer and upgrader. No migration tool, no version table.
4. **Rollout order: database → gateway → agents.** Columns must exist before a gateway that inserts them; a gateway must accept v4 before agents send it.

### Verified (dev VM, k3s 1.36, 2026-10-04)

`01_init.sql` re-run twice against the existing volume: the four columns appear, data intact. After restart, from ClickHouse:

| pod / container | cpu req | mem req | cpu lim | mem lim |
|---|---|---|---|---|
| `reqtest` / `web` | 250 | 128 MiB | 500 | 256 MiB |
| `reqtest` / `sidecar` | 50 | 32 MiB | 0 | 0 |
| `reqtest` / pause | 0 | 0 | 0 | 0 |
| `guaranteed-test` / `app` | 100 | 64 MiB | 100 | 64 MiB |

Zero gateway insert errors. Unit tests: v2 and v3 rows still parse; v4 round-trips all four fields.

## Alternatives considered

- **No version bump, fields optional only.** Rejected: a v4 agent against a v3 gateway would be accepted and silently lose requests.
- **A separate pod-spec table joined at query time.** Rejected: a new table, a join in every report query, and time-windowed history of spec changes to maintain — per-row columns give that history for free.
- **Keep `down -v` for dev and write migrations "later".** Rejected: the first company upgrade would be the first time the upgrade path ran.

## Consequences

- **Positive:** cost and savings are now computable straight from `statix.workload_metrics`. Installs upgrade without losing data.
- **Negative:** four extra `UInt64` per row (compresses well — mostly repeated values).
- **Rule going forward:** every schema change adds both the column in `CREATE TABLE` and an idempotent `ALTER … IF NOT EXISTS` at the end of `01_init.sql`.

## References

- [ADR 020](020-ingest-schema-version-window.md) — schema version window
- [ADR 058](../agent/058-phase14-cpu-usage-tracking.md) — the previous field added the same way (`cpu_usage_usec`, v3)
- [ADR 074](../agent/074-match-cgroups-by-container-id.md) — where the per-container requests/limits come from

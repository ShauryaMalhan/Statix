# Statix — Open work

**Only open items live here.** Shipped history was removed on 2026-09-15 and is not lost —
every decision is in [docs/adr/INDEX.md](../../../docs/adr/INDEX.md) (grouped by topic)
and every change is in `git log`. This file answers "what needs doing", not
"what did we do".

Roughly priority-ordered. `file:line` refs are the entry point for each item.

---

## P0b — Memory and CPU numbers: remaining accuracy gaps

Found while fixing tree double-counting, the sample/flush race and phantom bootstrap execs
([ADR 066](../../../docs/adr/agent/066-sample-leaf-cgroups-only.md),
[067](../../../docs/adr/agent/067-sample-inside-flush.md),
[068](../../../docs/adr/agent/068-bootstrap-registers-only.md)).

- [ ] **CPU is 0 in the first window after agent start.** A rate needs two readings; the
      first only primes `cpu_baseline` (ADR 058). Memory is already right in the first
      window (ADR 067). **Fix:** prime `cpu_baseline` during startup **and** start the flush
      timer one full window after startup (`interval_at(now + window)`). Priming alone is
      not enough, because tokio's first `tick()` fires immediately and would divide a real
      delta by a window only milliseconds long.
      Dashboard side, only a plain "waiting for first window…" state while
      `/api/v1/dashboard/state` returns no workloads. Do **not** hide rows client-side "until
      the 2nd flush": the dashboard has no way to know which flush it is (stateless, many
      nodes, agent restarts), and the zeros would still be stored in ClickHouse for billing.

- [ ] **Occasionally one row per window has execs but no sample.** Seen 2026-09-24 in three
      mid-run windows (16:15, 18:29, 19:11; `unsampled_rows = 1`, not restarts). A cgroup
      exec'd but was not read when the window closed. Unverified guess: a short-lived cgroup
      (e.g. from `docker exec`) removed before the close, so `is_leaf_cgroup`'s `stat` fails
      and it is skipped. Check by logging the path of any cgroup that has execs but no sample.
      If confirmed, it is correct behaviour (nothing left to measure) and only needs a note.

- [ ] **"Memory" counts page cache, so even leaf-only totals overstate what workloads need.**
      `memory.current` includes file cache, which the kernel drops whenever programs need
      the RAM. Measured on the dev VM (2026-09-24, leaves only, summed from `memory.stat`):

      ```
      free -m  used           2226 MiB
      leaves   anon           1709 MiB   program memory
      leaves   file           4547 MiB   page cache — reclaimable
      leaves   memory.current 6800 MiB   = anon + file + ~540 MiB kernel
      ```

      Kubernetes (`kubectl top`, the kubelet's eviction logic) reports **working set** =
      `memory.current − inactive_file`. Recommended: bill on working set, so Statix matches
      what users compare it against. New column (read `memory.stat`), needs an ADR.

- [ ] **The `container` label is wrong on sandbox cgroups.** The pause container is
      labelled with the application container's name, because the name comes from the pod
      spec rather than from the cgroup path. Its memory is real and small (0.2 MB), and after
      ADR 066 it is no longer double-counted — only the label is wrong.

---

## P1 — Dashboard hardening (Phase 15 follow-up, [ADR 061](../../../docs/adr/ui/061-phase15-dashboard-read-tier.md))

Shipped v1a knowingly deferred these. They are written down because they are real, not
because they are urgent on a laptop — most matter the moment the gateway is reachable by
anyone but you.

- [ ] **No security headers on the served page.** `GET /` returns only `content-type`.
      Missing `Content-Security-Policy`, `X-Frame-Options`/`frame-ancestors`,
      `X-Content-Type-Options: nosniff`, `Referrer-Policy`. **Clickjacking works today.**
      CSP is also the defence-in-depth that would contain any XSS that slips through.
      (`statix-gateway/src/routes/dashboard/mod.rs` — `page_handler`)
- [ ] **Unauthenticated by default, and the data is a map of your infrastructure.** The
      dashboard lists every namespace, pod, container and node — internal service topology,
      team structure, and traffic patterns. Decide a posture: bind loopback unless a token
      is set, or require the token for non-local requests. Today the only protection is a
      README sentence.
- [ ] **Rate limit is global, not per-client.** One abusive caller starves every legitimate
      one. A real per-client limit needs `X-Forwarded-For` plus a trusted-proxy list —
      deliberately deferred (see ADR 061), but it is a genuine gap, not a solved problem.
      (`routes/dashboard/cache.rs` — `RateLimiter`)
- [ ] **No XSS regression test.** Every interpolated field currently goes through `esc()`,
      and all of them sit in *text* context, so it is safe as written. Two reasons it still
      needs a test: `esc()` does not escape `'`, so anyone later moving a value into an
      *attribute* creates a hole; and in a shared cluster pod/namespace names are
      attacker-influenceable input. Add a test that a pod named
      `<img src=x onerror=alert(1)>` renders inert.
- [ ] **`/api/v1/dashboard/health` node list is unbounded.** `sql::nodes()`
      (`routes/dashboard/sql.rs:186`) has `ORDER BY node` and **no `LIMIT`**. At 1,000+
      nodes every client returns the whole fleet every 5s. Cap it, and return a count plus
      the worst-N by staleness instead of everything.
- [ ] **Health query scans a 1-hour window on every cache miss.** Fine at one node; at fleet
      scale it is the most expensive thing the dashboard does. Bound the horizon or
      pre-aggregate.

---

## P2 — Dashboard v1b

- [ ] **Drill-down charts** — `GET /api/v1/dashboard/workload/{cgroup_id}/series`, then a
      time-series view on row click. Cheap: the `cgroup_idx` minmax skip index
      ([ADR 059](../../../docs/adr/storage/059-phase10-clickhouse-cgroup-skip-index.md))
      exists precisely for this filter.
- [ ] **Single-flight on cache miss.** TTL alone collapses steady state; the stampede window
      is the instant after expiry. Add only if measurement shows it matters.
- [ ] **Agent-side signals in the health strip** — `statix_ring_drops_total` and
      `statix_wal_bytes_current` live on each agent's `:9091`, and the gateway has no agent
      registry or scraper. Needs Prometheus or an agent→gateway health channel. Until then
      the strip cannot show ring-buffer loss, which is the one metric SKILL.md says to
      always investigate.
- [ ] **Integration test against a real ClickHouse.** 19 unit tests cover parsing, clamping,
      cache and SQL shape — none execute a query. The two bugs found during Phase 15
      (alias shadowing → `ILLEGAL_AGGREGATION`, `argMax` dropping `LowCardinality`) were
      both invisible to unit tests and to the compiler.

---

## P3 — Kubernetes (the milestone that makes attribution real)


- [ ] **Stronger cgroup → pod mapping** *(Phase 8, long-open)*. The dashboard now makes this
      failure visible for the first time; expect it to surface the moment k3s is running.
- [ ] **K8s requests/limits → right-sizing.** Extend the pod watcher to read `resources`,
      add a wire field and column, then show "uses 200m of its 500m request". This is the
      single most valuable thing a FinOps dashboard displays, and it is a data-collection
      project, not a UI one.

---

## P4 — Portability


- [ ] **arm64 eBPF in CI.** Verified working by hand on 2026-09-15 — aarch64, Ubuntu 24.04,
      kernel 6.8, `eBPF program loaded and kernel-verified`. The CI matrix is still x86-only,
      so nothing stops an arm64 regression. Required for Graviton and every Apple Silicon
      dev machine.
- [ ] **cgroup v1-only host detection.** Graceful error and a clear log instead of silent
      failure.

---

## P5 — Observability

- [ ] **Extended agent metrics:** flush duration, retry queue depth, attribution cache size,
      ring drain-budget hits.
- [ ] **Phase 5 production ops tuning** — remaining hardening before a real deployment.

---

## P6 — Developer experience

### Run scripts: `dev-up` / `dev-down` / `dev-status`

> **Why this is worth doing properly.** Bringing the stack up is four pieces that must
> start in dependency order, half on the Mac and half inside the VM, with a health wait in
> the middle. It was rebuilt by hand on 2026-09-19 and every one of the gotchas below cost
> real time. `bootstrap.sh` + `make deps` already solve *installing*; this solves *running*.
>
> Target: **one command from a cold laptop to a live dashboard.**


- [ ] **Wire them to the Makefile** — `make dev-up` / `dev-down` / `dev-status`, and fix
      `make compose-up` at the same time (`Makefile:18` hardcodes `docker compose`).

#### Gotchas the scripts must handle — each of these actually bit

| Gotcha | What happens without it |
|---|---|
| **Compose spelling differs per machine.** Mac has `docker-compose`, the VM has `docker compose`. Neither has both. | `command not found`, on whichever machine you are on |
| **Detect macOS and re-exec inside the VM.** The agent needs Linux; the gateway should sit next to it. | agent cannot load eBPF at all |
| **Wait for ClickHouse `(healthy)`, not `Up`.** It reports `Up` ~10s before it accepts queries. | gateway starts, fails its ping, `ch_healthy=false`, every ingest 503s |
| **Read `CLICKHOUSE_PASSWORD` from `.env`.** | auth failures that look like the DB is down |
| **Never `pkill -f <pattern>` where the pattern matches the ssh command itself.** | kills your own session — `exit status 255`, seen live |
| **`sudo` strips the environment.** Use `sudo env VAR=… ./binary`. | agent starts with defaults and silently posts nowhere |
| **Lima forwards VM ports to the Mac automatically.** | people hunt for a forwarding step that does not exist |

#### Done when

Cold laptop → `./scripts/dev-up.sh` → dashboard shows live rows, with no manual step and
no second copy of anything if run twice. `dev-status.sh` then says so in one screen.

- [ ] **Process/binary detail (`comm`).** The kernel already captures `comm[16]` and `cpu_id`
      in the 64-byte ring record (`statix-common/src/lib.rs`) — the aggregator throws both
      away. Plumbing them through is a 5-crate change; consider cardinality first.

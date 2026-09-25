# Statix — Open work

**Only open items live here, ordered by the product goals in
[docs/PRODUCT.md](../../../docs/PRODUCT.md)** ([ADR 070](../../../docs/adr/meta/070-product-scope-self-hosted-waste-report.md)).
Once an item is shipped and pushed, **delete it** — every decision is in
[docs/adr/INDEX.md](../../../docs/adr/INDEX.md) and every change is in `git log`.

Before building any item: which goal does it serve, and what kernel fact does it depend on?
Verify that fact on a real node first. `file:line` refs are the entry point for each item.

---

## Goal 1 — Right-size pods by need

Usage + pressure vs each pod's requests/limits. Numbers first: a waste report nobody trusts
is useless.

- [ ] **Pressure signals: is the pod starving, not just busy?** Usage alone cannot tell
      "needs more" from "is fine". Four kernel counters answer it, all cumulative, so each
      becomes a per-window delta exactly like CPU (ADR 058):
      - **CPU capping:** `cpu.stat` `nr_throttled` / `throttled_usec`, in the file the agent
        already reads (it parses only the first line today)
      - **CPU wait:** `cpu.pressure` `some ... total=` (µs of stall)
      - **memory pressure:** `memory.pressure` `some ... total=`
      - **OOM kills:** `memory.events` `oom_kill`
      Verified present on the dev VM (kernel 6.8, 2026-09-26). Some distros ship PSI disabled
      by default (needs the `psi=1` boot parameter), so a missing `*.pressure` file must be a
      soft miss, like `cpu.stat` today. New columns → wire + schema change; needs an ADR.

- [ ] **K8s requests/limits → right-sizing.** Extend the pod watcher to read `resources`,
      add a wire field and column, then show "uses 200m of its 500m request". This is the
      single most valuable thing a FinOps dashboard displays, and it is a data-collection
      project, not a UI one.

- [ ] **Stronger cgroup → pod mapping** *(Phase 8, long-open)*. The dashboard now makes this
      failure visible for the first time; expect it to surface the moment k3s is running.

- [ ] **The `container` label is wrong on sandbox cgroups.** The pause container is
      labelled with the application container's name, because the name comes from the pod
      spec rather than from the cgroup path. Its memory is real and small (0.2 MB), and after
      ADR 066 it is no longer double-counted — only the label is wrong.

---

## Goal 2 — Count the CPU of short-lived jobs

- [ ] **CPU used by cgroups that exit between samples is lost.** CPU is read once per
      window (ADR 067). A cgroup deleted before the window closes is never read, and since
      only leaves are sampled (ADR 066) its CPU is not in any total either. Every workload
      also loses the CPU it used between its last sample and its exit (up to one window).
      Matters for **K8s Jobs and CronJobs**, which are exactly this shape.
      **Evidence (dev VM, 2026-09-24/25):** lone rows with `exec_count > 0, sample_count = 0`
      in six windows; the one at 00:36:09 UTC (47 execs) matches `apt-daily-upgrade.service`
      starting 06:06:18 IST, 9 s later in the same window; `find -inum` confirms the folder is
      gone. The six `cgroup_id`s rise by exactly 60 each (a new cgroup folder uses ~60 inodes
      for its interface files), so **every** cgroup created over ~15 h was a short-lived timer
      job, and all of them were missed.
      **Depends on a kernel fact, verify first:** a parent's `cpu.stat` `usage_usec` keeps a
      child's CPU after the child's folder is removed.
      **Proposed fix: leftover CPU per parent.**
      - read `cpu.stat` for every registered cgroup, `memory.current` for leaves only
      - per parent: leftover = parent delta − Σ direct children's deltas (`saturating_sub`,
        reads are not simultaneous); emit as the parent's own row, CPU only, memory 0
      - leaves + leftovers add up to the top-level totals, so nothing is double-counted and
        ADR 066 still holds for memory; no wire or schema change
      - also fixes ADR 069's remaining gap: a cgroup created after startup has its first
        window's CPU in its parent's delta, so it lands in the leftover
      **Constraints:** `prime()` and `cpu_baseline` pruning must include parents, or the
      first leftover after a start or a tree change is a spike or a zero. Decide whether
      leftover rows count as "sampled" (the `unsampled_rows` query depends on it). The root
      cgroup is never read, so a short-lived cgroup directly under `/` stays lost.
      **Honest limit:** CPU is attributed to the nearest parent that still exists: an
      `apt-daily-upgrade` run shows as `system.slice`, and a finished K8s Job pod as its QoS
      slice, not its namespace.
      **Rejected:** read `cpu.stat` on a BPF process-exit event (a race: systemd removes the
      folder ms after it empties, and reads fail after removal). **Future option:** per-cgroup
      CPU accounting in BPF on context switch, which is exact and keeps the Job's own labels,
      but is a large hot-path and verifier change.
      Needs an ADR.

---

## Goal 3 — Find zombie pods

- [ ] **Per-pod network bytes, so "no traffic" can be measured.** Zombie = near-zero CPU
      **and** near-zero network over days. cgroupfs has no network counters. Lightest option:
      each pod has its own network namespace, so `/proc/<pid>/net/dev` of any process in the
      pod shows that pod's interface byte counters — no eBPF. **Verify on k3s first** (host
      network pods share the node's namespace and must be excluded). Cumulative → per-window
      delta. Wire + schema change; needs an ADR. The zombie report itself is then a query.

---

## Goal 4 — Egress cost per workload

- [ ] **Egress bytes per workload, split by destination class.** `net/dev` counts bytes
      but not *where* they went, and cost depends on it: same-AZ (free), cross-AZ, internet,
      via NAT. Needs per-destination accounting — eBPF (`cgroup_skb/egress`, keyed by cgroup
      and destination class) or conntrack — plus each node's zone
      (`topology.kubernetes.io/zone`) and the VPC's CIDRs to classify destinations. The
      largest item on the list and the first real new eBPF surface since exec tracing:
      **design ADR first, and only after goals 1–3.**

---

## Self-hosted readiness — a company runs this in its own cluster

Shipped v1a knowingly deferred these. They are written down because they are real, not
because they are urgent on a laptop — most matter the moment the gateway is reachable by
anyone but you.

- [ ] **Unauthenticated by default, and the data is a map of your infrastructure.** The
      dashboard lists every namespace, pod, container and node — internal service topology,
      team structure, and traffic patterns. Decide a posture: bind loopback unless a token
      is set, or require the token for non-local requests. Today the only protection is a
      README sentence.
- [ ] **No security headers on the served page.** `GET /` returns only `content-type`.
      Missing `Content-Security-Policy`, `X-Frame-Options`/`frame-ancestors`,
      `X-Content-Type-Options: nosniff`, `Referrer-Policy`. **Clickjacking works today.**
      CSP is also the defence-in-depth that would contain any XSS that slips through.
      (`statix-gateway/src/routes/dashboard/mod.rs` — `page_handler`)
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
- [ ] **Rate limit is global, not per-client.** One abusive caller starves every legitimate
      one. A real per-client limit needs `X-Forwarded-For` plus a trusted-proxy list —
      deliberately deferred (see ADR 061), but it is a genuine gap, not a solved problem.
      (`routes/dashboard/cache.rs` — `RateLimiter`)

---

## Engineering health

- [ ] **arm64 eBPF in CI.** Verified working by hand on 2026-09-15 — aarch64, Ubuntu 24.04,
      kernel 6.8, `eBPF program loaded and kernel-verified`. The CI matrix is still x86-only,
      so nothing stops an arm64 regression. Required for Graviton and every Apple Silicon
      dev machine.
- [ ] **cgroup v1-only host detection.** Graceful error and a clear log instead of silent
      failure.
- [ ] **Integration test against a real ClickHouse.** 19 unit tests cover parsing, clamping,
      cache and SQL shape — none execute a query. The two bugs found during Phase 15
      (alias shadowing → `ILLEGAL_AGGREGATION`, `argMax` dropping `LowCardinality`) were
      both invisible to unit tests and to the compiler.

### Run scripts: `dev-up` / `dev-down` / `dev-status`

> **Why this is worth doing properly.** Bringing the stack up is four pieces that must
> start in dependency order, half on the Mac and half inside the VM, with a health wait in
> the middle. It was rebuilt by hand on 2026-09-19 and every one of the gotchas below cost
> real time. `bootstrap.sh` + `make deps` already solve *installing*; this solves *running*.
>
> Target: **one command from a cold laptop to a live dashboard.**


- [ ] **Wire them to the Makefile** — `make dev-up` / `dev-down` / `dev-status`, and fix
      `make compose-up` at the same time (`Makefile:18` hardcodes `docker compose`).
- [ ] **`dev-up.sh` silently runs a stale binary.** It starts whatever is in
      `target/release/` and never builds. On 2026-09-26 a working-set change was "tested"
      against a two-day-old binary because `make build` hadn't produced a new one, and the
      unchanged number looked like a code problem. Fix: warn (or refuse) when any `*.rs`
      under `statix/src` or `statix-gateway/src` is newer than the binary — or just run
      `make build` first.
- [ ] **`dev-up.sh`'s "agent is reporting" check can pass on the previous run's data.** It
      counts rows whose window started in the last **60 s**, not rows written by **this**
      agent. After a quick restart, the old agent's last windows satisfy it, so a new agent
      that is broken still prints "agent is reporting". Fix: record the start time before
      launching the agent and count only rows with `window_start_ns` after it (or filter on
      the `batch_id` / `agent_version` of this run).

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

---

## Parked — serves none of the goals yet

Not deleted: each may become relevant later, but none is on the path to the four goals
(ADR 070). Pick one up only when a goal needs it.

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
- [ ] **Extended agent metrics:** flush duration, retry queue depth, attribution cache size,
      ring drain-budget hits.
- [ ] **Phase 5 production ops tuning** — remaining hardening before a real deployment.
- [ ] **Process/binary detail (`comm`).** The kernel already captures `comm[16]` and `cpu_id`
      in the 64-byte ring record (`statix-common/src/lib.rs`) — the aggregator throws both
      away. Plumbing them through is a 5-crate change; consider cardinality first.

**Later, not now (product):** automatic in-place resizing, ML forecasting.

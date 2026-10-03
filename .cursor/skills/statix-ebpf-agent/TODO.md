# Statix — Open work

**Only open items live here, ordered by the product goals in
[docs/PRODUCT.md](../../../docs/PRODUCT.md)** ([ADR 070](../../../docs/adr/meta/070-product-scope-self-hosted-waste-report.md)).
Once an item is shipped and pushed, **delete it** — every decision is in
[docs/adr/INDEX.md](../../../docs/adr/INDEX.md) and every change is in `git log`.

Before building any item: which goal does it serve, and what kernel fact does it depend on?
Verify that fact on a real node first. `file:line` refs are the entry point for each item.

---

## Goal 1 — Cost and savings per service

The report in [PRODUCT.md](../../../docs/PRODUCT.md): per service, what it costs, its p95/p99/max
CPU and memory, and what right-sizing to p95 would save. Statix recommends; it never
resizes. Numbers first: a report nobody trusts is useless.

- [ ] **Requests/limits per pod.** "Reserves 1000m / 2 GiB" is what the company pays for,
      so it is the cost basis. Extend the pod watcher (`watch_k8s_pods`) to read each
      container's `resources.requests` / `resources.limits`, add wire fields + columns.
      Pods with no requests set need a stated rule (cost them at usage, and flag "no
      requests set" as its own insight).
      **Found 2026-09-27 on k3s (containerd, systemd driver):** container cgroups are
      `…/kubepods-burstable-pod<uid_>.slice/cri-containerd-<64hex>.scope`, and the 64-hex ID
      equals `containerStatuses[].containerID` after `containerd://`; the pause container is
      the one folder whose ID is in no status list. Two live bugs:
      - `extract_container_from_path` looks for `cri-container-` (missing `d`), so it never
        matches; every container gets the pod's **first** container name (the real cause of
        the pause-label bug, and `sidecar` was mislabelled too).
      - `extract_pod_uid_from_path` splits on `-pod`; a Guaranteed pod's
        `kubepods.slice/kubepods-pod<uid>.slice` (verified on k3s, no QoS level) has no `-pod`
        after the prefix, so **every Guaranteed pod is unattributed**.
      **Stages:** ✅ 1a parsers + tests (local commit) · ✅ 1b dev access via
      `STATIX_DEV_KUBECONFIG`, node name `colima` (ADR 072, local commit) · ✅ `kube` 4.2 +
      `k8s-openapi` 0.28, min Kubernetes 1.32 (ADR 073, local commit) · ✅ 1c-i container ID → name mapping (ADR 074, local commit; also
      resolves the two items below — delete them at push) · ✅ 1c-ii requests/limits per container
      + quantity parser (ADR 074 follow-up, local commit) · **next:** 2 pipeline (wire v4, gateway,
      columns + ALTER). Push when the whole item is done.
      **Design — independent of runtime and driver:** match cgroup → container by the 64-hex
      ID (folder name minus `.scope`, text after the last `-`, must be 64 hex), not by prefix;
      read `containerStatuses` + `initContainerStatuses` + `ephemeralContainerStatuses`;
      skip CRI-O's `crio-conmon-<id>` for requests (same ID, would double-charge); parse the
      pod UID as `pod` + 36-char UUID with `_` or `-`. Unit-test every layout (containerd /
      CRI-O / Docker × systemd / cgroupfs × Guaranteed / Burstable). Add
      `statix_k8s_unmatched_cgroups` so an unknown layout is visible, not silently wrong.
      Supersedes the "stronger cgroup → pod mapping" and pause-label items below.

- [ ] **Group pods by service (owner), not pod name.** Pod names change on every deploy
      (`checkout-api-7d9f8c-x2k4q`), so a report by pod is unreadable and loses history. Read
      the pod's `ownerReferences` (ReplicaSet → Deployment, StatefulSet, DaemonSet, Job) in
      the pod watcher and store an `owner_kind` / `owner_name` label. Resolving ReplicaSet →
      Deployment either needs one more watch, or the naming convention (strip the hash
      suffix) — decide in the ADR.

- [ ] **Price per vCPU-hour and per GiB-hour.** Self-hosted, so the company supplies its
      own rates (gateway config / env), with a documented default (e.g. a public on-demand
      list price) clearly labelled as an estimate. No cloud billing API — stay light.

- [ ] **The report: cost, recommendation and savings per service.** Rules in
      [PRODUCT.md](../../../docs/PRODUCT.md): CPU → p95 + 15%; memory → highest daily peak
      + 15% (never a percentile of all samples: memory OOMs, CPU only slows); both over ≥7
      days. cost = requests × price × hours; savings = (request − recommendation) × price ×
      hours; p95/p99/max shown underneath as evidence. A ClickHouse query over the
      per-window rows, grouped by service, joined with requests and price, then a page that
      reads like the PRODUCT.md example. Percentiles already work on today's data; cost and
      savings need the three items above.
      **Exclude host-pause windows** (see Parked): `WHERE window_end_ns -
      window_start_ns < 3 × window` — a paused window's CPU per second is far too low.

- [ ] **Pressure signals — what makes "increase this one" possible.** Right after the first
      report. Usage alone can only say *shrink*: a service throttled at its limit cannot use
      more than the limit, so its p95 looks fine while it starves. Usage alone cannot tell
      "needs more" from "is fine". Four kernel counters answer it, all cumulative, so each
      becomes a per-window delta exactly like CPU (ADR 058):
      - **CPU capping:** `cpu.stat` `nr_throttled` / `throttled_usec`, in the file the agent
        already reads (it parses only the first line today)
      - **CPU wait:** `cpu.pressure` `some ... total=` (µs of stall)
      - **memory pressure:** `memory.pressure` `some ... total=`
      - **OOM kills:** `memory.events` `oom_kill`
      Files verified present on the dev VM (kernel 6.8, 2026-09-26); all read 0 there because
      nothing was constrained. Before building: make a unit struggle on purpose
      (`systemd-run -p CPUQuota=20%`, `-p MemoryMax=50M` + `tail /dev/zero`) and confirm each
      counter moves. Some distros ship PSI disabled
      by default (needs the `psi=1` boot parameter), so a missing `*.pressure` file must be a
      soft miss, like `cpu.stat` today. New columns → wire + schema change; needs an ADR.

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

- [ ] **A host pause makes one window span minutes, and the dashboard table blinks empty.**
      Parked 2026-09-28: no data is lost, the table self-heals within one window, the effect on
      a 7-day p95 is negligible, and servers rarely pause. The report query's ≤ 3 × window
      filter covers the one real effect.
      When the VM freezes (Mac sleep; on servers, a live migration) and the clock is corrected
      on resume, the window open during the pause ends "now" but started minutes ago. Seen
      2026-09-27: 10 windows of 62–751 s instead of 10 s; one of 534 s matches
      `lima-guestagent` "drift was -8m43.9s" to the second. Effects: (1) the dashboard state
      query filters `window_start_ns >= cutoff` while health uses `window_end_ns`, so the
      table shows 0 workloads for one refresh while health says "data 1s old" — filter state
      on `window_end_ns` too (`routes/dashboard/sql.rs`, `inner`); (2) that window's CPU
      per second is far too low (≈10 s of activity spread over 534 s) — the report must
      exclude windows longer than ~3 × `STATIX_WINDOW_SECS`. No agent change needed now;
      if it ever matters on servers, the agent can detect it (wall elapsed ≫ monotonic
      elapsed) and flag the row.
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

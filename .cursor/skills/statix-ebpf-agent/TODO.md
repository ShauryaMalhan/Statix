# Statix — Open work

**Only open items live here.** Shipped history was removed on 2026-09-15 and is not lost —
every decision is in [docs/adr/INDEX.md](../../../docs/adr/INDEX.md) (61 ADRs, grouped by
topic) and every change is in `git log`. This file answers "what needs doing", not
"what did we do".

Roughly priority-ordered. `file:line` refs are the entry point for each item.

---

## P0 — Supply chain: the same bug in three more places

CI run #36 failed because `cargo install bpf-linker` had no version pin: 0.11.0 dropped
`aya-rustc-llvm-proxy`, so it needs a system LLVM 21+ that the runner doesn't have. Fixed
in CI (`9856d7e`) — **but the identical bug is still live in three other places.** A cache
was hiding it for months; these will fail the same way, on someone else's machine or in a
production image build.

- [ ] **Two gateway Dockerfiles duplicate each other** — `deploy/docker/Dockerfile.gateway`
      (production) and `Dockerfile.gateway` (dev compose). Every base-image or dependency
      change has to be made twice and can silently drift. Collapse to one, or generate the
      dev one from the prod one.
- [ ] **`.github/workflows/ebpf-ci.yml:69`** — `pip install --break-system-packages virtme-ng`,
      unpinned. Same shape, different ecosystem.
- [ ] **Enable Dependabot** — `.github/dependabot.yml` for `cargo`, `github-actions` and
      `docker`. Pinning stops things moving silently; Dependabot is what makes the pins
      *maintainable* — it opens one PR per bump and CI judges it, so upgrades arrive as a
      reviewable green/red signal instead of a surprise on an unrelated push. Without it,
      pins go stale invisibly and nothing ever tells us a security fix is available.
- [ ] **Add `cargo audit` to CI** — fails the build on a known RustSec advisory in any
      dependency. Cheap, and it is the only thing that currently would tell us a
      vulnerable crate is in the tree ([ADR 062](../../../docs/adr/meta/062-pin-build-toolchain-versions.md)).

> **Rule learned:** `--locked` pins the dependency *tree*, not the *version* of the crate
> being installed. And a pinned dependency sitting behind a cache isn't pinned — it's a
> time bomb with a slow fuse.

---

## P0b — cgroup hierarchy is double-counted (found 2026-09-21 in k3s)

> Surfaced the moment attribution started working: the dashboard reported **15 GiB** of
> memory on a VM with **7.7 GiB** installed and **2.4 GiB** actually in use — roughly 6x over.

- [ ] **Totals sum a tree, so parents and children are both counted.** cgroups are
      hierarchical and a parent's `memory.current` already includes every descendant.
      Measured on the node:

      ```
      /user.slice                       669 MiB
        /user.slice/user-501.slice      669 MiB   same bytes
          /.../session-4.scope          664 MiB   same bytes again
      /docker                          1698 MiB
        /docker/<clickhouse>           1492 MiB   subset of the above
      ```

      Every fleet-wide `sum()` — the dashboard tiles, `/api/v1/workloads/summary`'s
      `total_cpu_usec` — is inflated by an unknown factor that depends on tree depth.

- [ ] **Each Kubernetes pod appears 2-3 times.** A pod has its own cgroup plus one per
      container, and all of them resolve to the same `namespace/pod/container` labels.
      Verified for one pod: `8537` (pod slice, 24.5 MB), `9044` (pause sandbox, 0.2 MB),
      `9609` (the real container, 24.3 MB) — and 24.5 ≈ 0.2 + 24.3.

- [ ] **The `container` label is wrong on sandbox cgroups.** The pause container is
      labelled with the application container's name, because the name comes from the pod
      spec rather than from the cgroup path.

### Why no query can fix this

**The aggregator discards the cgroup path.** ClickHouse stores `cgroup_id` (an inode) and
nothing else — no parent, no depth, no path. The read side therefore *cannot* distinguish
a pod cgroup from a container cgroup, or a parent from a leaf. This has to be fixed where
the data is produced, not where it is consumed.

Options, roughly in increasing cost:

- **sample only leaf cgroups** — `bootstrap_existing_cgroups` currently registers every
  directory it walks; skipping any directory that has child cgroups would remove most of
  the double-counting, at the cost of losing the "whole slice" rollups
- **emit a `depth` or `parent_cgroup_id` column** so the read path can filter to leaves —
  a wire + schema change, but it keeps both views available
- **for K8s specifically, prefer the container-level cgroup** and drop pod-level rows when
  a container-level row exists for the same pod
- **stop showing fleet-wide totals** until one of the above lands — the tiles are actively
  misleading today

Whichever is chosen needs an ADR; this is a data-model decision, not a bug fix.

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

- [ ] **Validate in a real cluster.** Run k3s inside the Colima VM and deploy
      `deploy/k8s/*.yaml`. **Success criterion: the dashboard's `unattributed` count
      collapses toward zero.** On the VM today all 41 cgroups are unattributed because the
      agent logs `Not in K8s — pod watch disabled` — that badge is the progress bar.
- [ ] **Stronger cgroup → pod mapping** *(Phase 8, long-open)*. The dashboard now makes this
      failure visible for the first time; expect it to surface the moment k3s is running.
- [ ] **K8s requests/limits → right-sizing.** Extend the pod watcher to read `resources`,
      add a wire field and column, then show "uses 200m of its 500m request". This is the
      single most valuable thing a FinOps dashboard displays, and it is a data-collection
      project, not a UI one.

---

## P4 — Portability

- [ ] **Clock offset survives a host suspend for up to an hour, stamping every row wrong.**
      Observed live on 2026-09-20: the agent emitted windows **26 minutes in the past**,
      advancing at exactly 1.00x real time, with no WAL backlog. Restarting the agent fixed
      it instantly.

      **Mechanism.** The agent reports `wall = bpf_monotonic_timestamp + clock_offset`, and
      caches `clock_offset` at startup. When the Mac sleeps, the Virtualization framework
      *pauses* the VM — the guest sees no suspend event, so `CLOCK_MONOTONIC` and
      `CLOCK_BOOTTIME` both simply freeze (measured: identical, no suspend recorded). On
      wake, NTP steps the **wall** clock forward by the sleep duration; monotonic is never
      corrected. The cached offset is now too small by exactly that duration, and every
      window is stamped that far in the past until the next recalibration.

      Evidence: VM wall-age 26.7h vs `CLOCK_MONOTONIC` uptime 19.35h — **7.3 hours of wall
      time the monotonic clock never counted**.

      **Why [ADR 047](../../../docs/adr/agent/047-atomic-clock-offset-recalibration.md) does
      not cover it.** That ADR handles NTP *drift* — slow, small, and well served by an
      hourly tick. A host suspend is a *step*: sudden and large. Hourly recalibration does
      eventually correct it, but every row written in the meantime is permanently
      mis-stamped in ClickHouse, and there is no signal that it happened.

      **Not a laptop-only curiosity.** Every developer running the agent on a Mac hits this
      the first time they open the lid. VM live-migration and hypervisor pauses produce the
      same shape on servers.

      **Options** (needs an ADR superseding 047, per the project rule — do not edit 047):
      - recalibrate far more often; it is two clock reads and essentially free
      - on each recalibration, compare new offset against old and if it jumped beyond a
        threshold, apply immediately and emit a metric/log rather than absorbing it silently
      - consider `bpf_ktime_get_boot_ns()` (`CLOCK_BOOTTIME`) instead of
        `bpf_ktime_get_ns()` — note it does **not** help this case, since a hypervisor pause
        freezes both, but it does cover a genuine guest suspend
      - add a `statix_clock_offset_step_seconds` metric so the correction is observable


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

- [ ] **`scripts/dev-up.sh`** — start everything, in order, idempotently:
      1. `colima start` if the VM is not running (reuses saved cpu/memory/disk config)
      2. ClickHouse via compose, then **wait for `(healthy)`** — not just `Up`
      3. gateway (background, log to a file), then wait for `/ready` to return 200
      4. agent under `sudo` (needs `CAP_BPF`), then wait for a row to land
      5. print the dashboard URL and where the logs are
      Re-running it while things are already up must be a no-op, not a second copy.

- [ ] **`scripts/dev-down.sh`** — stop agent, then gateway, then optionally ClickHouse.
      Default keeps ClickHouse (and therefore the data); `--all` stops it too.

- [ ] **`scripts/dev-status.sh`** — one screen: VM up? ClickHouse healthy? gateway `/ready`?
      agent running? newest row age? unattributed count? Answers "is it working" without
      remembering any `curl`.

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

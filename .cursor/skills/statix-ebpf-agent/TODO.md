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
- [ ] **`Makefile:31`** — `which bpf-linker || cargo install bpf-linker`, unpinned. A fresh
      dev box gets 0.11.x and cannot build the agent at all.
- [ ] **`.github/workflows/ebpf-ci.yml:69`** — `pip install --break-system-packages virtme-ng`,
      unpinned. Same shape, different ecosystem.
- [ ] **Write down the pin policy.** There is no rule anywhere saying tool versions must be
      pinned, which is why this happened four times. One line in SKILL.md.

> **Rule learned:** `--locked` pins the dependency *tree*, not the *version* of the crate
> being installed. And a pinned dependency sitting behind a cache isn't pinned — it's a
> time bomb with a slow fuse.

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

- [ ] **`make compose-up` is broken on macOS.** The Makefile uses `docker compose` (the
      plugin, `Makefile:18`); a Mac with Colima typically has standalone `docker-compose`.
      Detect which exists and use it, rather than every Mac dev working around it by hand.
- [ ] **Process/binary detail (`comm`).** The kernel already captures `comm[16]` and `cpu_id`
      in the 64-byte ring record (`statix-common/src/lib.rs`) — the aggregator throws both
      away. Plumbing them through is a 5-crate change; consider cardinality first.

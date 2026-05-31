# FinOps eBPF Agent — Patterns

Enterprise templates. **Before coding:** [SKILL.md](SKILL.md) workflow → implement → `make build` → update ADR/docs/skills.

Rules: [enterprise-latency.md](../../../docs/enterprise-latency.md). Architecture: [REFERENCE.md](REFERENCE.md).

---

## Pattern 1 — `FinopsEvent` in finops-common

```rust
pub const EVENT_KIND_WORKLOAD_IDENTITY: u8 = 1;
pub const EVENT_KIND_MEMORY_SAMPLE: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FinopsEvent { /* kind, cgroup_id, timestamp, memory_bytes, comm, ... */ }

#[cfg(feature = "user")]
unsafe impl aya::Pod for FinopsEvent {}
```

---

## Pattern 2 — Ring buffer map (finops-ebpf)

```rust
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(512 * 1024, 0);
```

---

## Pattern 3 — Tracepoint identity capture (kernel)

`reserve` → fill → `submit(0)`. Never `?` after `reserve`.

---

## Pattern 4 — Attach tracepoint (loader.rs)

`program.attach("sched", "sched_process_exec")`

---

## Pattern 5 — User event loop + batch flush (main.rs)

K8s pod list refresh runs in a **detached** `tokio::spawn` (`AttributionCache::clone`), not inside `select!`.

```rust
if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
    output::emit_batch(&batch);
}
```

---

## Pattern 6 — Memory sampling (userspace hot path)

Precompute `memory.current` on identity; `sample_tracked_cgroups` is `async` — snapshot paths, `spawn_blocking` + stack `[u8; 32]` read (not `read_to_string` on the runtime worker).

---

## Pattern 6b — Attribution cache

`AttributionCache`: `Clone` via `Arc<RwLock<...>>` maps; K8s refresh in background task.  
`cgroup_path_from_pid`: stack `[u8; 1024]` read of `/proc/{pid}/cgroup` (no `read_to_string` on exec path).  
`parking_lot::RwLock`, cgroup v2 `split_once("::")`, `Path::components()`.

---

## Pattern 6c — Aggregator

`FxHashMap`, double buffer, flip-before-drain, early flush at `max_keys`.

---

## Pattern 7 — Batched JSON (schema v2)

Agent → API envelope unchanged; see `output::BatchJson`.

---

## Pattern 8 — OOM-safe bounds (Phase 4+)

```rust
requests = (p99 × 1.20).max(MIN_REQUESTS);
limits   = requests × 1.25;
```

---

## Pattern 9 — GitOps PR body

```markdown
## Test plan
- [ ] `make build` && `make check`
- [ ] Phase 3: `make compose-up` → `curl http://127.0.0.1:3000/health` → agent ingest → ClickHouse count > 0
- [ ] ADR + skills + docs updated in same PR
```

---

## Pattern 10 — Phase 3 non-blocking ingest (enterprise)

**Agent:** `OnceLock<reqwest::Client>` with `.timeout(3s)` + `.pool_idle_timeout(90s)`; `tokio::spawn` POST ([ADR 006](../../../docs/adr/006-shared-http-client-for-ingest.md)).

**API:** `GET /health` (`503` if producer dead); `schema_version == 2` gate (`400`); `try_send` — `200`, `400`, or `503`; shutdown drain 10s cap.

**Kafka:** micro-batch (`recv_many` + linger); hoisted `payloads`; `drain().map().collect()` per batch for `produce` (library owns `Vec<Record>` — no recycle).

**ClickHouse:** Kafka engine settings — [ADR 008](../../../docs/adr/008-clickhouse-kafka-engine-resilience.md). MergeTree: LC only `node`/`namespace`; `ORDER BY (node, namespace, time, cgroup_id)`; 30d TTL — [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md).

---

## Pattern 11 — Docker / Makefile (Phase 3 dev)

```bash
make compose-up    # stop-api (host binary only) + stack + health check
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
curl -s -u default:finops_dev 'http://localhost:8123/?query=SELECT count() FROM finops_telemetry'
make compose-down
```

- **Do not** `make run-api` while compose `finops-api` is on `:3000`.
- **Do not** `fuser -k 3000` — breaks Docker port-forward ([ADR 009](../../../docs/adr/009-finops-api-docker-compose.md)).

Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md).

**API shutdown (container or host):** `with_graceful_shutdown` → drain mpsc → 10s cap ([ADR 005](../../../docs/adr/005-non-blocking-ingest-pipeline.md)).

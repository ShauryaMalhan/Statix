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

```rust
if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
    output::emit_batch(&batch);
}
```

---

## Pattern 6 — Memory sampling (userspace hot path)

Precompute `memory.current` on identity; stack `[u8; 32]` read; one `sample_tick_ns` per tick.

---

## Pattern 6b — Attribution cache

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
- [ ] Phase 3: `make compose-up`, `make run-api`, FINOPS_INGEST_URL ingest
- [ ] ADR + skills + docs updated in same PR
```

---

## Pattern 10 — Phase 3 non-blocking ingest (enterprise)

**Agent:** `OnceLock<reqwest::Client>` + `tokio::spawn` POST.

**API:** `FlatRow<'a>` borrows `&batch.node`; `try_send` only; always `200`.

**Kafka:** background `produce` task.

**ClickHouse:** daily `PARTITION BY`, `ORDER BY (namespace, pod, node, window_start_ns)`, 30d TTL — [ADR 007](../../../docs/adr/007-clickhouse-mergetree-tuning.md).

---

## Pattern 11 — Docker / Makefile (Phase 3 dev)

```bash
make compose-up && make run-api
sudo -E FINOPS_INGEST_URL=http://localhost:3000/ingest make run
```

Validate: [docs/phase3-validation.md](../../../docs/phase3-validation.md).

# L8/L9 Post-GA Audit V3 — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Each item has: **What** (the bug), **Why** (blast radius), **How** (prescriptive fix).
> Run `cargo check --workspace` after each fix. Create an ADR per wave.
> Priority: P0 = data loss / crash at scale, P1 = resource exhaustion / silent degradation, P2 = performance / correctness edge-case.

**Status:** All V3 waves shipped (ADR 049–053). Canonical checklist: [TODO.md](TODO.md).

---

## Shipped ✅ (ADR index)

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 ✅ | [049](../../../docs/adr/phase55/v3/049-phase55-v3-wave1-silent-deaths.md) | V3-7 K8s watcher panic monitor, V3-8 ring drops monitor panic, V3-13 ingest `try_reserve_many` |
| Wave 2 ✅ | [050](../../../docs/adr/phase55/v3/050-phase55-v3-wave2-cache-eviction.md) | V3-4 cgroup cache eviction, V3-5 pod delete eviction, V3-9 K8s reconnect backoff |
| Wave 3 ✅ | [051](../../../docs/adr/phase55/v3/051-phase55-v3-wave3-distributed-state.md) | V3-11 CH hour partitions, V3-12 kafka consumers, V3-15 recovery spread |
| Wave 4 ✅ | [052](../../../docs/adr/phase55/v3/052-phase55-v3-wave4-perf-observability.md) | V3-2 bootstrap blocking, V3-6 ring drops counter, V3-10 join error, V3-14 body limit, V3-1 resource limits |
| Wave 5 ✅ | [053](../../../docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md) | V3-16 BPF const, V3-17 alignment, V3-18 poll interval, V3-3 node `Arc<str>` |

---

## Deferred (not in wave scope)

### V3-5-extra: Persistent fd pool for `memory.current` reads

**File:** `statix/src/memory_sampler.rs`

**What:** 400 `open()/close()` syscalls per tick with 4000 cgroups.

**How (P2 — future optimization):** Cache open fds in `CacheState`; seek to 0 + read on each tick. Evict fd when cgroup is removed.

---

## Execution Order

```
Wave 1 ✅ (shipped):  V3-7, V3-8, V3-13          — ADR 049
Wave 2 ✅ (shipped):  V3-4, V3-5, V3-9            — ADR 050
Wave 3 ✅ (shipped):  V3-11, V3-12, V3-15         — ADR 051
Wave 4 ✅ (shipped):  V3-2, V3-6, V3-10, V3-14, V3-1  — ADR 052
Wave 5 ✅ (shipped):  V3-16, V3-17, V3-18, V3-3  — ADR 053
```

## ADR Index

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 ✅ | [049](../../../docs/adr/phase55/v3/049-phase55-v3-wave1-silent-deaths.md) | V3-7 spawn panic, V3-8 ring monitor panic, V3-13 TOCTOU batch |
| Wave 2 ✅ | [050](../../../docs/adr/phase55/v3/050-phase55-v3-wave2-cache-eviction.md) | V3-4 cache eviction, V3-5 pod eviction, V3-9 reconnect backoff |
| Wave 3 ✅ | [051](../../../docs/adr/phase55/v3/051-phase55-v3-wave3-distributed-state.md) | V3-11 CH partition, V3-12 kafka consumers, V3-15 thundering herd |
| Wave 4 ✅ | [052](../../../docs/adr/phase55/v3/052-phase55-v3-wave4-perf-observability.md) | V3-2 bootstrap blocking, V3-6 ring drops, V3-10 join error, V3-14 body limit, V3-1 QoS |
| Wave 5 ✅ | [053](../../../docs/adr/phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md) | V3-16 BPF const, V3-17 alignment, V3-18 poll interval, V3-3 node alloc |

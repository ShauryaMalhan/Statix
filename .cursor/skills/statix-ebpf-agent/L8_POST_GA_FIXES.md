# L8/L9 Post-GA Audit V3 — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Each item has: **What** (the bug), **Why** (blast radius), **How** (prescriptive fix).
> Run `cargo check --workspace` after each fix. Create an ADR per wave.
> Priority: P0 = data loss / crash at scale, P1 = resource exhaustion / silent degradation, P2 = performance / correctness edge-case.

**Status:** Waves 1–2 shipped. Remaining: Waves 3–5. Canonical checklist: [TODO.md](TODO.md).

---

## Shipped ✅ (ADR index)

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 ✅ | [049](../../../docs/adr/049-phase55-v3-wave1-silent-deaths.md) | V3-7 K8s watcher panic monitor, V3-8 ring drops monitor panic, V3-13 ingest `try_reserve_many` |
| Wave 2 ✅ | [050](../../../docs/adr/050-phase55-v3-wave2-cache-eviction.md) | V3-4 cgroup cache eviction, V3-5 pod delete eviction, V3-9 K8s reconnect backoff |

---

## Wave 3 — P1: Distributed State Physics (ACTIVE)

### V3-11: ClickHouse midnight partition boundary storm

**File:** `deploy/clickhouse/01_init.sql:31`

**What:** `PARTITION BY toYYYYMMDD(toDateTime(intDiv(window_start_ns, 1000000000)))` creates day-boundary partitions. Agents with clock drift (NTP accuracy: 1-100ms on EC2) produce windows that straddle midnight UTC, splitting into two partitions.

**Why:** Dual-partition writes at midnight double merge pressure for ~20 seconds. With 10,000 agents, the midnight boundary storm creates a spike of cross-partition parts that can exceed `max_parts_per_partition` (default 300) and trigger INSERT rejection.

**How:** Round partition expression to a coarser boundary. Use hour-aligned partitions or add a guard:

```sql
-- Option A: Partition by YYYYMMDDHH (hourly) to reduce boundary crossings
PARTITION BY toStartOfHour(toDateTime(intDiv(window_start_ns, 1000000000)))

-- Option B: Keep daily but truncate to prevent drift splits
PARTITION BY toYYYYMMDD(toDateTime(intDiv(window_start_ns, 1000000000) - 
    (intDiv(window_start_ns, 1000000000) % 60)))
```

Verify merge pressure with `SELECT partition, count() FROM system.parts WHERE table = 'workload_metrics' AND active GROUP BY partition`.

---

### V3-12: `kafka_num_consumers = 1` bottleneck at scale

**File:** `deploy/clickhouse/01_init.sql:59`

**What:** Single ClickHouse Kafka consumer for the entire telemetry stream.

**Why:** At 10,000 nodes × 100 workloads × 0.1 Hz = 100,000 rows/second. A single Kafka consumer thread in ClickHouse can typically handle ~50,000 rows/second of JSONEachRow. At 2x this rate, consumer lag grows unboundedly.

**How:** Set `kafka_num_consumers` to match topic partition count:

```sql
-- Set to topic partition count (at least 4 for production)
kafka_num_consumers = 4;
```

Add a TODO entry for monitoring: `SELECT * FROM system.kafka_consumers WHERE table = 'kafka_telemetry_queue'`.

---

### V3-15: Agent recovery thundering herd

**File:** `statix/src/output.rs:115-118`

**What:** After gateway outage recovery, all agents detect success within seconds. The 5-second jitter window means 10,000 agents recover in ~5 seconds = 2,000 agents/second.

**Why:** Each agent has up to 60 queued batches. Initial burst: up to 120,000 batches in 5 seconds against 2 gateway replicas = 12,000 batches/second/replica. This can OOM the gateway or trigger Kafka backpressure.

**How:** Scale jitter with node identity to naturally stagger recovery:

```rust
// Hash node name to produce a deterministic spread over 30 seconds
let node_hash = {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node_name.hash(&mut h);
    h.finish()
};
let spread_secs = (node_hash % 30_000) as f64 / 1000.0;
let jitter = rand::random::<f64>() * 5.0 + spread_secs;
```

Also tracked in **Phase 11** ([TODO.md](TODO.md)).

---

## Wave 4 — P1: Performance & Observability

### V3-2: `bootstrap_existing_cgroups` blocking async runtime

**File:** `statix/src/attribution/mod.rs:116-171`

**What:** `WalkDir` + `fs::metadata` are blocking syscalls on the Tokio worker thread.

**How:** Wrap the walk in `spawn_blocking`:

```rust
pub async fn bootstrap_existing_cgroups(
    cache: &AttributionCache,
    agg: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let root = cgroup_v2_mount();
    let cache_clone = cache.clone();
    
    let discovered = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
            // ... collect (cgroup_id, rel_path) pairs ...
        }
        entries
    }).await.unwrap_or_default();
    
    // Register and aggregate on the async thread (no I/O)
    // ...
}
```

---

### V3-6: `RING_DROPS` counter uses `absolute()` — fragile on reload

**File:** `statix/src/loader.rs:67`

**What:** `metrics::counter!("statix_ring_drops_total").absolute(total_drops)` assumes the BPF counter is monotonically increasing across agent restarts. If the BPF program is reloaded, the counter resets and Prometheus sees a counter decrease — which violates the counter monotonicity invariant.

**How:** Track the previous reading and emit increments:

```rust
let mut prev_total: u64 = 0;
// In the loop:
let delta = total_drops.saturating_sub(prev_total);
if delta > 0 {
    metrics::counter!("statix_ring_drops_total").increment(delta);
    prev_total = total_drops;
}
```

---

### V3-10: `spawn_blocking` JoinError silently returns empty Vec

**File:** `statix/src/memory_sampler.rs:36-37`

**What:** `.await.unwrap_or_default()` silently swallows panics in the blocking task.

**How:**

```rust
let readings = match tokio::task::spawn_blocking(move || { /* ... */ }).await {
    Ok(results) => results,
    Err(e) => {
        log::error!("Memory sampler blocking task failed: {e}");
        metrics::counter!("statix_memory_sampler_errors_total").increment(1);
        Vec::new()
    }
};
```

---

### V3-14: No explicit body size limit on POST /ingest

**File:** `statix-gateway/src/routes/ingest.rs`

**How:** Add an explicit Axum body limit layer:

```rust
use axum::extract::DefaultBodyLimit;

// In Router setup (main.rs):
.route("/ingest", post(routes::ingest::handler))
.layer(DefaultBodyLimit::max(2 * 1024 * 1024)) // 2MB explicit
```

---

### V3-1: Agent DaemonSet missing resource requests/limits

**File:** `deploy/k8s/statix-daemonset.yaml`

**How:** Add resource stanza to guarantee `Burstable` QoS class:

```yaml
resources:
  requests:
    cpu: 50m
    memory: 64Mi
  limits:
    cpu: 500m
    memory: 256Mi
```

---

## Wave 5 — P2: Micro-architecture Polish

### V3-16: Magic number for `BPF_RB_NO_WAKEUP`

**File:** `statix-ebpf/src/main.rs:77`

**How:** Define a named constant:

```rust
const BPF_RB_NO_WAKEUP: u64 = 1;
// ...
if count & 63 == 0 { 0 } else { BPF_RB_NO_WAKEUP }
```

---

### V3-17: No alignment assertion for `StatixEvent` pointer cast

**File:** `statix/src/main.rs:109-110`

**How:** Add a compile-time assertion:

```rust
const _: () = assert!(
    std::mem::align_of::<StatixEvent>() <= 8,
    "StatixEvent alignment exceeds BPF ring buffer guarantee"
);
```

---

### V3-18: 1ms poll interval is unnecessarily aggressive

**File:** `statix/src/main.rs:92`

**How:** Increase to 5ms. The wakeup suppression fires every 64th event; 5ms still catches the worst-case 64-event gap at 12,800 events/second:

```rust
let mut poll_interval = time::interval(Duration::from_millis(5));
```

---

### V3-3: `node.to_string()` on every flush

**File:** `statix/src/aggregator.rs:216`

**How:** Change `BatchPayload.node` to `&'a str` or `Arc<str>`:

```rust
pub struct BatchPayload {
    // ...
    pub node: Arc<str>,  // or pass &str with lifetime
    // ...
}

// In flush():
node: Arc::from(node),
```

---

### V3-5-extra: Persistent fd pool for `memory.current` reads

**File:** `statix/src/memory_sampler.rs`

**What:** 400 `open()/close()` syscalls per tick with 4000 cgroups.

**How (P2 — future optimization):** Cache open fds in `CacheState`; seek to 0 + read on each tick. Evict fd when cgroup is removed.

---

## Execution Order

```
Wave 1 ✅ (shipped):  V3-7, V3-8, V3-13          — ADR 049
Wave 2 ✅ (shipped):  V3-4, V3-5, V3-9            — ADR 050
Wave 3 (ACTIVE):      V3-11, V3-12, V3-15         — distributed state
Wave 4:               V3-2, V3-6, V3-10, V3-14, V3-1  — perf + observability
Wave 5:               V3-16, V3-17, V3-18, V3-3  — polish
```

## ADR Index

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 ✅ | [049](../../../docs/adr/049-phase55-v3-wave1-silent-deaths.md) | V3-7 spawn panic, V3-8 ring monitor panic, V3-13 TOCTOU batch |
| Wave 2 ✅ | [050](../../../docs/adr/050-phase55-v3-wave2-cache-eviction.md) | V3-4 cache eviction, V3-5 pod eviction, V3-9 reconnect backoff |
| Wave 3 | TBD | V3-11 CH partition, V3-12 kafka consumers, V3-15 thundering herd |
| Wave 4 | TBD | V3-2 bootstrap blocking, V3-6 ring drops counter, V3-10 join error, V3-14 body limit, V3-1 resource limits |
| Wave 5 | TBD | V3-16 BPF const, V3-17 alignment, V3-18 poll interval, V3-3 node alloc |

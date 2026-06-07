# L8/L9 Post-GA Audit V3 — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Each item has: **What** (the bug), **Why** (blast radius), **How** (prescriptive fix).
> Run `cargo check --workspace` after each fix. Create an ADR per wave.
> Priority: P0 = data loss / crash at scale, P1 = resource exhaustion / silent degradation, P2 = performance / correctness edge-case.

---

## Wave 1 — P0: Silent Deaths & Data Integrity

### V3-7: K8s watcher `tokio::spawn` silently swallows panics (CRITICAL)

**File:** `statix/src/main.rs:55-68`

**What:** The `JoinHandle` from `tokio::spawn(watch_k8s_pods)` is dropped. If the task panics (malformed Pod API response, serde deserialization edge-case), the panic is silently swallowed. The agent runs forever with `k8s_resolved: false` on every row.

**Why:** Total cost attribution failure on the affected node. No metric, no alert, no log. Discovered months later when the CFO asks why costs are "unattributed." On a 10,000-node cluster, even 1% of nodes hitting this = 100 nodes with silent data quality regression.

**How:** Store the `JoinHandle` and monitor it in the main `select!` loop. On panic, log at `error!`, emit a `statix_k8s_watcher_panics_total` counter, and restart the task.

```rust
// In main(), replace the fire-and-forget spawn:
let k8s_handle = tokio::spawn(async move {
    if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
        log::info!("Not in K8s — pod watch disabled");
        return;
    }
    match kube::Client::try_default().await {
        Ok(client) => {
            attribution::watch_k8s_pods(cache_for_k8s, client).await;
        }
        Err(e) => {
            log::warn!("K8s client init failed; pod resolution disabled: {e}");
        }
    }
});

// In the select! loop, add a branch:
_ = &mut k8s_handle => {
    match k8s_handle.await {
        Ok(()) => log::warn!("K8s watcher exited unexpectedly"),
        Err(e) => {
            log::error!("CRITICAL: K8s watcher task panicked: {e}");
            metrics::counter!("statix_k8s_watcher_panics_total").increment(1);
        }
    }
    // Optionally restart the task here
}
```

**Validation:** Kill the K8s API server mid-watch; verify error log + metric appears.

---

### V3-8: Ring drops monitor `tokio::spawn` also silently swallows panics

**File:** `statix/src/loader.rs:53-74`

**What:** Same pattern as V3-7. If the ring drops monitor panics, ring buffer overflows become completely invisible.

**Why:** BPF ring buffer overflows = silent telemetry data loss. The only visibility into this failure mode disappears.

**How:** Return the `JoinHandle` from `spawn_ring_drops_monitor`, store it in `main()`, and add a `select!` branch that logs + emits `statix_ring_monitor_panics_total` on `JoinError`.

```rust
// loader.rs: return JoinHandle
pub fn spawn_ring_drops_monitor(ring_drops: PerCpuArray<MapData, u64>) -> JoinHandle<()> {
    tokio::spawn(async move { /* existing loop */ })
}

// main.rs: store and monitor
let ring_monitor_handle = loader::spawn_ring_drops_monitor(ring_drops);
```

---

### V3-13: Ingest handler capacity pre-check is TOCTOU — partial batch delivery

**File:** `statix-gateway/src/routes/ingest.rs:87-149`

**What:** The capacity pre-check (`state.kafka_tx.capacity() >= required_slots`) is not atomic with the subsequent `try_send` loop. Under concurrent load, another handler can consume capacity between check and send, causing a mid-batch `Full` error. Some rows are sent, some rejected — the batch is split.

**Why:** Split batches corrupt ClickHouse aggregation. The `ReplacingMergeTree` deduplicates by `(node, window_start_ns, cgroup_id)`, but partial batches create a state where some cgroups appear in a window and others don't. Billing queries report inconsistent data.

**How:** Use `mpsc::Sender::reserve_many` (Tokio 1.33+) or serialize row production so only one handler sends at a time via a `Mutex<()>` write guard on the send loop. The simplest correct fix: pre-serialize all rows, then send them in a single `try_send` as one `Vec<u8>` blob (change `KafkaQueueItem` to carry the full batch).

```rust
// Option A: Send entire batch as single channel item
// Change KafkaQueueItem to carry Vec<(Arc<[u8]>, Vec<u8>)>
// Single try_send = atomic accept/reject

// Option B: Reserve permits
let permit = state.kafka_tx.reserve_many(required_slots).await
    .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "Channel closed"))?;
// Send all rows through permits (guaranteed capacity)
```

**Validation:** Load test with 100 concurrent ingest handlers; verify zero partial batch deliveries.

---

## Wave 2 — P0: Resource Exhaustion Time Bombs

### V3-4: `AttributionCache` unbounded growth — no eviction of dead cgroups

**File:** `statix/src/attribution/mod.rs:34-38`

**What:** `cgroup_paths`, `memory_current_paths`, and `cgroup_labels` maps grow monotonically. When a pod terminates, its cgroup disappears from the filesystem, but the cache entries persist forever.

**Why:** On a node cycling 500 pods/day, after 6 months: ~90,000 stale entries. At ~250 bytes per entry set across 3 maps, that's ~22MB leaked per agent. Additionally, `memory_sampler` attempts `File::open` on ~90,000 stale `memory.current` paths every 10 seconds, generating ENOENT errors (suppressed to `debug` log). At 10,000 nodes = 220GB aggregate leaked memory.

**How:** Add an eviction sweep on a timer. Every 60 seconds, iterate `memory_current_paths` and remove entries where the path no longer exists. Cascade delete to `cgroup_paths` and `cgroup_labels`.

```rust
impl AttributionCache {
    pub fn evict_stale_cgroups(&self) -> usize {
        let stale_ids: Vec<u64> = {
            let state = self.state.read();
            state.memory_current_paths.iter()
                .filter(|(_, path)| !path.exists())
                .map(|(id, _)| *id)
                .collect()
        };
        if stale_ids.is_empty() { return 0; }
        let mut state = self.state.write();
        for id in &stale_ids {
            state.cgroup_paths.remove(id);
            state.memory_current_paths.remove(id);
            state.cgroup_labels.remove(id);
        }
        stale_ids.len()
    }
}
```

Add a 60-second eviction timer in `main.rs` `select!` loop. Emit `statix_cache_evictions_total` gauge.

**Validation:** Start 100 pods, stop them, wait 2 minutes; verify cache size returns to baseline.

---

### V3-5: `pod_by_uid` never evicts deleted pods

**File:** `statix/src/attribution/mod.rs:38`

**What:** The `pod_by_uid` map tracks K8s pod UIDs but never removes entries for deleted pods.

**Why:** Same memory leak pattern as V3-4 but in the K8s label dimension. Every pod that ever existed on this node stays in memory. At 500 pods/day for 6 months = ~90,000 entries × ~120 bytes = ~10MB per agent.

**How:** In `watch_k8s_pods`, handle `Event::Delete` by removing the pod UID from the map:

```rust
Event::Delete(pod) => {
    if let Some(uid) = pod.metadata.uid.as_ref() {
        let mut state = cache.state.write();
        state.pod_by_uid.remove(uid);
    }
}
```

---

### V3-9: K8s watcher reconnect loop has no backoff — 5s tight DDoS

**File:** `statix/src/attribution/mod.rs:418-424`

**What:** When the K8s API is unreachable, the watcher reconnects every 5 seconds with no exponential backoff.

**Why:** At 10,000 nodes, that's 2,000 requests/second against the K8s API server during an outage. This can trigger API server rate limiting and affect all other controllers on the cluster.

**How:** Add jittered exponential backoff to the reconnect loop:

```rust
let mut reconnect_backoff = Duration::from_secs(5);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(300);

loop {
    // ... run watcher ...
    
    log::warn!("K8s pod watcher stream ended; reconnecting in {reconnect_backoff:?}");
    if let Err(e) = refresh_k8s_pods(&cache, &client).await {
        log::warn!("K8s list fallback failed: {e}");
    }
    let jitter = rand::random::<f64>() * reconnect_backoff.as_secs_f64() * 0.3;
    tokio::time::sleep(reconnect_backoff + Duration::from_secs_f64(jitter)).await;
    reconnect_backoff = (reconnect_backoff * 2).min(MAX_RECONNECT_BACKOFF);
    // Reset backoff on successful reconnect (inside the while loop)
}
```

---

## Wave 3 — P1: Distributed State Physics

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
Wave 1 (WEEK 1):    V3-7, V3-8, V3-13          — silent death + data integrity
Wave 2 (WEEK 2):    V3-4, V3-5, V3-9            — memory leaks + API DDoS
Wave 3 (WEEK 3):    V3-11, V3-12, V3-15         — distributed state
Wave 4 (WEEK 4):    V3-2, V3-6, V3-10, V3-14, V3-1  — perf + observability
Wave 5 (MONTH 2):   V3-16, V3-17, V3-18, V3-3  — polish
```

## ADR Index

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 | TBD | V3-7 spawn panic, V3-8 ring monitor panic, V3-13 TOCTOU batch |
| Wave 2 | TBD | V3-4 cache eviction, V3-5 pod eviction, V3-9 reconnect backoff |
| Wave 3 | TBD | V3-11 CH partition, V3-12 kafka consumers, V3-15 thundering herd |
| Wave 4 | TBD | V3-2 bootstrap blocking, V3-6 ring drops counter, V3-10 join error, V3-14 body limit, V3-1 resource limits |
| Wave 5 | TBD | V3-16 BPF const, V3-17 alignment, V3-18 poll interval, V3-3 node alloc |

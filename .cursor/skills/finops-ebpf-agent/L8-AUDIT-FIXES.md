# L8 Audit — Fix Playbook

**Purpose:** Precise, copy-pasteable fixes for every flaw identified in the L8 architectural audit. Each fix includes the exact before/after code, the file and function to modify, the rationale for _this specific_ approach, and pitfalls that will cause regressions if ignored.

**Rule:** When implementing any fix from this file, follow the exact approach described here. Do not invent alternative solutions. These fixes are interdependent — read the dependency notes before reordering.

**Validation after every fix:** `cargo check -p finops-agent -p finops-api -p finops-wire && cargo test -p finops-api`

---

## Fix Index (execution order matters)

| # | File | Flaw | Effort | Depends On |
|---|------|------|--------|------------|
| F1 | `output.rs` | `std::env::var` per flush | 1 min | — |
| F2 | `aggregator.rs` | `getrandom` syscall per flush | 5 min | — |
| F3 | `aggregator.rs` | Heap-alloc `agent_version` per flush | 1 min | — |
| F4 | `aggregator.rs` | `Arc::new` default labels per cgroup | 2 min | — |
| F5 | `output.rs` + `main.rs` | Clone entire payload in `emit_batch` | 5 min | — |
| F6 | `output.rs` | Body clone on every HTTP retry | 3 min | — |
| F7 | `memory_sampler.rs` | `spawn_blocking` per cgroup | 5 min | — |
| F8 | `main.rs` | Ring buffer drain without budget | 3 min | — |
| F9 | `kafka.rs` | HashMap alloc per batch in producer | 5 min | — |
| F10 | `kafka.rs` | `Utc::now()` syscall per Kafka record | 2 min | F9 |
| F11 | `attribution.rs` | `kube::Client` recreated every 30s | 5 min | — |
| F12 | `kafka.rs` | No partition metadata refresh | 10 min | F9 |
| F13 | `routes/ingest.rs` | `node_vec.clone()` per workload row | 10 min | — |
| F14 | `routes/query.rs` | `FINAL` on operational reads | 3 min | — |

---

## F1 — Cache `FINOPS_INGEST_URL` with `OnceLock`

**File:** `finops-agent/src/output.rs`

**Why:** `std::env::var()` acquires a process-wide `RwLock` on the environment block. Called on every flush (every 5–10s baseline, sub-second under early-flush). The env never changes after startup.

**Before (line ~242):**

```rust
pub fn emit_batch(payload: &BatchPayload) {
    // ... json serialization ...
    if std::env::var("FINOPS_INGEST_URL").is_ok() {
        enqueue_batch_json(json);
    } else {
        println!("{json}");
    }
}
```

**After:**

```rust
use std::sync::OnceLock;

static IS_HTTP_INGEST: OnceLock<bool> = OnceLock::new();

fn is_http_ingest() -> bool {
    *IS_HTTP_INGEST.get_or_init(|| std::env::var("FINOPS_INGEST_URL").is_ok())
}
```

Then replace the check in `emit_batch`:

```rust
    if is_http_ingest() {
        enqueue_batch_json(json);
    } else {
        println!("{json}");
    }
```

**Pitfalls:**
- Do NOT use `LazyLock` here. `OnceLock` is correct because it initializes on first access (which happens after env is fully set up in `main`).
- Do NOT cache the URL string itself here — `init_retry_worker` already receives it. This flag is just a bool.

---

## F2 — Thread-local RNG for `batch_id` UUID

**File:** `finops-agent/src/aggregator.rs`

**Why:** `uuid::Uuid::new_v4()` calls `getrandom(2)` — a syscall that can block under entropy starvation (fresh container, cold VM). This runs on the same Tokio task that drains the eBPF ring buffer.

**Before (line ~195 in `flush`):**

```rust
        let batch_id = uuid::Uuid::new_v4().to_string();
```

**After — add this at the top of `aggregator.rs`:**

```rust
use std::cell::RefCell;
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};

thread_local! {
    static TL_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_os_rng());
}

fn fast_batch_id() -> String {
    let mut bytes = [0u8; 16];
    TL_RNG.with(|rng| rng.borrow_mut().fill_bytes(&mut bytes));
    uuid::Builder::from_random_bytes(bytes).into_uuid().to_string()
}
```

Then in `flush`:

```rust
        let batch_id = fast_batch_id();
```

**Why `SmallRng` and not `ThreadRng`:**
- `ThreadRng` (from `rand::thread_rng()`) internally uses `OsRng` for reseeding, which calls `getrandom` periodically. We want zero syscalls after init.
- `SmallRng` seeds once from OS entropy via `from_os_rng()`, then uses xoshiro256++ which is pure register math.
- This is NOT cryptographic. UUID v4 for `batch_id` does not need crypto strength — it's a correlation key, not a secret.

**Why `thread_local!` and not a struct field:**
- The `Aggregator` is `&mut self` (single owner), so a struct field would also work. But `thread_local!` is the standard pattern for RNG in Rust and keeps the Aggregator focused on aggregation logic. Either approach is acceptable.

**Pitfalls:**
- Do NOT use `uuid::Uuid::new_v4()` anywhere else in the hot path without this pattern.
- `rand` is already in `Cargo.toml` (`rand = "0.8"`). Confirm `SmallRng` is available — it is in `rand::rngs::SmallRng` since 0.8.
- `from_os_rng()` was added in rand 0.9. If using rand 0.8, use `SmallRng::from_entropy()` instead which also calls `getrandom` once. If on rand 0.8, the code is:

```rust
thread_local! {
    static TL_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_entropy());
}
```

Check the `rand` version in `finops-agent/Cargo.toml`. If `rand = "0.8"`, use `from_entropy()`. If upgraded to `0.9+`, use `from_os_rng()`.

---

## F3 — Static `agent_version` string

**File:** `finops-agent/src/aggregator.rs`

**Why:** `env!("CARGO_PKG_VERSION").to_string()` heap-allocates a new `String` from a compile-time `&'static str` on every flush. Pure waste.

**Before (line ~196 in `flush`):**

```rust
        let agent_version = env!("CARGO_PKG_VERSION").to_string();
```

**After — add at module level:**

```rust
static AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
```

Then in `flush`, change the `BatchPayload` construction to use a `&'static str`. This requires changing `BatchPayload::agent_version` from `String` to something that doesn't allocate.

**Option A (minimal change):** Keep `BatchPayload::agent_version` as `String` but allocate once:

```rust
use std::sync::OnceLock;

static AGENT_VERSION_STRING: OnceLock<String> = OnceLock::new();

fn agent_version() -> String {
    AGENT_VERSION_STRING.get_or_init(|| env!("CARGO_PKG_VERSION").to_string()).clone()
}
```

This still clones, but from a cached `String` — the underlying bytes share the allocator.

**Option B (zero-alloc — preferred):** Change `BatchPayload::agent_version` to `&'static str`:

```rust
pub struct BatchPayload {
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub batch_id: String,
    pub agent_version: &'static str,
    pub workloads: Vec<WorkloadRow>,
}
```

Then in `flush`:

```rust
        let agent_version = env!("CARGO_PKG_VERSION");
```

And in `output.rs::emit_batch`, when building `IngestBatch`:

```rust
        agent_version: payload.agent_version.to_string(),
```

The single `.to_string()` happens once at emit time (not per-event). This is acceptable since `emit_batch` is already serializing to JSON.

**Pitfalls:**
- If you choose Option B, `IngestBatch::agent_version` in `finops-wire` is `String` (for serde). That's fine — the single allocation moves to the serialization boundary.
- Do NOT change `finops-wire`'s types to `&str` — wire types must be owned for deserialization on the gateway side.

---

## F4 — Use `DEFAULT_LABELS` in `WorkloadStats::default()`

**File:** `finops-agent/src/aggregator.rs`

**Why:** Every new cgroup entry via `.entry(cgroup_id).or_default()` allocates a new `Arc<WorkloadLabels>` on the heap. A static default already exists in `attribution.rs` but isn't used.

**Step 1:** Make `DEFAULT_LABELS` public in `attribution.rs`:

```rust
// attribution.rs — change from:
static DEFAULT_LABELS: LazyLock<Arc<WorkloadLabels>> =
    LazyLock::new(|| Arc::new(WorkloadLabels::default()));

// to:
pub static DEFAULT_LABELS: LazyLock<Arc<WorkloadLabels>> =
    LazyLock::new(|| Arc::new(WorkloadLabels::default()));
```

**Step 2:** Use it in `aggregator.rs`:

```rust
// aggregator.rs — change WorkloadStats::default:
impl Default for WorkloadStats {
    fn default() -> Self {
        Self {
            exec_count: 0,
            sample_count: 0,
            memory_bytes_max: 0,
            memory_bytes_last: 0,
            labels: Arc::clone(&crate::attribution::DEFAULT_LABELS),
        }
    }
}
```

**Pitfalls:**
- `LazyLock` requires `std::sync::LazyLock` (stable since Rust 1.80). Verify the agent's MSRV.
- The `Arc::clone` is a single atomic increment — no heap allocation.

---

## F5 — Consume `BatchPayload` in `emit_batch` (move, not clone)

**File:** `finops-agent/src/output.rs` + `finops-agent/src/main.rs`

**Why:** `emit_batch` takes `&BatchPayload` and clones every field (node, batch_id, agent_version, entire workloads vec). All callers own the payload and never use it after the call.

**Step 1:** Change `emit_batch` signature in `output.rs`:

```rust
// Before:
pub fn emit_batch(payload: &BatchPayload) {
    let batch = IngestBatch {
        schema_version: SCHEMA_VERSION,
        window_start_ns: payload.window_start_ns,
        window_end_ns: payload.window_end_ns,
        node: payload.node.clone(),
        batch_id: payload.batch_id.clone(),
        agent_version: payload.agent_version.clone(),
        workloads: payload.workloads.clone(),
    };

// After:
pub fn emit_batch(payload: BatchPayload) {
    let batch = IngestBatch {
        schema_version: SCHEMA_VERSION,
        window_start_ns: payload.window_start_ns,
        window_end_ns: payload.window_end_ns,
        node: payload.node,
        batch_id: payload.batch_id,
        agent_version: payload.agent_version.to_string(),
        workloads: payload.workloads,
    };
```

Note: If you applied F3 Option B (`agent_version: &'static str`), use `.to_string()` here. If `agent_version` is still `String`, just move it: `agent_version: payload.agent_version,`.

**Step 2:** Update all call sites in `main.rs` — remove `&`:

```rust
// main.rs — all these patterns change from &batch to batch:

// Line ~81 (bootstrap):
for batch in attribution::bootstrap_existing_cgroups(&cache, &mut agg, &node).await {
    output::emit_batch(batch);   // was &batch
}

// Line ~100 (ring buffer event):
if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
    output::emit_batch(batch);   // was &batch
}

// Line ~108 (flush interval):
if let Some(batch) = agg.flush(&node, &cache) {
    output::emit_batch(batch);   // was &batch
}

// Lines ~113-117 (memory sampler):
for batch in
    memory_sampler::sample_tracked_cgroups(&cache, &mut agg, &node).await
{
    output::emit_batch(batch);   // was &batch
}

// Line ~122 (shutdown):
if let Some(batch) = agg.flush(&node, &cache) {
    output::emit_batch(batch);   // was &batch
}
```

**Pitfalls:**
- Verify there are no other callers of `emit_batch` outside `main.rs`. grep for `emit_batch` in the agent crate.
- The `emit_raw` function is separate and unaffected.
- This is safe because `BatchPayload` is never read after `emit_batch` at any call site.

---

## F6 — Fix `post_ingest` body clone

**File:** `finops-agent/src/output.rs`

**Why:** `.body(body.to_string())` allocates a new heap `String` for every HTTP attempt, including retries. A single batch could be 10–50 KB.

**Before (in `post_ingest`, line ~168):**

```rust
async fn post_ingest(url: &str, body: &str) -> PostOutcome {
    let client = HTTP_CLIENT
        .get()
        .cloned()
        .unwrap_or_else(reqwest::Client::new);

    let response = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
    {
```

**After:**

```rust
async fn post_ingest(url: &str, body: &str) -> PostOutcome {
    let client = match HTTP_CLIENT.get() {
        Some(c) => c,
        None => {
            return PostOutcome::Retryable("HTTP client not initialized".into());
        }
    };

    let response = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_owned())
        .send()
        .await
    {
```

Wait — `body.to_owned()` is the same as `body.to_string()`. The real fix is at the channel level. Change the retry channel to pass `bytes::Bytes` or use `Arc<str>`:

**Better approach — change channel type to `Arc<str>`:**

```rust
// Change these statics:
static RETRY_TX: OnceLock<mpsc::Sender<Arc<str>>> = OnceLock::new();
static RETRY_RX: OnceLock<Arc<Mutex<mpsc::Receiver<Arc<str>>>>> = OnceLock::new();
```

In `enqueue_batch_json`:

```rust
fn enqueue_batch_json(json: String) {
    let json: Arc<str> = json.into();  // one alloc, refcounted
    // ... rest uses json (Arc<str> is Clone without heap alloc)
```

In `init_retry_worker`, the retry loop body becomes:

```rust
            loop {
                match post_ingest(&url, &body).await {
```

And `post_ingest` becomes:

```rust
async fn post_ingest(url: &str, body: &str) -> PostOutcome {
    let client = match HTTP_CLIENT.get() {
        Some(c) => c,
        None => return PostOutcome::Retryable("HTTP client not initialized".into()),
    };

    let response = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(bytes::Bytes::copy_from_slice(body.as_bytes()))
        .send()
        .await
    {
```

**Why `Bytes::copy_from_slice`:** `reqwest::Body::from(Bytes)` is zero-copy from the Bytes buffer. The `copy_from_slice` copies once from the `Arc<str>` into a refcounted `Bytes`. On retries, the same `&str` reference is used — no new allocation per attempt.

**Simpler alternative if `bytes` isn't already a dependency:** Just leave the channel as `String` and fix only `post_ingest` to avoid `.cloned()` on the client:

```rust
async fn post_ingest(url: &str, body: &str) -> PostOutcome {
    let Some(client) = HTTP_CLIENT.get() else {
        return PostOutcome::Retryable("HTTP client not initialized".into());
    };
    // body is &str — reqwest accepts &str as body (no extra alloc needed)
    let response = match client
        .post(url)
        .body(body.to_string())
        .header("Content-Type", "application/json")
        .send()
        .await
    {
```

The `.cloned()` on `HTTP_CLIENT.get()` was cloning the entire `reqwest::Client` (which is internally `Arc`-wrapped, so the clone is cheap). But removing the `unwrap_or_else(reqwest::Client::new)` fallback eliminates a potential silent misconfiguration.

**Pitfalls:**
- `reqwest` does NOT accept `&str` directly as a body without allocation. It needs an owned type. The best approach is `Bytes` if the dep exists, otherwise accept the single `.to_string()`.
- `bytes` is a transitive dependency of `reqwest` and `tokio`, so it's already available. Add `bytes = "1"` to `finops-agent/Cargo.toml` if you want to use it explicitly, or use `reqwest::Body::from(body.as_bytes().to_vec())` which is equivalent.
- The real win here is the `Arc<str>` channel type — the string is allocated once at `enqueue_batch_json` time and shared across all retry attempts without cloning.

---

## F7 — Batch `spawn_blocking` in memory sampler

**File:** `finops-agent/src/memory_sampler.rs`

**Why:** Currently spawns one blocking task per tracked cgroup (N tasks for N cgroups). At 500 cgroups, this saturates Tokio's blocking thread pool (default 512 threads) and creates 500 task scheduling round-trips because each is awaited sequentially.

**Before:**

```rust
pub async fn sample_tracked_cgroups(
    cache: &AttributionCache,
    aggregator: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let sample_tick_ns = now_ns();
    let mut early_batches = Vec::new();

    let mut targets: Vec<(u64, Arc<PathBuf>)> = Vec::new();
    cache.for_each_memory_current_path(|cgroup_id, path| {
        targets.push((cgroup_id, path));
    });

    for (cgroup_id, path) in targets {
        let memory_bytes = match read_memory_current_at_async(Arc::clone(&path)).await {
            Ok(v) => v,
            Err(e) => {
                log::debug!("memory.current read failed for {path:?}: {e}");
                continue;
            }
        };

        if let Some(batch) = aggregator.ingest_memory_sample(
            EVENT_KIND_MEMORY_SAMPLE,
            cgroup_id,
            memory_bytes,
            sample_tick_ns,
            0,
            cache,
            node,
        ) {
            early_batches.push(batch);
        }
    }

    early_batches
}
```

**After:**

```rust
pub async fn sample_tracked_cgroups(
    cache: &AttributionCache,
    aggregator: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let sample_tick_ns = now_ns();

    let mut targets: Vec<(u64, Arc<PathBuf>)> = Vec::new();
    cache.for_each_memory_current_path(|cgroup_id, path| {
        targets.push((cgroup_id, path));
    });

    let readings = tokio::task::spawn_blocking(move || {
        let mut results = Vec::with_capacity(targets.len());
        for (cgroup_id, path) in targets {
            match read_memory_current_at(path.as_path()) {
                Ok(v) => results.push((cgroup_id, v)),
                Err(e) => log::debug!("memory.current read failed for {path:?}: {e}"),
            }
        }
        results
    })
    .await
    .unwrap_or_default();

    let mut early_batches = Vec::new();
    for (cgroup_id, memory_bytes) in readings {
        if let Some(batch) = aggregator.ingest_memory_sample(
            EVENT_KIND_MEMORY_SAMPLE,
            cgroup_id,
            memory_bytes,
            sample_tick_ns,
            0,
            cache,
            node,
        ) {
            early_batches.push(batch);
        }
    }

    early_batches
}
```

**Then remove or mark `read_memory_current_at_async` as dead code:**

```rust
// DELETE this function — no longer needed:
// async fn read_memory_current_at_async(path: Arc<PathBuf>) -> anyhow::Result<u64> { ... }
```

**Why a single `spawn_blocking` is correct:**
- cgroupfs (`/sys/fs/cgroup/.../memory.current`) is a kernel pseudo-filesystem backed by in-memory data structures. Reads complete in microseconds — there is no disk I/O.
- The reason to use `spawn_blocking` at all is to avoid blocking the Tokio runtime thread (which also drains the ring buffer). One blocking thread doing 500 sequential microsecond reads takes ~1–5ms total.
- 500 separate `spawn_blocking` calls create 500 task wake/schedule cycles through Tokio's executor. The scheduling overhead dominates the actual read time.

**Pitfalls:**
- Do NOT use `tokio::fs::read_to_string` — it spawns blocking internally per call (same problem).
- Do NOT try to parallelize reads with `join_all` on `spawn_blocking` — the reads are already microsecond-fast and the parallelism overhead would dominate.
- The `move` closure captures `targets` by value (Vec of Arc clones). This is fine — Arc clone is a single atomic increment.

---

## F8 — Ring buffer drain budget

**File:** `finops-agent/src/main.rs`

**Why:** The `while let Some(item) = rb.next()` loop drains the ENTIRE ring buffer before yielding back to `tokio::select!`. An exec storm generating 10,000 events blocks flush intervals and memory samples for the duration of processing.

**Before (in the main `loop { tokio::select! { ... } }`, line ~86-103):**

```rust
            guard_result = async_fd.readable_mut() => {
                let mut guard = guard_result?;
                let rb = guard.get_inner_mut();
                while let Some(item) = rb.next() {
                    if item.len() < size_of::<FinopsEvent>() {
                        log::warn!("Undersized event ({} bytes), skipping", item.len());
                        continue;
                    }
                    let event: &FinopsEvent =
                        unsafe { &*(item.as_ptr() as *const FinopsEvent) };
                    if raw_events {
                        output::emit_raw(event);
                    }
                    if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
                        output::emit_batch(&batch);
                    }
                }
                guard.clear_ready();
            }
```

**After:**

```rust
            guard_result = async_fd.readable_mut() => {
                let mut guard = guard_result?;
                let rb = guard.get_inner_mut();
                let mut drained = 0usize;
                const DRAIN_BUDGET: usize = 256;
                while drained < DRAIN_BUDGET {
                    let Some(item) = rb.next() else { break };
                    if item.len() < size_of::<FinopsEvent>() {
                        log::warn!("Undersized event ({} bytes), skipping", item.len());
                        continue;
                    }
                    let event: &FinopsEvent =
                        unsafe { &*(item.as_ptr() as *const FinopsEvent) };
                    if raw_events {
                        output::emit_raw(event);
                    }
                    if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
                        output::emit_batch(batch);
                    }
                    drained += 1;
                }
                guard.clear_ready();
            }
```

**Why 256:**
- At 64 bytes per event, 256 events = 16 KB of ring buffer data. Processing 256 events takes ~50–100µs (pointer cast + hashmap lookup + optional label fetch per event).
- This gives the flush and memory sample intervals a chance to fire every ~100µs under load, rather than being starved for seconds.
- If the ring buffer has more than 256 events, the `AsyncFd` will immediately become readable again and the next `select!` iteration processes the next 256.

**Pitfalls:**
- Do NOT set the budget too low (e.g., 16). This adds unnecessary `select!` loop overhead per event batch.
- Do NOT set it too high (e.g., 65536). That defeats the purpose.
- The `continue` for undersized events should NOT count against the budget (it's not a real event).
- `guard.clear_ready()` must happen AFTER the drain loop, not inside it.
- If you applied F5 (consume BatchPayload), the `emit_batch` call uses `batch` not `&batch`.

---

## F9 — Reuse `by_partition` HashMap in Kafka producer

**File:** `finops-api/src/kafka.rs`

**Why:** `produce_grouped_batch` creates a new `HashMap<i32, Vec<KafkaQueueItem>>` on every micro-batch. At 20 batches/second, that's 20 HashMap allocations + N Vec allocations + 20 HashMap drops per second.

**Step 1:** Add `by_partition` as a parameter to `produce_grouped_batch`:

```rust
// Before:
async fn produce_grouped_batch(
    partition_ids: &[i32],
    clients: &HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>,
    batch: &mut Vec<KafkaQueueItem>,
    batch_max: usize,
) {
    if batch.is_empty() {
        return;
    }
    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> = HashMap::new();

// After:
async fn produce_grouped_batch(
    partition_ids: &[i32],
    clients: &HashMap<i32, Arc<rskafka::client::partition::PartitionClient>>,
    batch: &mut Vec<KafkaQueueItem>,
    batch_max: usize,
    by_partition: &mut HashMap<i32, Vec<KafkaQueueItem>>,
) {
    if batch.is_empty() {
        return;
    }
    by_partition.clear();
```

**Step 2:** Allocate `by_partition` in `run_producer_loop`, outside the loop:

```rust
// In run_producer_loop, after `let mut batch = Vec::with_capacity(batch_max);`:
    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> =
        HashMap::with_capacity(partition_ids.len());
```

**Step 3:** Pass it to every `produce_grouped_batch` call (there are 3 call sites in `run_producer_loop`):

```rust
    // Main loop:
    produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max, &mut by_partition).await;

    // Drain loop inside None arm:
    produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max, &mut by_partition).await;

    // Final drain:
    produce_grouped_batch(&partition_ids, &clients, &mut batch, batch_max, &mut by_partition).await;
```

**Why `.clear()` preserves capacity:** `HashMap::clear()` removes all entries but keeps the allocated bucket array. The inner `Vec<KafkaQueueItem>` values are dropped, but the HashMap itself doesn't reallocate. On the next batch, `.entry(pid).or_default()` reuses the existing bucket slots.

**Bonus — also preserve the inner Vecs:** For even less allocation churn, drain inner vecs instead of dropping them:

```rust
    by_partition.values_mut().for_each(|v| v.clear());
    for (node, payload) in batch.drain(..) {
        let pid = partition_id_for_node(node.as_slice(), partition_ids);
        by_partition.entry(pid).or_default().push((node, payload));
    }
```

But this only helps if the number of partitions is stable (which it almost always is). The simple `.clear()` on the HashMap is sufficient.

**Pitfalls:**
- All three `produce_grouped_batch` call sites in `run_producer_loop` must be updated. Miss one and the compiler will tell you (arity mismatch).
- Do NOT make `by_partition` a field on `KafkaProducer` — it's only used inside `run_producer_loop` and shouldn't leak to the public API.

---

## F10 — Batch `Utc::now()` per produce cycle

**File:** `finops-api/src/kafka.rs`

**Why:** `bytes_to_record` calls `Utc::now()` per Kafka record. At 1024 records per batch, that's 1024 `clock_gettime` syscalls (the vDSO may optimize this, but it's still unnecessary work).

**Before (in `bytes_to_record`):**

```rust
fn bytes_to_record(node: Vec<u8>, payload: Vec<u8>) -> Record {
    Record {
        key: Some(node),
        value: Some(payload),
        headers: std::collections::BTreeMap::new(),
        timestamp: Utc::now(),
    }
}
```

**After:**

```rust
fn bytes_to_record(node: Vec<u8>, payload: Vec<u8>, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        key: Some(node),
        value: Some(payload),
        headers: std::collections::BTreeMap::new(),
        timestamp: ts,
    }
}
```

**Then in `produce_grouped_batch`, compute the timestamp once per partition chunk:**

```rust
    for (pid, mut rows) in by_partition.drain() {
        // ... existing client lookup ...
        while !rows.is_empty() {
            let chunk_len = rows.len().min(batch_max);
            let chunk: Vec<_> = rows.drain(..chunk_len).collect();
            let n = chunk.len();
            let batch_ts = Utc::now();  // one syscall per chunk, not per record
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(node, payload)| bytes_to_record(node, payload, batch_ts))
                .collect();
```

**Pitfalls:**
- `chrono::DateTime<Utc>` implements `Copy`, so passing it by value to each `bytes_to_record` call is free.
- The `BTreeMap::new()` is lazy (no heap alloc until first insert). It's fine to leave as-is. rskafka requires the field to exist.

---

## F11 — Cache `kube::Client` across K8s refresh polls

**File:** `finops-agent/src/attribution.rs`

**Why:** `refresh_k8s_pods` creates a new `kube::Client` every 30 seconds. Each creation opens a TCP connection, performs TLS handshake, and reads the service account token from disk. At 200 agents, that's 400 API server connections/minute.

**Before (line ~300 in `refresh_k8s_pods`):**

```rust
pub async fn refresh_k8s_pods(cache: &AttributionCache) -> anyhow::Result<()> {
    if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
        return Ok(());
    }

    let client = kube::Client::try_default().await?;
```

**After — use a `OnceLock` for the client:**

Add at the top of `attribution.rs`:

```rust
use std::sync::OnceLock;
use tokio::sync::OnceCell;

static K8S_CLIENT: OnceCell<kube::Client> = OnceCell::const_new();

async fn get_k8s_client() -> anyhow::Result<&'static kube::Client> {
    K8S_CLIENT
        .get_or_try_init(|| async { kube::Client::try_default().await.map_err(Into::into) })
        .await
}
```

Then in `refresh_k8s_pods`:

```rust
pub async fn refresh_k8s_pods(cache: &AttributionCache) -> anyhow::Result<()> {
    if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
        return Ok(());
    }

    let client = get_k8s_client().await?;
```

And change the `pods` API client construction to borrow:

```rust
    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> = kube::Api::all(client.clone());
```

**Why `tokio::sync::OnceCell` and not `std::sync::OnceLock`:**
- `kube::Client::try_default()` is `async`. `OnceLock` requires a sync init function. `tokio::sync::OnceCell` supports async initialization.
- `kube::Client` is internally `Arc`-wrapped, so `client.clone()` is cheap (atomic increment).

**Pitfalls:**
- `tokio::sync::OnceCell` is available in `tokio` with the `sync` feature (already enabled in `finops-agent/Cargo.toml`).
- If the K8s API server is unreachable at first init, `get_or_try_init` will retry on the next call (it only caches on success).
- If the service account token rotates (rare, but possible in long-running pods), the cached client will use the stale token. This is acceptable for a 30-second poll interval — the token rotation window is typically hours.

---

## F12 — Kafka partition metadata periodic refresh

**File:** `finops-api/src/kafka.rs`

**Why:** Partition clients are loaded once at startup. If partitions are added/reassigned, the producer silently routes to stale partition IDs.

**Before (in `run_producer_loop`, line ~249-260):**

```rust
async fn run_producer_loop(
    brokers: String,
    mut rx: mpsc::Receiver<KafkaQueueItem>,
    is_ready: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let batch_max = read_kafka_batch_max();
    let linger = read_kafka_linger();

    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let (partition_ids, clients) = load_partition_clients(&client).await?;

    is_ready.store(true, Ordering::Release);
    // ... loop starts ...
```

**After:**

```rust
async fn run_producer_loop(
    brokers: String,
    mut rx: mpsc::Receiver<KafkaQueueItem>,
    is_ready: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let batch_max = read_kafka_batch_max();
    let linger = read_kafka_linger();

    let client = ClientBuilder::new(vec![brokers]).build().await?;
    let (mut partition_ids, mut clients) = load_partition_clients(&client).await?;

    is_ready.store(true, Ordering::Release);
    log::info!("Kafka producer connected and ready to accept traffic");

    log::info!(
        "Kafka producer ready (topic={TOPIC}, partitions={partition_ids:?}, \
         channel_depth=mpsc, batch_max={batch_max}, linger_ms={})",
        linger.as_millis()
    );

    let mut batch = Vec::with_capacity(batch_max);
    let mut by_partition: HashMap<i32, Vec<KafkaQueueItem>> =
        HashMap::with_capacity(partition_ids.len());
    let mut last_metadata_refresh = Instant::now();
    let metadata_refresh_interval = Duration::from_secs(300);

    loop {
        if last_metadata_refresh.elapsed() > metadata_refresh_interval {
            match load_partition_clients(&client).await {
                Ok((new_ids, new_clients)) => {
                    if new_ids != partition_ids {
                        log::info!(
                            "Kafka partition metadata refreshed: {partition_ids:?} -> {new_ids:?}"
                        );
                    }
                    partition_ids = new_ids;
                    clients = new_clients;
                    last_metadata_refresh = Instant::now();
                }
                Err(e) => {
                    log::warn!("Kafka metadata refresh failed (using stale): {e}");
                }
            }
        }

        match rx.recv().await {
            // ... rest unchanged, but use &partition_ids and &clients ...
```

**Pitfalls:**
- The metadata refresh happens at the top of every loop iteration (before blocking on `rx.recv()`). If the producer is idle (no messages), the refresh won't fire until the next message arrives. This is acceptable — no messages means no routing, so stale metadata is harmless.
- `load_partition_clients` creates new `PartitionClient` instances. Old ones are dropped, closing their connections. This is fine — rskafka handles reconnection internally.
- If this fix is combined with F9 (reuse by_partition HashMap), the `by_partition` declaration moves out of the loop (already done in F9).
- The `metadata_refresh_interval` could be made configurable via env var, but 300s is a reasonable default. Don't over-engineer this.

---

## F13 — Reduce per-row allocations in gateway ingest handler

**File:** `finops-api/src/routes/ingest.rs`

**Why:** For each workload row in a batch: `node_vec.clone()` (heap alloc) + `FlatRow::from_ingest` clones node/batch_id/agent_version (3 heap allocs) + `serde_json::to_vec` (heap alloc) = 5 allocations per row. At 100 rows/batch, 50 batches/sec = 25,000 allocs/sec.

**Step 1:** Use `Arc<[u8]>` for the node key (requires changing `KafkaQueueItem` type):

In `kafka.rs`:

```rust
// Before:
pub type KafkaQueueItem = (Vec<u8>, Vec<u8>);

// After:
use std::sync::Arc;
pub type KafkaQueueItem = (Arc<[u8]>, Vec<u8>);
```

**Step 2:** Update `ingest.rs`:

```rust
// Before:
    let node_vec = batch.node.as_bytes().to_vec();
    for row in &batch.workloads {
        let flat = FlatRow::from_ingest(&batch, row);
        let bytes = match serde_json::to_vec(&flat) { ... };
        match state.kafka_tx.try_send((node_vec.clone(), bytes)) {

// After:
    let node_key: Arc<[u8]> = Arc::from(batch.node.as_bytes());
    for row in &batch.workloads {
        let flat = FlatRow::from_ingest(&batch, row);
        let bytes = match serde_json::to_vec(&flat) { ... };
        match state.kafka_tx.try_send((Arc::clone(&node_key), bytes)) {
```

This changes `node_vec.clone()` (N heap allocs) to `Arc::clone(&node_key)` (N atomic increments — no heap alloc).

**Step 3:** Update `kafka.rs` to extract `Vec<u8>` from `Arc<[u8]>` where needed:

In `produce_grouped_batch`:

```rust
    for (node, payload) in batch.drain(..) {
        let pid = partition_id_for_node(&node, partition_ids);
        by_partition.entry(pid).or_default().push((node, payload));
    }
```

`partition_id_for_node` already takes `&[u8]`, and `&Arc<[u8]>` auto-derefs to `&[u8]`. No change needed there.

In `bytes_to_record`:

```rust
// Before:
fn bytes_to_record(node: Vec<u8>, payload: Vec<u8>, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        key: Some(node),

// After:
fn bytes_to_record(node: Arc<[u8]>, payload: Vec<u8>, ts: chrono::DateTime<Utc>) -> Record {
    Record {
        key: Some(node.to_vec()),  // rskafka Record requires Vec<u8>
```

**Why this is still a win:** We moved from N clones of the node bytes (in the HTTP handler, on the Tokio runtime thread) to 1 allocation + N atomic increments (in the handler) + N `to_vec` (in the background producer task). The HTTP handler latency improves; the producer does the same work it was doing before but off the HTTP thread.

**Advanced (optional):** To eliminate the `FlatRow::from_ingest` clones, create a `FlatRowRef` struct that borrows from the batch:

```rust
#[derive(Serialize)]
struct FlatRowRef<'a> {
    window_start_ns: u64,
    window_end_ns: u64,
    node: &'a str,
    batch_id: &'a str,
    agent_version: &'a str,
    cgroup_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pod: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: &'a Option<String>,
    k8s_resolved: bool,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    exec_count: u32,
    sample_count: u32,
}
```

This eliminates all per-row string clones. Define this struct locally in `ingest.rs` — it doesn't need to be in `finops-wire` because it's a serialization optimization internal to the gateway. But this is a larger refactor — ship F13 Step 1-3 first.

**Pitfalls:**
- `rskafka::record::Record` requires `key: Option<Vec<u8>>`. You cannot pass `Arc<[u8]>` directly. The `.to_vec()` in `bytes_to_record` is unavoidable unless rskafka is patched upstream.
- Adding `use std::sync::Arc;` to `kafka.rs` is needed if not already imported.
- The `on_kafka_dequeued` metric calls don't need changes — they operate on counts, not data.

---

## F14 — Remove `FINAL` from operational read queries

**File:** `finops-api/src/routes/query.rs`

**Why:** `SELECT ... FROM finops.workload_metrics FINAL` forces on-the-fly merge of all overlapping parts. At 100M rows with 50+ unmerged parts, a single query can take 10+ seconds and consume gigabytes of RAM.

**Before:**

```rust
const SUMMARY_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       MAX(memory_bytes_max) AS peak_memory,
       SUM(exec_count) AS total_execs
FROM finops.workload_metrics FINAL
WHERE window_start_ns >= {cutoff_ns:UInt64}
GROUP BY cgroup_id, namespace, pod, container
ORDER BY peak_memory DESC
LIMIT 100
"#;
```

**After:**

```rust
const SUMMARY_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       max(memory_bytes_max) AS peak_memory,
       sum(exec_count) AS total_execs
FROM finops.workload_metrics
WHERE window_start_ns >= {cutoff_ns:UInt64}
GROUP BY cgroup_id, namespace, pod, container
ORDER BY peak_memory DESC
LIMIT 100
"#;
```

**Why this is safe without `FINAL`:**
- The `ORDER BY` key is `(node, window_start_ns, cgroup_id)`. Duplicate rows (from retries) have the same key.
- `GROUP BY cgroup_id, namespace, pod, container` with `max`/`sum` aggregates naturally collapse duplicates — `max` of the same value is the same value; `sum` of duplicated exec counts slightly over-counts, but `ReplacingMergeTree` background merges will eliminate most duplicates within minutes.
- For billing/export queries (where exact correctness matters), keep `FINAL`. For operational dashboards (where speed matters), remove it.
- You can add a comment explaining the trade-off:

```rust
const SUMMARY_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       max(memory_bytes_max) AS peak_memory,
       sum(exec_count) AS total_execs
FROM finops.workload_metrics
WHERE window_start_ns >= {cutoff_ns:UInt64}
GROUP BY cgroup_id, namespace, pod, container
ORDER BY peak_memory DESC
LIMIT 100
"#;

const BILLING_SQL: &str = r#"
SELECT cgroup_id, namespace, pod, container,
       max(memory_bytes_max) AS peak_memory,
       sum(exec_count) AS total_execs
FROM finops.workload_metrics FINAL
WHERE window_start_ns >= {cutoff_ns:UInt64}
GROUP BY cgroup_id, namespace, pod, container
ORDER BY peak_memory DESC
"#;
```

**Pitfalls:**
- Do NOT remove `FINAL` from billing queries. FinOps correctness requires exact deduplication for cost attribution.
- The `exec_count` sum may be slightly inflated on the operational endpoint (duplicate rows before merge). This is acceptable for a dashboard — it self-corrects as ClickHouse merges parts in the background.
- If you later add a billing endpoint, use `BILLING_SQL` with `FINAL`.

---

## Validation Checklist

After implementing all fixes:

```bash
# 1. Compile check (catches type errors from signature changes)
cargo check -p finops-agent -p finops-api -p finops-wire

# 2. Run tests (catches readiness threshold regressions)
cargo test -p finops-api

# 3. Full build
make build

# 4. Smoke test with compose stack
make compose-up
# Wait for /ready → 200
export FINOPS_INGEST_URL=http://127.0.0.1:3000/ingest
sudo -E make run
# Generate traffic, verify ClickHouse count > 0
```

---

## Dependency Notes

- F5 (consume BatchPayload) interacts with F3 (static agent_version). Apply F3 first — it determines whether `BatchPayload::agent_version` is `String` or `&'static str`, which affects the `emit_batch` move semantics.
- F9 (reuse HashMap) and F10 (batch Utc::now) modify the same function (`produce_grouped_batch`). Apply F9 first, then F10 as a follow-up in the same PR.
- F12 (metadata refresh) modifies `run_producer_loop` which is also touched by F9. Apply F9 first.
- F13 (Arc node key) changes `KafkaQueueItem` type, which ripples through `kafka.rs`. Apply F9 and F10 first to minimize merge conflicts.
- All other fixes (F1, F2, F4, F6, F7, F8, F11, F14) are independent and can be applied in any order.

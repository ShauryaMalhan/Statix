# L8 Audit V2 — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Each fix has: file path, root cause, exact code change.
> Do NOT deviate from these instructions. Do NOT invent new abstractions.
> Apply fixes in the order listed. Run `cargo check --workspace` after each fix.

---

## V2-1: Agent SIGTERM Handler (P0-BLOCKS-GA)

**File:** `finops-agent/src/main.rs`

**Root Cause:** The agent only handles `signal::ctrl_c()` (SIGINT). Kubernetes sends SIGTERM for pod eviction. The agent silently loses the current aggregation window on every K8s rolling update or node drain.

**Fix:** Add a SIGTERM handler to the main `tokio::select!` loop. The shutdown logic must flush the partial window and drain the retry queue.

**Step 1:** Add this shutdown future BEFORE the main `loop`:

```rust
#[cfg(unix)]
let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
    .expect("failed to install SIGTERM handler");
```

**Step 2:** Replace the existing `signal::ctrl_c()` arm in the `tokio::select!` block with:

```rust
_ = signal::ctrl_c() => {
    log::info!("SIGINT received — flushing partial window");
    if let Some(batch) = agg.flush(&node, &cache) {
        output::emit_batch(batch);
    }
    println!(r#"{{"status":"shutdown","signal":"SIGINT"}}"#);
    break;
}

#[cfg(unix)]
_ = sigterm.recv() => {
    log::info!("SIGTERM received — flushing partial window for graceful shutdown");
    if let Some(batch) = agg.flush(&node, &cache) {
        output::emit_batch(batch);
    }
    println!(r#"{{"status":"shutdown","signal":"SIGTERM"}}"#);
    break;
}
```

**Verify:** `cargo check -p finops-agent`. Then test: `kill -TERM <pid>` should produce a flush log line + shutdown JSON.

---

## V2-2: ReplacingMergeTree Version Column (P0-BLOCKS-GA)

**File:** `deploy/clickhouse/01_init.sql`

**Root Cause:** `ENGINE = ReplacingMergeTree()` without a version column causes ClickHouse to keep an arbitrary row during merge when duplicates exist (same `ORDER BY` key). If the agent retries a batch with updated `memory_bytes_max`, the older/lower value may be kept. This is silent data corruption in a billing system.

**Fix:** Change the engine declaration to use `window_end_ns` as the version column:

**Find this line:**
```sql
ENGINE = ReplacingMergeTree()
```

**Replace with:**
```sql
ENGINE = ReplacingMergeTree(window_end_ns)
```

**Why `window_end_ns`:** It is always monotonically increasing. A retried batch with more samples will have a later `window_end_ns`, so ClickHouse keeps the most-complete version.

**IMPORTANT:** This is a schema change. Existing data requires migration:
```
docker compose down -v && make compose-up
```

For production with existing data, use `ALTER TABLE finops.workload_metrics MODIFY ENGINE = ReplacingMergeTree(window_end_ns)` (ClickHouse 23.3+).

---

## V2-3: Atomic Batch Delivery in Ingest Handler (P0-BLOCKS-GA)

**File:** `finops-gateway/src/routes/ingest.rs`

**Root Cause:** The `for row in &batch.workloads` loop sends rows one by one via `try_send`. If the channel fills mid-batch, rows already sent are produced to Kafka, but the handler returns 503. The agent retries the entire batch, creating duplicates for the already-sent rows.

**Fix:** Add a capacity pre-check before the loop. Find the `ingest_inner` function and add the capacity check after the schema version validation:

**Find this block (after the schema_version check, before the `for row` loop):**
```rust
    let node_key: Arc<[u8]> = Arc::from(batch.node.as_bytes());

    for row in &batch.workloads {
```

**Replace with:**
```rust
    let required_slots = batch.workloads.len();
    let available_slots = state.kafka_tx.capacity();
    if available_slots < required_slots {
        metrics::counter!("finops_api_kafka_channel_full_total").increment(1);
        log::warn!(
            "Kafka channel has insufficient capacity ({available_slots}/{required_slots} needed); rejecting entire batch"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Ingest channel capacity insufficient for batch. Retry later.",
        )
            .into_response();
    }

    let node_key: Arc<[u8]> = Arc::from(batch.node.as_bytes());

    for row in &batch.workloads {
```

**Note:** The `TrySendError::Full` match arm inside the loop should remain as a safety net (TOCTOU race), but will now rarely trigger.

---

## V2-4: K8s Watch Instead of List Polling (P0-BLOCKS-GA)

**File:** `finops-agent/src/attribution/mod.rs`

**Root Cause:** `refresh_k8s_pods` calls `pods.list(...)` every 30 seconds. At 5000 nodes, this generates 167 full-list API calls/sec to the K8s API server, which will overload etcd.

**Fix:** This is a larger refactor. Replace the list-poll pattern in `main.rs` with a `kube::runtime::watcher`.

**Step 1:** Add dependency to `finops-agent/Cargo.toml`:
```toml
kube = { version = "0.98", default-features = false, features = ["client", "rustls-tls", "runtime"] }
futures = "0.3"
```

**Step 2:** In `finops-agent/src/attribution/mod.rs`, add a new public function:

```rust
pub async fn watch_k8s_pods(
    cache: AttributionCache,
    client: kube::Client,
) {
    use futures::TryStreamExt;
    use kube::runtime::watcher;
    use kube::runtime::watcher::Event;

    let node_name = std::env::var("FINOPS_NODE_NAME")
        .or_else(|_| std::env::var("NODE_NAME"))
        .unwrap_or_else(|_| hostname());

    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> = kube::Api::all(client);
    let lp = kube::api::ListParams::default()
        .fields(&format!("spec.nodeName={node_name}"));

    let mut stream = watcher(pods, watcher::Config::default().list_params(lp))
        .default_backoff();

    while let Ok(Some(event)) = stream.try_next().await {
        match event {
            Event::Apply(pod) | Event::InitApply(pod) => {
                let meta = &pod.metadata;
                let uid = meta.uid.clone().unwrap_or_default();
                if uid.is_empty() { continue; }
                let namespace = meta.namespace.clone().unwrap_or_else(|| "default".into());
                let pod_name = meta.name.clone().unwrap_or_default();
                let mut container = None;
                if let Some(spec) = &pod.spec {
                    if let Some(first) = spec.containers.first() {
                        container = Some(first.name.clone());
                    }
                }
                cache.upsert_pod_labels(uid, WorkloadLabels {
                    namespace: Some(namespace),
                    pod: Some(pod_name),
                    container,
                    pod_uid: None,
                    k8s_resolved: true,
                });
            }
            Event::Delete(pod) => {
                // Optionally remove stale pod labels
            }
            Event::Init | Event::InitDone => {
                merge_cgroup_labels_from_k8s(&cache);
                log::info!("K8s watcher initial sync complete for node {node_name}");
            }
        }
    }
    log::warn!("K8s pod watcher stream ended; pod labels will be stale");
}
```

**Step 3:** In `finops-agent/src/main.rs`, replace the K8s polling task (the `tokio::spawn` block starting at line 54) with:

```rust
let cache_for_k8s = cache.clone();
tokio::spawn(async move {
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
```

**Verify:** `cargo check -p finops-agent`. The old `refresh_k8s_pods` function can be kept for fallback but the polling loop should be removed.

---

## V2-5: DaemonSet preStop Hook (P0-BLOCKS-GA)

**File:** `deploy/k8s/agent-daemonset.yaml`

**Root Cause:** No preStop hook means the agent goes straight from SIGTERM to SIGKILL with no time to flush. Even with V2-1 (SIGTERM handler), the container runtime may kill the process before the flush completes if the HTTP POST to the gateway takes time.

**Fix:** Add lifecycle and terminationGracePeriodSeconds to the agent container spec.

**Find this block in the DaemonSet spec:**
```yaml
      containers:
        - name: finops-agent
          image: finops-agent:latest
          imagePullPolicy: Always
          securityContext:
            privileged: true
```

**Replace with:**
```yaml
      terminationGracePeriodSeconds: 45
      containers:
        - name: finops-agent
          image: finops-agent:latest
          imagePullPolicy: Always
          securityContext:
            privileged: true
          lifecycle:
            preStop:
              exec:
                command: ["sh", "-c", "sleep 5"]
```

The `sleep 5` gives the kubelet time to deregister the pod from endpoints before SIGTERM fires, preventing traffic from arriving during shutdown.

---

## V2-6: Gateway PodDisruptionBudget (P0-BLOCKS-GA)

**File:** `deploy/k8s/gateway.yaml`

**Root Cause:** With 2 replicas and no PDB, `kubectl drain` can evict both simultaneously, causing total ingest blackout.

**Fix:** Append this resource to `deploy/k8s/gateway.yaml`:

```yaml
---
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: finops-gateway-pdb
  namespace: finops-system
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: finops-gateway
```

Also add to the gateway Deployment spec:

```yaml
      terminationGracePeriodSeconds: 30
```

And add a preStop hook to the gateway container:

```yaml
          lifecycle:
            preStop:
              exec:
                command: ["sh", "-c", "sleep 5"]
```

---

## V2-7: Pin Images to Registry Digests (P0-BLOCKS-GA)

**Files:** `deploy/k8s/gateway.yaml`, `deploy/k8s/agent-daemonset.yaml`

**Root Cause:** `:latest` tags are mutable. A bad push to the registry silently breaks all pods on next restart.

**Fix:** After building and pushing images to your registry, replace:
```yaml
image: finops-gateway:latest
```
with:
```yaml
image: <your-registry>/finops-gateway@sha256:<digest>
```

For CI/CD, the build pipeline should output the digest and template it into the manifest. Do NOT hardcode digests in source — use a Kustomize overlay or Helm values file.

---

## V2-8: Cross-AZ Placement Constraints (P0-BLOCKS-GA)

**File:** `deploy/k8s/gateway.yaml`

**Root Cause:** Without topology constraints, both gateway replicas may land in the same AZ. An AZ failure causes total ingest blackout.

**Fix:** Add `topologySpreadConstraints` to the gateway Deployment's pod spec:

**Find:**
```yaml
    spec:
      containers:
        - name: finops-gateway
```

**Add before `containers:`:**
```yaml
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: topology.kubernetes.io/zone
          whenUnsatisfiable: DoNotSchedule
          labelSelector:
            matchLabels:
              app: finops-gateway
```

---

## V2-9: BPF Ring Buffer Wakeup Suppression (P1-WEEK)

**File:** `finops-ebpf/src/main.rs`

**Root Cause:** `entry.submit(0)` sends an epoll wakeup to userspace on every event. At 100k events/sec, this causes 100k context switches/sec between kernel and userspace.

**Fix:** Add a per-CPU counter and only wakeup every 64 events.

**Step 1:** Add a per-CPU wakeup counter map after the existing maps:

```rust
#[map]
static WAKEUP_COUNTER: PerCpuArray<u32> = PerCpuArray::with_max_entries(1, 0);
```

**Step 2:** Replace `entry.submit(0);` in `capture_identity` with:

```rust
let wakeup_flag = match WAKEUP_COUNTER.get_ptr_mut(0) {
    Some(ptr) => unsafe {
        let count = (*ptr).wrapping_add(1);
        *ptr = count;
        if count & 63 == 0 { 0 } else { 1 } // 1 = BPF_RB_NO_WAKEUP
    },
    None => 0, // fallback: always wake
};
entry.submit(wakeup_flag);
```

**Step 3:** In `finops-agent/src/main.rs`, add a 1ms poll timer as a fallback drain mechanism. In the main `tokio::select!` loop, add a new arm:

```rust
_ = poll_interval.tick() => {
    // Drain any events that arrived without an epoll wakeup
    let rb = async_fd.get_mut();
    let mut drained = 0usize;
    while drained < DRAIN_BUDGET {
        let Some(item) = rb.next() else { break };
        if item.len() < size_of::<FinopsEvent>() { continue; }
        let event: &FinopsEvent = unsafe { &*(item.as_ptr() as *const FinopsEvent) };
        if raw_events { output::emit_raw(event); }
        if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
            output::emit_batch(batch);
        }
        drained += 1;
    }
}
```

With `poll_interval` initialized as:
```rust
let mut poll_interval = time::interval(Duration::from_millis(1));
poll_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
```

**IMPORTANT:** The `async_fd.readable_mut()` arm must still exist for wakeup-event draining. The poll timer is a safety net.

**Verify:** Build the BPF program: `cd finops-ebpf && cargo +nightly build --release -Z build-std=core --target bpfel-unknown-none`. Then `cargo check -p finops-agent`.

---

## V2-10: Deduplicate Procfs Reads in `on_identity_event` (P1-WEEK)

**File:** `finops-agent/src/attribution/mod.rs`

**Root Cause:** `on_identity_event` calls `cgroup_path_from_pid(event.pid)` (two blocking syscalls: open + read on `/proc/{pid}/cgroup`) for every `sched_process_exec` event, even if that cgroup_id was already resolved. At 100k events/sec, this is 200k blocking syscalls/sec on the Tokio runtime thread.

**Fix:** Add a read-lock fast path check before the procfs read.

**Find the current `on_identity_event` method:**
```rust
    pub fn on_identity_event(&self, event: &FinopsEvent) {
        let rel_path = cgroup_path_from_pid(event.pid).ok();
        let mut state = self.state.write();
```

**Replace with:**
```rust
    pub fn on_identity_event(&self, event: &FinopsEvent) {
        {
            let state = self.state.read();
            if state.cgroup_paths.contains_key(&event.cgroup_id) {
                return;
            }
        }

        let rel_path = cgroup_path_from_pid(event.pid).ok();
        let mut state = self.state.write();
        if state.cgroup_paths.contains_key(&event.cgroup_id) {
            return;
        }
```

The double-check-locking pattern: read lock for fast path (no procfs I/O), write lock only for first-seen cgroups. On a node with 500 cgroups and 100k exec events/sec, this reduces procfs reads from 100k/sec to ~500 total.

**Verify:** `cargo check -p finops-agent`.

---

## V2-11: Kafka Produce Retry Buffer (P1-WEEK)

**File:** `finops-gateway/src/kafka.rs`

**Root Cause:** When `partition_client.produce()` fails (e.g., during a Kafka rebalance), the failed records are dropped with a `log::warn!`. No retry is attempted. At 100k events/sec with a 30s rebalance, 3M events are silently lost.

**Fix:** Add a bounded retry VecDeque to the producer loop.

**Step 1:** Add at the top of the file:
```rust
use std::collections::VecDeque;

const MAX_RETRY_BATCHES: usize = 32;
```

**Step 2:** In `run_producer_loop`, after the existing `let mut by_partition` declaration, add:
```rust
let mut retry_queue: VecDeque<(i32, Vec<Record>)> = VecDeque::new();
```

**Step 3:** In `produce_grouped_batch`, change the error handling. Find:
```rust
            if let Err(e) = partition_client
                .produce(records, Compression::default())
                .await
            {
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}");
                refresh_partition_metadata(client, partition_ids, clients).await;
            }
```

**Replace with:**
```rust
            if let Err(e) = partition_client
                .produce(records.clone(), Compression::default())
                .await
            {
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}; queuing for retry");
                metrics::counter!("finops_api_kafka_produce_errors_total").increment(1);
                if retry_queue.len() < MAX_RETRY_BATCHES {
                    retry_queue.push_back((pid, records));
                } else {
                    log::error!("Kafka retry queue full ({MAX_RETRY_BATCHES}); dropping {n} records");
                    metrics::counter!("finops_api_kafka_produce_dropped_total").increment(n as u64);
                }
                refresh_partition_metadata(client, partition_ids, clients).await;
            }
```

**Note:** This requires adding `retry_queue: &mut VecDeque<(i32, Vec<Record>)>` as a parameter to `produce_grouped_batch`. Also add retry drain logic at the start of the function:

```rust
// Drain retry queue first
while let Some((pid, records)) = retry_queue.front().cloned() {
    let Some(pc) = clients.get(&pid).cloned() else {
        retry_queue.pop_front();
        continue;
    };
    match pc.produce(records, Compression::default()).await {
        Ok(_) => { retry_queue.pop_front(); }
        Err(_) => break, // Broker still unhealthy; stop retrying
    }
}
```

**Verify:** `cargo check -p finops-gateway`.

---

## V2-12: Stable Partition Hash (P1-WEEK)

**File:** `finops-gateway/src/kafka.rs`

**Root Cause:** `std::collections::hash_map::DefaultHasher` is explicitly documented as non-portable across Rust versions. During rolling deploys, different gateway binaries could route the same node to different Kafka partitions.

**Fix:** Replace `DefaultHasher` with `FxHasher` (already a dependency in the workspace via `rustc-hash`).

**Step 1:** Add `rustc-hash` dependency to `finops-gateway/Cargo.toml`:
```toml
rustc-hash = "1.1"
```

**Step 2:** Find in `kafka.rs`:
```rust
fn hash_node_to_slot(node: &[u8], num_partitions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node.hash(&mut hasher);
    (hasher.finish() as usize) % num_partitions
}
```

**Replace with:**
```rust
fn hash_node_to_slot(node: &[u8], num_partitions: usize) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    node.hash(&mut hasher);
    (hasher.finish() as usize) % num_partitions
}
```

Add at the top of the file:
```rust
use rustc_hash::FxHasher;
```

Remove the now-unused `use std::hash::{Hash, Hasher};` if `Hash` and `Hasher` are no longer needed (they are — `Hash` is used by `.hash()` and `Hasher` is the trait). Keep both imports but replace `DefaultHasher`.

**Verify:** `cargo check -p finops-gateway`.

---

## V2-13: Hoist Node Key Allocation in `bytes_to_record` (P1-WEEK)

**File:** `finops-gateway/src/kafka.rs`

**Root Cause:** `node.to_vec()` inside `bytes_to_record` allocates a new `Vec<u8>` for every record. All records in a partition batch share the same node key, so this allocation is repeated unnecessarily.

**Fix:** In `produce_grouped_batch`, allocate the key once per partition group.

**Find this block in `produce_grouped_batch`:**
```rust
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(node, payload)| bytes_to_record(node, payload, batch_ts))
                .collect();
```

**Replace with:**
```rust
            let key_bytes: Vec<u8> = chunk.first()
                .map(|(node, _)| node.to_vec())
                .unwrap_or_default();
            let records: Vec<Record> = chunk
                .into_iter()
                .map(|(_node, payload)| Record {
                    key: Some(key_bytes.clone()),
                    value: Some(payload),
                    headers: Default::default(),
                    timestamp: batch_ts,
                })
                .collect();
```

This reduces allocations from O(batch_size) to O(1) per partition group. The `key_bytes.clone()` is still O(n) clones, but since `Vec::clone()` is a single memcpy of ~32 bytes, it's faster than the hash+allocate pattern in `bytes_to_record`.

**Alternative (zero-copy):** If `rskafka::Record` ever supports `Bytes` or `Arc<[u8]>` for keys, switch to that. For now, the single-allocation-per-group pattern is sufficient.

**Remove the now-unused `bytes_to_record` function.**

**Verify:** `cargo check -p finops-gateway`.

---

## V2-14: Fix `merge_cgroup_labels_from_k8s` Lock Duration (P1-WEEK)

**File:** `finops-agent/src/attribution/mod.rs`

**Root Cause:** The function acquires a write lock on line 315, then clones the entire `pod_by_uid` HashMap (heap allocation per pod UID string) while holding the write lock. At 500 pods, this blocks hot-path readers for the duration of 500 string clones + 500 label computations.

**Fix:** Split into: (1) read-lock snapshot, (2) compute outside lock, (3) short write-lock batch insert.

**Find the entire `merge_cgroup_labels_from_k8s` function:**
```rust
fn merge_cgroup_labels_from_k8s(cache: &AttributionCache) {
    let mut state = cache.state.write();
    let cgroup_ids: Vec<u64> = state.cgroup_paths.keys().copied().collect();
    let pod_by_uid = state.pod_by_uid.clone();
    for cgroup_id in cgroup_ids {
        ...
    }
}
```

**Replace with:**
```rust
fn merge_cgroup_labels_from_k8s(cache: &AttributionCache) {
    let (cgroup_snap, pod_snap) = {
        let state = cache.state.read();
        let cgroups: Vec<(u64, PathBuf)> = state
            .cgroup_paths
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let pods = state.pod_by_uid.clone();
        (cgroups, pods)
    };

    let mut new_labels: Vec<(u64, Arc<WorkloadLabels>)> =
        Vec::with_capacity(cgroup_snap.len());

    for (cgroup_id, path) in &cgroup_snap {
        let mut labels = labels_from_cgroup_path(Some(path));
        if let Some(uid) = extract_pod_uid_from_path(path) {
            if let Some(pod_labels) = pod_snap.get(&uid) {
                labels.namespace = pod_labels.namespace.clone();
                labels.pod = pod_labels.pod.clone();
                labels.k8s_resolved = true;
                if labels.container.is_none() {
                    labels.container = pod_labels.container.clone();
                }
            }
        }
        new_labels.push((*cgroup_id, Arc::new(labels)));
    }

    let mut state = cache.state.write();
    for (cgroup_id, labels) in new_labels {
        state.cgroup_labels.insert(cgroup_id, labels);
    }
}
```

**Key difference:** The write lock is now held only for the final batch insert (O(n) hash inserts, no allocation), not for the entire label computation + clone phase.

**Verify:** `cargo check -p finops-agent`.

---

## V2-15: Agent-Side Jittered Backoff Recovery (P2-SPRINT)

**File:** `finops-agent/src/output.rs`

**Root Cause:** When the gateway recovers from an outage, all agents flush their retry queues simultaneously (thundering herd). 5000 agents × 60 batches = 300k requests in a burst.

**Fix:** After a successful retry following a series of failures, add a random delay before draining the next retry item.

**Find this block in `init_retry_worker`:**
```rust
                    PostOutcome::Success => {
                        backoff_secs = initial_backoff;
                        break;
                    }
```

**Replace with:**
```rust
                    PostOutcome::Success => {
                        if backoff_secs > initial_backoff {
                            let jitter = rand::random::<f64>() * 5.0;
                            tokio::time::sleep(Duration::from_secs_f64(jitter)).await;
                        }
                        backoff_secs = initial_backoff;
                        break;
                    }
```

The 0-5s random jitter after recovery spreads 5000 agents over 5 seconds instead of all flushing at t=0.

**Verify:** `cargo check -p finops-agent`.

---

## V2-16: ClickHouse Merge Pressure Monitoring (P2-SPRINT)

**File:** New SQL query for Grafana dashboard or alerting.

**Root Cause:** Without monitoring `system.parts` and `system.merges`, there is no visibility into ClickHouse merge backlog. A merge queue that falls behind causes query degradation and eventually `TOO_MANY_PARTS` errors.

**Fix:** Add these queries to the Grafana ClickHouse datasource:

**Parts count alert (fire when > 300 active parts):**
```sql
SELECT
    count() AS active_parts,
    sum(rows) AS total_rows,
    formatReadableSize(sum(bytes_on_disk)) AS disk_size
FROM system.parts
WHERE database = 'finops'
  AND table = 'workload_metrics'
  AND active = 1
```

**Merge queue depth:**
```sql
SELECT
    count() AS active_merges,
    sum(num_parts) AS parts_being_merged,
    formatReadableSize(sum(total_size_bytes_compressed)) AS merge_bytes
FROM system.merges
WHERE database = 'finops'
  AND table = 'workload_metrics'
```

**Alert threshold:** `active_parts > 300` → P1 alert. `active_parts > 1000` → P0 page.

---

## V2-17: Kafka Produce Error Rate Metric (P2-SPRINT)

**File:** `finops-gateway/src/kafka.rs`

**Root Cause:** Kafka produce failures are logged but not metriced. There is no way to alert on produce error rates or build dashboards.

**Fix:** Already partially addressed in V2-11 (the `metrics::counter!("finops_api_kafka_produce_errors_total")` line). If V2-11 is not yet applied, add this counter independently.

**Find in `produce_grouped_batch` the error log line:**
```rust
                log::warn!("Kafka produce failed (partition={pid}, {n} records): {e}");
```

**Add after it:**
```rust
                metrics::counter!("finops_api_kafka_produce_errors_total", "partition" => pid.to_string()).increment(1);
```

**Verify:** `cargo check -p finops-gateway`.

---

## V2-18: End-to-End Latency Histogram (P2-SPRINT)

**File:** `finops-gateway/src/routes/ingest.rs`

**Root Cause:** No metric tracks the time from agent window flush to gateway acceptance. This makes it impossible to diagnose pipeline latency at scale.

**Fix:** Extract the window timestamp from the batch and compute the ingest lag.

**Find in `ingest_inner`, before the `StatusCode::OK` return:**
```rust
    StatusCode::OK.into_response()
```

**Add before it:**
```rust
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let lag_secs = now_ns.saturating_sub(batch.window_end_ns) as f64 / 1_000_000_000.0;
    metrics::histogram!("finops_api_ingest_lag_seconds").record(lag_secs);
```

Wait — `batch` has been moved/consumed by this point. The fix needs the `window_end_ns` captured before the loop. Add at the start of `ingest_inner`:

```rust
    let batch_window_end_ns = batch.window_end_ns;
```

Then use `batch_window_end_ns` in the lag computation before the final return.

**Verify:** `cargo check -p finops-gateway`.

# L8 Audit V2 — Cursor Playbook

> Strict instruction manual for AI-assisted implementation.
> Shipped fixes are removed from this file (ADR index below). Apply open items in order.
> Run `cargo check --workspace` after each fix.

## Shipped (ADR index)

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 | [038](../../../docs/adr/038-phase55-v2-wave1-l8-fixes.md) | V2-1 SIGTERM, V2-2 CH version col, V2-3 atomic ingest, V2-9 BPF wakeup |
| Wave 2 | [039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md) | V2-10 procfs dedup, V2-12 FxHasher, V2-13 key hoist, V2-14 K8s merge lock |
| Wave 3 | [040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md) | V2-5 preStop, V2-6 PDB, V2-11 Kafka retry |
| Wave 4 | [041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md) | V2-4 K8s watch, V2-7 digest pins, V2-8 cross-AZ spread |

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

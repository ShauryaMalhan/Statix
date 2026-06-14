# Phase 13 — Part 1 Playbook: Kafka Removal → Direct ClickHouse Ingest

> **Audience:** the Cursor execution engine.
> **Mode:** queue-less ingestion. After this phase the agent sends telemetry
> straight to `statix-gateway`, which inserts batches directly into ClickHouse
> over its native HTTP interface. Apache Kafka is gone.

## Why this change exists (read before touching code)

Today Kafka sits between the gateway and ClickHouse:

```
agent → POST /ingest → gateway mpsc(8192) → rskafka producer → topic statix-telemetry
        → CH Kafka engine table (statix.kafka_telemetry_queue) → MV → statix.workload_metrics
```

Kafka was the **shock absorber**. When ClickHouse stalled, rows piled up in the
broker and the agents never felt backpressure. Removing Kafka makes the gateway
the **terminal buffer**. The new topology:

```
agent → POST /ingest → gateway mpsc(bounded coalescer) → CH insert worker (RowBinary)
        → INSERT INTO statix.workload_metrics
```

The entire point of Part 1 is **backpressure honesty**. With no broker to absorb
the shock, when ClickHouse stalls the gateway must emit `503 Service
Unavailable` **immediately** — faster than the agent's 5-second HTTP timeout —
so the agent's existing circuit breaker (3 consecutive 5xx → Open) trips and
overflow spills to the agent-local Phase-11 WAL. The gateway must **never**
silently buffer doomed rows in RAM; that converts an absorbable CH stall into
agent-side data loss when the small in-memory queue overflows.

### Two load-bearing physics facts
1. **ClickHouse dies on many small parts.** Per-request inserts are unsafe under
   fan-in. The in-memory mpsc is therefore **retained**, but re-cast from a Kafka
   feed into a **micro-batch coalescer** (linger + max-rows) drained by one
   writer task. Do not insert per HTTP request.
2. **Synchronous insert ACK is the stall detector.** Do **not** enable
   ClickHouse `async_insert`. We batch in the gateway and rely on the synchronous
   `insert.end()` (wrapped in a timeout shorter than the agent's 5s) to learn the
   instant ClickHouse stalls, so we can flip a health flag and fast-fail the next
   POST. `async_insert` would ACK before durability and hide this signal.

### What does NOT change
- The agent. `statix/src/output.rs:211` already treats any `is_server_error()`
  (which includes 503) as retryable; 3 consecutive → circuit Open → WAL spill;
  recovery drains the WAL on the first 200. Our only job is to emit clean 503s
  promptly.
- `statix.workload_metrics`. It is already `ReplacingMergeTree(window_end_ns)`
  with `ORDER BY (node, window_start_ns, cgroup_id)`; it absorbs batched
  RowBinary inserts as-is, and `ReplacingMergeTree` + `FINAL` already dedups
  at-least-once WAL replays.

### Reuse these existing assets — do not reinvent
- `AppState.ch_client: clickhouse::Client` (`main.rs:60,71`) already exists and
  already serves the read path (`routes/query.rs`). **Reuse the same client for
  writes** — cloning shares the connection pool.
- `Config::clickhouse_client()` (`config.rs:46`) — credentialed client factory.
- `statix_wire::FlatRow` (`statix-wire/src/lib.rs:36`) + `FlatRow::from_ingest`
  (`:58`) — owned denormalized row; columns already match `workload_metrics` 1:1.
- `READY_CHANNEL_FULL_THRESHOLD_PCT` + `ingest_channel_over_threshold`
  (`main.rs:26,171`) — keep verbatim for the mpsc readiness gate.

### Confirmed design decisions
- **Insert path:** RowBinary via the already-present `clickhouse = "0.13"` crate
  (`ch_client.insert("statix.workload_metrics")`). Faster than JSONEachRow and
  idiomatic. Use a gateway-local `#[derive(Row)]` struct so the `clickhouse` dep
  stays out of `statix-wire`.
- **Scope of Part 1:** the two named files (`01_init.sql`, `main.rs`) **plus the
  compile-required companions** the AppState change forces — delete `kafka.rs`,
  retype the queue item in `routes/ingest.rs`, drop `rskafka` from `Cargo.toml`.
  Docker-compose Kafka-service removal + ADR/README/skill updates are Part 2.

---

Execute the tasks **in order**. The build only goes green after the final task.

---
### Task: ClickHouse_Schema_Drop_Kafka
- **Target File Path:** `deploy/clickhouse/01_init.sql`
- **Root Cause & Vulnerability Physics:** The Kafka engine table + materialized
  view are the ingest pipe being removed. Dropping the source table while the MV
  still reads it can race mid-teardown, so drop the **consumer (MV) first**, then
  the source. Both are metadata operations external to `workload_metrics` and
  never lock the destination MergeTree. `workload_metrics` needs no schema change;
  `async_insert` is deliberately left disabled so the synchronous insert ACK can
  detect stalls.
- **Prescriptive Refactor Specification:**
  ```sql
  -- DELETE lines 35–64 (the Kafka engine table `kafka_telemetry_queue` and the
  -- materialized view `telemetry_mv`). Replace them with:

  -- Phase 13: Kafka removed — gateway inserts RowBinary batches directly.
  -- Drop the consumer (MV) before the source (Kafka table) so no rows are read mid-teardown.
  DROP VIEW  IF EXISTS statix.telemetry_mv          SYNC;
  DROP TABLE IF EXISTS statix.kafka_telemetry_queue SYNC;

  -- statix.workload_metrics (defined above, lines 12–33) is UNCHANGED:
  --   ReplacingMergeTree(window_end_ns) natively absorbs batched HTTP inserts;
  --   ReplacingMergeTree + FINAL dedups at-least-once WAL replays.
  -- Do NOT add async_insert: the synchronous insert ACK is the stall-detection primitive.
  ```

---
### Task: Gateway_ClickHouse_Writer
- **Target File Path:** `statix-gateway/src/clickhouse_writer.rs`  *(new module — replaces `kafka.rs`)*
- **Root Cause & Vulnerability Physics:** ClickHouse degrades catastrophically on
  many small parts, so per-request inserts are unsafe under fan-in. The mpsc is
  retained as a micro-batch coalescer (linger + max-rows), drained by one writer
  task that owns the `clickhouse::Client` connection pool (keep-alive reused
  between batches — never one connection per request). The synchronous
  `insert.end()`, wrapped in a timeout **shorter than the agent's 5s HTTP
  timeout**, is the stall detector: on timeout/error it flips `ch_healthy=false`
  so the ingest handler can fast-fail the next POST and trip the agent circuit
  breaker into WAL fallback.
- **Prescriptive Refactor Specification:**
  ```rust
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::Arc;
  use std::time::Duration;
  use clickhouse::{Client, Row};
  use serde::Serialize;
  use statix_wire::FlatRow;
  use tokio::sync::mpsc;

  const TABLE: &str = "statix.workload_metrics";

  // Gateway-local RowBinary insert shape (keeps the `clickhouse` dep out of statix-wire).
  // Field order MUST match the workload_metrics column order exactly.
  #[derive(Row, Serialize)]
  struct MetricRow {
      window_start_ns: u64,
      window_end_ns: u64,
      node: String,
      batch_id: String,
      agent_version: String,
      cgroup_id: u64,
      namespace: Option<String>,
      pod: Option<String>,
      container: Option<String>,
      k8s_resolved: bool,
      memory_bytes_max: u64,
      memory_bytes_last: u64,
      exec_count: u32,
      sample_count: u32,
  }
  impl From<FlatRow> for MetricRow { /* 1:1 move of every field */ }

  pub struct ChWriter {
      pub tx: mpsc::Sender<FlatRow>,
      pub channel_capacity: usize,
      pub ch_healthy: Arc<AtomicBool>,
      task: tokio::task::JoinHandle<()>,
  }

  /// Spawn the coalescing writer. `ch_healthy` is the backpressure signal the
  /// ingest handler and /ready read. Seeded false; a startup `SELECT 1` flips it true.
  pub fn spawn_writer(client: Client, channel_capacity: usize) -> ChWriter {
      let (tx, mut rx) = mpsc::channel::<FlatRow>(channel_capacity);
      let ch_healthy = Arc::new(AtomicBool::new(false));
      let flag = ch_healthy.clone();
      let task = tokio::spawn(async move {
          let batch_max = env_usize("STATIX_CH_BATCH_MAX", 1024, 64..=16384);
          let linger    = Duration::from_millis(env_u64("STATIX_CH_LINGER_MS", 50, 1..=1000));
          let ins_to    = Duration::from_secs(env_u64("STATIX_CH_INSERT_TIMEOUT_SECS", 3, 1..=30));
          ping_ready(&client, &flag).await; // SELECT 1 → seed ch_healthy before /ready can pass
          loop {
              // fill_batch coalesces until batch_max rows OR the linger window expires;
              // returns None when all tx clones are dropped (graceful shutdown).
              let Some(batch) = fill_batch(&mut rx, batch_max, linger).await else { break };
              flush_with_retry(&client, &flag, batch, ins_to).await;
          }
      });
      ChWriter { tx, channel_capacity, ch_healthy, task }
  }

  /// One RowBinary INSERT per batch over the pooled connection. The timeout must be
  /// < the agent's 5s HTTP timeout so the gateway flips ch_healthy and returns a
  /// clean 503 *before* the agent's own client times out.
  async fn flush_batch(client: &Client, batch: &[MetricRow], ins_to: Duration)
      -> Result<(), clickhouse::error::Error>
  {
      let mut insert = client.insert(TABLE)?;            // streaming RowBinary POST
      for row in batch { insert.write(row).await?; }
      match tokio::time::timeout(ins_to, insert.end()).await {
          Ok(r)  => r,                                   // Ok ⇒ durable ACK from ClickHouse
          Err(_) => Err(clickhouse::error::Error::Custom("clickhouse insert timeout".into())),
      }
  }

  // flush_with_retry(client, flag, batch, ins_to):
  //   loop: map batch→MetricRow once; flush_batch(...)
  //     Ok  → flag.store(true,  Release); reset backoff; return
  //     Err → flag.store(false, Release); bounded exp backoff + 30% jitter; retry
  //           on retry-budget exhaustion: increment statix_api_ch_insert_dropped_total, drop batch
  //
  // ping_ready(client, flag): client.query("SELECT 1").execute() → flag.store(ok, Release)
  //
  // shutdown(self): drop self.tx, await self.task (drains remaining rx with a final bounded flush).
  ```

---
### Task: Gateway_State_And_Backpressure
- **Target File Path:** `statix-gateway/src/main.rs`
- **Root Cause & Vulnerability Physics:** With Kafka gone the gateway is the
  terminal buffer. `AppState` must drop the Kafka producer handle and expose
  `ch_healthy` so the ingest fast-fail gate and `/ready` reflect ClickHouse
  reachability instantly. `/ready` must 503 when CH is unhealthy so K8s/the LB
  pulls the pod from rotation; `/health` stays pure liveness.
- **Prescriptive Refactor Specification:**
  ```rust
  // --- module wiring (line 6) ---
  // replace `mod kafka;` with:
  mod clickhouse_writer;

  // --- AppState (was lines 28–38) ---
  #[derive(Clone)]
  pub struct AppState {
      pub ingest_tx: mpsc::Sender<statix_wire::FlatRow>, // bounded coalescer, NOT a shock absorber
      pub ingest_channel_capacity: usize,
      pub ch_healthy: Arc<AtomicBool>,                   // writer flips on insert success/timeout
      pub expected_bearer: Option<String>,
      pub ch_client: clickhouse::Client,                 // shared read + write connection pool
  }

  // --- main() init (was lines 60–72): replace spawn_producer with spawn_writer ---
  let ch_client = config.clickhouse_client();
  let writer = clickhouse_writer::spawn_writer(ch_client.clone(), ingest_channel_capacity());
  let ingest_channel_capacity = writer.channel_capacity;
  let state = AppState {
      ingest_tx: writer.tx.clone(),
      ingest_channel_capacity,
      ch_healthy: writer.ch_healthy.clone(),
      expected_bearer: config.expected_bearer().map(str::to_string),
      ch_client,
  };
  // graceful shutdown: call writer.shutdown() where producer.shutdown() was (lines 100–107).
  // `ingest_channel_capacity()` helper: keep the existing STATIX_KAFKA_CHANNEL_SIZE reader but
  // rename the env var to STATIX_INGEST_CHANNEL_SIZE (document old name as removed in Part 2).

  // --- readiness_check (was lines 143–168): swap kafka_ready → ch_healthy ---
  async fn readiness_check(State(state): State<AppState>) -> StatusCode {
      if state.ingest_tx.is_closed() { return StatusCode::SERVICE_UNAVAILABLE; }
      if !state.ch_healthy.load(Ordering::Acquire) { return StatusCode::SERVICE_UNAVAILABLE; }
      let remaining = state.ingest_tx.capacity();
      let total = state.ingest_channel_capacity;
      if ingest_channel_over_threshold(remaining, total, READY_CHANNEL_FULL_THRESHOLD_PCT) {
          return StatusCode::SERVICE_UNAVAILABLE;
      }
      StatusCode::OK
  }
  // health_check (lines 134–140): keep as a pure state.ingest_tx.is_closed() liveness probe.
  ```

---
### Task: Gateway_Ingest_FastFail
- **Target File Path:** `statix-gateway/src/routes/ingest.rs`
- **Root Cause & Vulnerability Physics:** This is the instant-503 surface — the
  direct replacement for Kafka's absorption. Without a broker, buffering rows
  while ClickHouse is down merely defers the loss to the agent WAL on mpsc
  overflow. The **Tier-1 gate** refuses the POST the moment the writer marks CH
  unhealthy — no serialization, no enqueue — so the agent circuit breaker trips
  promptly. The existing `try_reserve_many`→`Full`→503 stays as **Tier-2** for
  the window between a stall starting and the flag flipping. The queue item
  becomes an owned `FlatRow`; the per-row JSON serialization (`FlatRowRef`) is
  deleted because RowBinary is built in the writer.
- **Prescriptive Refactor Specification:**
  ```rust
  // Delete the FlatRowRef struct (lines 17–37) and the node_key/JSON loop (lines 87–150).
  async fn ingest_inner(state: AppState, batch: IngestBatch) -> Response {
      let batch_window_end_ns = batch.window_end_ns;

      // schema_version validation unchanged (MIN=2, MAX=3 → 400) ...

      // TIER 1 — instant fast-fail: refuse new work the moment ClickHouse stalls.
      if !state.ch_healthy.load(std::sync::atomic::Ordering::Acquire) {
          metrics::counter!("statix_api_ch_unhealthy_reject_total").increment(1);
          return (
              StatusCode::SERVICE_UNAVAILABLE,
              "ClickHouse unavailable; retry later.",
          ).into_response();
      }

      // Build owned rows (no JSON here — the writer emits RowBinary). Drop node_key entirely.
      let rows: Vec<statix_wire::FlatRow> = batch
          .workloads
          .iter()
          .map(|w| statix_wire::FlatRow::from_ingest(&batch, w))
          .collect();
      if rows.is_empty() { return StatusCode::OK.into_response(); }

      // TIER 2 — bounded-buffer backpressure (unchanged physics, retyped channel).
      let mut permits = match state.ingest_tx.try_reserve_many(rows.len()) {
          Ok(p) => p,
          Err(mpsc::error::TrySendError::Full(_)) => {
              metrics::counter!("statix_api_ingest_channel_full_total").increment(1);
              return (StatusCode::SERVICE_UNAVAILABLE,
                      "Ingest buffer full. Retry later.").into_response();
          }
          Err(mpsc::error::TrySendError::Closed(_)) => {
              return (StatusCode::SERVICE_UNAVAILABLE,
                      "Writer unavailable.").into_response();
          }
      };
      for row in rows { permits.next().expect("try_reserve_many exact capacity").send(row); }

      // ingest-lag histogram (lines 152–157) unchanged.
      StatusCode::OK.into_response()
  }
  ```

---
### Task: Gateway_Remove_Kafka_Dep
- **Target File Path:** `statix-gateway/Cargo.toml`
- **Root Cause & Vulnerability Physics:** `rskafka` is now dead weight; leaving it
  keeps a transitive broker-protocol dependency and the `kafka.rs` module that no
  longer compiles against the new `AppState`. Removal is required for a green build.
- **Prescriptive Refactor Specification:**
  ```toml
  # DELETE this dependency line:
  rskafka = "0.5"
  ```
  ```text
  # Then delete the file statix-gateway/src/kafka.rs (its `mod kafka;` was already
  # replaced by `mod clickhouse_writer;` in the Gateway_State_And_Backpressure task).
  ```

---
## Verification (run after all tasks)
- `make check` — workspace + nightly BPF check compiles cleanly with `rskafka` gone.
- `cargo test -p statix-gateway` — readiness/threshold unit tests pass (retarget
  any `kafka_*` test identifiers to `ingest_*`).
- `docker compose down -v && make compose-up` — fresh ClickHouse init contains no
  Kafka table or materialized view.
- End-to-end: `sudo -E make run` with `STATIX_INGEST_URL` set; confirm rows land
  in `statix.workload_metrics`; `curl :3000/ready` returns 200.
- **Backpressure drill (the core acceptance test):** pause the ClickHouse
  container. Within `STATIX_CH_INSERT_TIMEOUT_SECS` (3s), `POST /ingest` returns
  503 and `/ready` flips to 503; agent logs show the circuit going Open and
  `statix_wal_frames_written_total` rising. Unpause ClickHouse → `/ready` returns
  200 and the WAL drains (`statix_wal_frames_replayed_total` rises).

## ⚠️ Project-rule companions (Part 2 — NOT in this playbook)
Per `CLAUDE.md`, the same PR wave must also: remove the Kafka/Zookeeper services
from docker-compose, add an ADR under `docs/adr/phase13/`, update `README.md` +
`docs/guides/*`, and update the skill files (`SKILL.md` / `REFERENCE.md` /
`PATTERNS.md` / `TODO.md`). New env vars to document: `STATIX_CH_BATCH_MAX`,
`STATIX_CH_LINGER_MS`, `STATIX_CH_INSERT_TIMEOUT_SECS`, `STATIX_INGEST_CHANNEL_SIZE`.
Removed env vars: `KAFKA_BROKERS`, `STATIX_KAFKA_CHANNEL_SIZE`,
`STATIX_KAFKA_BATCH_MAX`, `STATIX_KAFKA_LINGER_MS`.

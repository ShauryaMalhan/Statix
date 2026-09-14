# ADR index

Every architecture decision, grouped by topic. **The number is the ADR's identity** — it never changes, even when a file moves.

New decision? Add the next number (highest wins), drop it in the right folder, and add a row here.

| Folder | What lives there |
|--------|------------------|
| [`ebpf/`](ebpf/) | BPF program, ring buffer sizing, verifier CI. |
| [`agent/`](agent/) | Aggregator, attribution, clock domain, cgroup sampling. |
| [`ingest/`](ingest/) | Agent→gateway HTTP, retry, WAL spillway, wire contract. |
| [`gateway/`](gateway/) | Endpoints, probes, auth, config, zero-alloc ingest handler. |
| [`storage/`](storage/) | Schema, MergeTree tuning, dedup identity, skip index. |
| [`observability/`](observability/) | Metrics and saturation signals. |
| [`ui/`](ui/) | Read tier and the operator-facing dashboard. |
| [`deploy/`](deploy/) | Docker, Compose, Kubernetes, TLS, secrets. |
| [`fixes/`](fixes/) | Batches of cross-cutting fixes from audit rounds. Cross-cutting by nature. |
| [`meta/`](meta/) | Renames, workspace restructure, roadmap, shared env parsing. |
| [`kafka-legacy/`](kafka-legacy/) | Removed in Phase 13. Kept for history — do not reintroduce. |

## eBPF & kernel

| ADR | Title |
|-----|-------|
| [013](ebpf/013-configurable-ring-buffer-size.md) | Build-time ring buffer tiers + CPU-based ELF selection |
| [022](ebpf/022-bpf-ring-buffer-drop-counter.md) | BPF ring buffer drop counter (`RING_DROPS`) |
| [037](ebpf/037-phase9-ebpf-verifier-ci.md) | Phase 9 eBPF verifier CI (kernel matrix) |

## Agent hot path

| ADR | Title |
|-----|-------|
| [001](agent/001-use-rustc-hash-for-latency.md) | Use `rustc-hash` (`FxHashMap`) for aggregator keys |
| [002](agent/002-double-buffer-aggregator.md) | Double-buffered aggregator maps |
| [003](agent/003-early-flush-instead-of-cap-eviction.md) | Early flush instead of cap eviction |
| [004](agent/004-swap-buffer-before-drain.md) | Flip active buffer before draining on flush |
| [015](agent/015-cgroup-v2-bootstrap-on-startup.md) | Bootstrap existing cgroup v2 workloads on agent startup |
| [016](agent/016-clock-domain-offset.md) | Clock domain offset (BPF monotonic → wall) |
| [047](agent/047-atomic-clock-offset-recalibration.md) | Atomic background clock-offset recalibration (NTP drift) |
| [058](agent/058-phase14-cpu-usage-tracking.md) | Phase 14 — CPU time tracking (`cpu_usage_usec`) |

## Ingest & transport

| ADR | Title |
|-----|-------|
| [005](ingest/005-non-blocking-ingest-pipeline.md) | Non-blocking HTTP → Kafka ingest pipeline |
| [006](ingest/006-shared-http-client-for-ingest.md) | Shared `reqwest::Client` and ingest retry worker |
| [017](ingest/017-batch-lineage-metadata.md) | Batch lineage metadata (`batch_id`, `agent_version`) |
| [020](ingest/020-ingest-schema-version-window.md) | Ingest schema version window (2 and 3) |
| [054](ingest/054-phase11-wal-spillway.md) | Phase 11 — Local disk WAL spillway for the agent |
| [055](ingest/055-phase13-part1-kafka-removal-rowbinary.md) | Phase 13 Part 1 — Kafka removal, direct ClickHouse RowBinary ingest |

## Gateway & API

| ADR | Title |
|-----|-------|
| [012](gateway/012-finops-api-prometheus-metrics.md) | Prometheus metrics on `finops-api` |
| [019](gateway/019-ingest-bearer-token-auth.md) | Bearer token auth on `POST /ingest` |
| [021](gateway/021-ingest-ready-probe.md) | `/ready` probe vs `/health` liveness |
| [027](gateway/027-api-read-path-clickhouse.md) | API read-path — ClickHouse workload summary |
| [029](gateway/029-ready-channel-depth-gate.md) | `/ready` ingest mpsc depth gate (80%) |
| [030](gateway/030-finops-api-config-struct.md) | Centralized `finops-api` `Config` |
| [056](gateway/056-phase13-part2-ingest-zero-alloc.md) | Phase 13 Part 2 — Ingest zero-alloc collapse (single `MetricRow`) |

## Storage (ClickHouse)

| ADR | Title |
|-----|-------|
| [007](storage/007-clickhouse-mergetree-tuning.md) | ClickHouse storage layout (partition, sort key, TTL) |
| [011](storage/011-replacingmergetree-dedupe-identity.md) | ReplacingMergeTree dedupe identity (no `namespace` in sort key) |
| [026](storage/026-clickhouse-finops-database-init.md) | ClickHouse `finops` database init (Target 2) |
| [059](storage/059-phase10-clickhouse-cgroup-skip-index.md) | Phase 10 — ClickHouse `cgroup_id` minmax skip index |

## Observability

| ADR | Title |
|-----|-------|
| [060](observability/060-phase10-golden-signal-saturation-metrics.md) | Phase 10 — Golden-Signal saturation metrics |

## UI & dashboard

| ADR | Title |
|-----|-------|
| [061](ui/061-phase15-dashboard-read-tier.md) | Phase 15 — Dashboard read tier (v1a) |

## Deploy & infra

| ADR | Title |
|-----|-------|
| [009](deploy/009-finops-api-docker-compose.md) | Containerized `finops-api` in Docker Compose |
| [024](deploy/024-agent-production-container.md) | Production agent container (`Dockerfile.statix`) |
| [025](deploy/025-kubernetes-gateway-and-agent.md) | Kubernetes gateway Deployment + agent DaemonSet |
| [031](deploy/031-grafana-clickhouse-compose.md) | Grafana in local Docker Compose (Phase 10) |
| [043](deploy/043-kubernetes-alb-tls-termination.md) | TLS termination at AWS ALB Ingress |
| [046](deploy/046-secrets-env-file.md) | Local secrets via `.env` (ClickHouse password) |
| [057](deploy/057-phase13-part2-infra-kafka-strip.md) | Phase 13 Part 2 — Strip Kafka from compose and K8s manifests |

## Audit & fix waves

| ADR | Title |
|-----|-------|
| [023](fixes/023-phase5-hot-path-fixes.md) | Phase 5 hot-path fixes (attribution, agent metrics, ingest bearer) |
| [032](fixes/032-phase55-l8-p0-hot-path-fixes.md) | Phase 5.5 L8 audit — P0-SHIP agent hot-path fixes |
| [033](fixes/033-phase55-l8-p1-week-gateway-fixes.md) | Phase 5.5 L8 audit — P1-WEEK gateway and agent fixes |
| [034](fixes/034-phase55-l8-p2-ingest-zero-copy.md) | Phase 5.5 L8 audit — P2 ingest zero-copy hot path |
| [038](fixes/038-phase55-v2-wave1-l8-fixes.md) | Phase 5.5 V2 L8 Wave 1 fixes |
| [039](fixes/039-phase55-v2-wave2-l8-fixes.md) | Phase 5.5 V2 L8 Wave 2 fixes |
| [040](fixes/040-phase55-v2-wave3-l8-fixes.md) | Phase 5.5 V2 L8 Wave 3 fixes (durability + K8s eviction) |
| [041](fixes/041-phase55-v2-wave4-l8-fixes.md) | Phase 5.5 V2 L8 Wave 4 fixes (GA hardening) |
| [042](fixes/042-phase55-v2-p2-sprint-l8-fixes.md) | Phase 5.5 V2 P2-SPRINT fixes (GA observability + thundering herd) |
| [049](fixes/049-phase55-v3-wave1-silent-deaths.md) | Phase 5.5 V3 Wave 1 — silent async deaths and atomic ingest |
| [050](fixes/050-phase55-v3-wave2-cache-eviction.md) | Phase 5.5 V3 Wave 2 — cache eviction and K8s reconnect backoff |
| [051](fixes/051-phase55-v3-wave3-distributed-state.md) | Phase 5.5 V3 Wave 3 — distributed state physics |
| [052](fixes/052-phase55-v3-wave4-perf-observability.md) | Phase 5.5 V3 Wave 4 — performance & observability |
| [053](fixes/053-phase55-v3-wave5-micro-arch-polish.md) | Phase 5.5 V3 Wave 5 — micro-architecture polish |

## Project meta

| ADR | Title |
|-----|-------|
| [018](meta/018-phase-roadmap-status.md) | Phase roadmap status |
| [028](meta/028-finops-wire-and-agent-rename.md) | `statix-wire` crate and `statix` rename (Phase 7) |
| [035](meta/035-phase7-workspace-restructure.md) | Phase 7 workspace restructure — `statix-gateway` + `statix-infra` |
| [036](meta/036-phase7-typed-errors-labels-read-path.md) | Phase 7 typed errors and read-only `labels_for_cgroup` |
| [044](meta/044-statix-agent-rename.md) | `finops-agent` → `statix` (company rename) |
| [045](meta/045-statix-platform-rename.md) | FinOps → Statix platform rename (shared crates & ops surface) |
| [048](meta/048-generic-env-positive-parsing.md) | Generic positive-bounded env parsing in `statix-infra` |

## Kafka (historical)

| ADR | Title |
|-----|-------|
| [008](kafka-legacy/008-clickhouse-kafka-engine-resilience.md) | ClickHouse Kafka engine resilience and throughput |
| [010](kafka-legacy/010-kafka-partition-key-by-node.md) | Kafka partition routing by `node` message key |
| [014](kafka-legacy/014-kafka-producer-env-tuning.md) | Env-tuned Kafka producer backpressure and batching |

---

## Reading across topics

Some decisions genuinely belong to more than one topic. They live in one folder but are worth knowing about from several:

- **[023](fixes/023-phase5-hot-path-fixes.md)** — attribution locking *and* agent metrics *and* ingest auth
- **[041](fixes/041-phase55-v2-wave4-l8-fixes.md)** — K8s watcher *and* image pinning *and* cross-AZ placement
- **[052](fixes/052-phase55-v3-wave4-perf-observability.md)** — performance *and* observability *and* deploy QoS
- **[055](ingest/055-phase13-part1-kafka-removal-rowbinary.md)** — ingest transport *and* storage write path
- **[060](observability/060-phase10-golden-signal-saturation-metrics.md)** — metrics that the gateway and agent both emit

That overlap is why `fixes/` exists: an audit wave touches the whole system by nature, so it is filed by *kind of decision*, not by component.

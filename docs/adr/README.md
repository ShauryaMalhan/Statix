# Architecture Decision Records (ADRs)

Point-in-time notes on **why** we chose something—not polished docs. When code changes, add a new numbered file; don't rewrite history.

**Workflow:** Any architectural change must add or update an ADR and sync [enterprise-latency.md](../guides/enterprise-latency.md) + `.cursor/skills/statix-ebpf-agent/`.

**Layout:**

| Folder | ADRs |
|--------|------|
| [adr/](.) (this directory) | 001–031, 035–048 — foundations through Phase 9 |
| [phase55/l8/](phase55/l8/) | 032–034 — L8 audit V1 |
| [phase55/v2/](phase55/v2/) | 038–043 — L8 audit V2 |
| [phase55/v3/](phase55/v3/) | 049–053 — Post-GA V3 waves |
| [phase11/](phase11/) | 054 — Agent WAL spillway |
| [phase13/](phase13/) | 055+ — Queue-less ingest |

See also [phase55/README.md](phase55/README.md) and [docs/README.md](../README.md).

---

## Index (001–048)

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-use-rustc-hash-for-latency.md) | Use `rustc-hash` (`FxHashMap`) in the aggregator | Accepted |
| [002](002-double-buffer-aggregator.md) | Double-buffered aggregator maps | Accepted |
| [003](003-early-flush-instead-of-cap-eviction.md) | Early flush instead of random key eviction | Accepted |
| [004](004-swap-buffer-before-drain.md) | Flip active buffer before draining on flush | Accepted |
| [005](005-non-blocking-ingest-pipeline.md) | HTTP → mpsc → Kafka; ClickHouse Kafka engine | Accepted |
| [006](006-shared-http-client-for-ingest.md) | Shared `reqwest::Client` + ingest retry worker | Accepted |
| [007](007-clickhouse-mergetree-tuning.md) | Storage layout: partitions, sort key, TTL (see 011) | Accepted |
| [008](008-clickhouse-kafka-engine-resilience.md) | Kafka engine: skip broken messages, `kafka_num_consumers` | Accepted |
| [009](009-finops-api-docker-compose.md) | `statix-gateway` in Docker Compose (`Dockerfile.gateway`) | Accepted |
| [010](010-kafka-partition-key-by-node.md) | Kafka partition routing by `node` message key | Accepted |
| [011](011-replacingmergetree-dedupe-identity.md) | ReplacingMergeTree; ORDER BY without `namespace`; `FINAL` reads | Accepted |
| [012](012-finops-api-prometheus-metrics.md) | `GET /metrics`; ingest/Kafka Prometheus instrumentation | Accepted |
| [013](013-configurable-ring-buffer-size.md) | Ring buffer build-time tiers + CPU-based ELF pick | Accepted |
| [014](014-kafka-producer-env-tuning.md) | Kafka mpsc / batch / linger env tuning | Accepted |
| [015](015-cgroup-v2-bootstrap-on-startup.md) | Walk cgroup v2 on startup; inode = cgroup_id | Accepted |
| [016](016-clock-domain-offset.md) | BPF monotonic → wall offset for aggregator windows | Accepted |
| [017](017-batch-lineage-metadata.md) | `batch_id` + `agent_version` on every flush | Accepted |
| [018](018-phase-roadmap-status.md) | Phases 4 & 6 complete; Phase 5 security focus | Accepted |
| [019](019-ingest-bearer-token-auth.md) | `STATIX_API_TOKEN` bearer on `POST /ingest` | Accepted |
| [020](020-ingest-schema-version-window.md) | Accept `schema_version` 2 or 3 on ingest | Accepted |
| [021](021-ingest-ready-probe.md) | `GET /ready` after Kafka connect + metadata | Accepted |
| [022](022-bpf-ring-buffer-drop-counter.md) | `RING_DROPS` per-CPU map + agent poll | Accepted |
| [023](023-phase5-hot-path-fixes.md) | Attribution lock/label cache; agent `:9091/metrics`; `expected_bearer` | Accepted |
| [024](024-agent-production-container.md) | `Dockerfile.statix` — BPF bundle + privileged runtime | Accepted |
| [025](025-kubernetes-gateway-and-agent.md) | `deploy/k8s` gateway Deployment + agent DaemonSet | Accepted |
| [026](026-clickhouse-finops-database-init.md) | `deploy/clickhouse/01_init.sql` — `statix.workload_metrics` | Accepted |
| [027](027-api-read-path-clickhouse.md) | `GET /api/v1/workloads/summary` → ClickHouse | Accepted |
| [028](028-finops-wire-and-agent-rename.md) | `statix-wire` + `finops-user` → `statix` | Accepted |
| [029](029-ready-channel-depth-gate.md) | `/ready` fails when ingest mpsc &gt; 80% full | Accepted |
| [030](030-finops-api-config-struct.md) | `statix-gateway` `Config::from_env()` | Accepted |
| [031](031-grafana-clickhouse-compose.md) | Grafana + ClickHouse plugin on `:3001` (dev) | Accepted |
| [035](035-phase7-workspace-restructure.md) | `statix-gateway` rename + `statix-infra`; drop `ProcessEvent` | Accepted |
| [036](036-phase7-typed-errors-labels-read-path.md) | `GatewayError` + `AttributionError`; read-only `labels_for_cgroup` | Accepted |
| [037](037-phase9-ebpf-verifier-ci.md) | eBPF verifier CI — kernel matrix 5.10–6.8 (virtme-ng + Aya) | Accepted |
| [044](044-statix-agent-rename.md) | `finops-agent` → `statix` company rename | Accepted |
| [045](045-statix-platform-rename.md) | FinOps → Statix platform rename (shared crates, CH, K8s, env) | Accepted |
| [046](046-secrets-env-file.md) | ClickHouse password in `.env`; scrub git history | Accepted |
| [047](047-atomic-clock-offset-recalibration.md) | Atomic clock offset + hourly NTP drift recalibration | Accepted |
| [048](048-generic-env-positive-parsing.md) | Generic `read_env_positive` — reject `<= T::default()` numeric env | Accepted |

## Phase 5.5 L8 audit (grouped)

| ADR | Title | Status |
|-----|-------|--------|
| [032](phase55/l8/032-phase55-l8-p0-hot-path-fixes.md) | L8 P0-SHIP agent hot-path | Accepted |
| [033](phase55/l8/033-phase55-l8-p1-week-gateway-fixes.md) | L8 P1-WEEK gateway + agent fixes | Accepted |
| [034](phase55/l8/034-phase55-l8-p2-ingest-zero-copy.md) | L8 P2 ingest `Arc<[u8]>` + `FlatRowRef` | Accepted |
| [038](phase55/v2/038-phase55-v2-wave1-l8-fixes.md) | V2 Wave 1 — SIGTERM, CH version, atomic ingest, BPF wakeup | Accepted |
| [039](phase55/v2/039-phase55-v2-wave2-l8-fixes.md) | V2 Wave 2 — procfs dedup, FxHasher, key hoist | Accepted |
| [040](phase55/v2/040-phase55-v2-wave3-l8-fixes.md) | V2 Wave 3 — Kafka retry, preStop, gateway PDB | Accepted |
| [041](phase55/v2/041-phase55-v2-wave4-l8-fixes.md) | V2 Wave 4 — K8s watch, digest pins, cross-AZ | Accepted |
| [042](phase55/v2/042-phase55-v2-p2-sprint-l8-fixes.md) | V2 P2-SPRINT — jitter recovery, ingest lag | Accepted |
| [043](phase55/v2/043-kubernetes-alb-tls-termination.md) | AWS ALB Ingress TLS for `/ingest` | Accepted |
| [049](phase55/v3/049-phase55-v3-wave1-silent-deaths.md) | V3 Wave 1 — panic monitors; ingest `try_reserve_many` | Accepted |
| [050](phase55/v3/050-phase55-v3-wave2-cache-eviction.md) | V3 Wave 2 — cache eviction; K8s reconnect backoff | Accepted |
| [051](phase55/v3/051-phase55-v3-wave3-distributed-state.md) | V3 Wave 3 — CH partitions; Kafka consumers; recovery spread | Accepted |
| [052](phase55/v3/052-phase55-v3-wave4-perf-observability.md) | V3 Wave 4 — bootstrap blocking; ring metrics; body limit; QoS | Accepted |
| [053](phase55/v3/053-phase55-v3-wave5-micro-arch-polish.md) | V3 Wave 5 — BPF const; alignment; 5ms poll; `Arc<str>` node | Accepted |

## Phase 11 — Agent reliability

| ADR | Title | Status |
|-----|-------|--------|
| [054](phase11/054-phase11-wal-spillway.md) | Agent WAL spillway + circuit breaker | Accepted |

## Phase 13 — Queue-less ingest

| ADR | Title | Status |
|-----|-------|--------|
| [055](phase13/055-phase13-part1-kafka-removal-rowbinary.md) | Part 1 — Kafka removal; gateway RowBinary → ClickHouse | Accepted |

# L8 Audit — Fix Playbook

**Status:** All fixes shipped. This file is retained as an index of ADRs; remove fix sections from here once landed (do not mark ✅ in place).

| Phase | ADR | Summary |
|-------|-----|---------|
| P0-SHIP | [032](../../../docs/adr/phase55/l8/032-phase55-l8-p0-hot-path-fixes.md) | Agent hot path: env cache, RNG, static version, `DEFAULT_LABELS`, move `BatchPayload`, batched `spawn_blocking`, ring drain budget |
| P1-WEEK | [033](../../../docs/adr/phase55/l8/033-phase55-l8-p1-week-gateway-fixes.md) | `Bytes` retry body, Kafka HashMap/`Utc::now`, cached `kube::Client`, metadata refresh, `argMax` summary |
| P2-SPRINT | [034](../../../docs/adr/phase55/l8/034-phase55-l8-p2-ingest-zero-copy.md) | `Arc<[u8]>` node key + `FlatRowRef` zero-copy ingest |

**Validation:** `cargo check --workspace && cargo test -p statix-gateway`

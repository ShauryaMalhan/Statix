# L8 Audit V2 — Cursor Playbook (SHIPPED)

> **Status:** All V2 fixes shipped for GA. This file is retained as an ADR index.
> **Next:** [L8_POST_GA_FIXES.md](L8_POST_GA_FIXES.md) — V3 Post-GA audit (async silent deaths, cache exhaustion, distributed state physics).
> Run `cargo check --workspace` after each fix.

## Shipped (ADR index)

| Wave | ADR | Items |
|------|-----|-------|
| Wave 1 | [038](../../../docs/adr/038-phase55-v2-wave1-l8-fixes.md) | V2-1 SIGTERM, V2-2 CH version col, V2-3 atomic ingest, V2-9 BPF wakeup |
| Wave 2 | [039](../../../docs/adr/039-phase55-v2-wave2-l8-fixes.md) | V2-10 procfs dedup, V2-12 FxHasher, V2-13 key hoist, V2-14 K8s merge lock |
| Wave 3 | [040](../../../docs/adr/040-phase55-v2-wave3-l8-fixes.md) | V2-5 preStop, V2-6 PDB, V2-11 Kafka retry |
| Wave 4 | [041](../../../docs/adr/041-phase55-v2-wave4-l8-fixes.md) | V2-4 K8s watch, V2-7 digest pins, V2-8 cross-AZ spread |
| P2-SPRINT | [042](../../../docs/adr/042-phase55-v2-p2-sprint-l8-fixes.md) | V2-15 jitter recovery, V2-16 CH merge SQL, V2-18 ingest lag |

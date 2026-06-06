# ADR 039: Phase 5.5 V2 L8 Wave 2 fixes

**Status:** Accepted  
**Date:** 2026-06-06  
**Context:** L8 Audit V2 P1 — shrink RwLock critical sections, eliminate redundant procfs I/O, stable Kafka routing.

## Decision

| ID | Area | Fix |
|----|------|-----|
| V2-10 | `attribution/mod.rs` | `on_identity_event` read-lock fast path — skip `cgroup_path_from_pid` when `cgroup_id` known; double-check after procfs |
| V2-14 | `attribution/mod.rs` | `merge_cgroup_labels_from_k8s` — read-lock snapshot → compute labels outside lock → short write-lock batch insert |
| V2-12 | `finops-gateway/src/kafka.rs` | `FxHasher` replaces `DefaultHasher` for deterministic cross-version partition routing |
| V2-13 | `finops-gateway/src/kafka.rs` | Hoist `node.to_vec()` once per partition chunk in `produce_grouped_batch`; remove `bytes_to_record` |

## Rationale

- **V2-10:** At 100k exec/sec with ~500 cgroups, procfs drops from O(events) to O(new cgroups).
- **V2-14:** Hot-path `labels_for_cgroup` no longer blocked by K8s merge string clones.
- **V2-12:** Rolling gateway deploys must not re-route nodes across partitions ([ADR 001](001-use-rustc-hash-for-latency.md) pattern).
- **V2-13:** One key allocation per micro-batch chunk vs per record.

## Consequences

- **Positive:** Shorter attribution read-lock hold times; stable Kafka keys during deploys.
- **Negative:** `key_bytes.clone()` per record remains (small memcpy); zero-copy keys deferred until `rskafka` supports `Arc<[u8]>`.

## References

- [ADR 038](038-phase55-v2-wave1-l8-fixes.md)
- [ADR 010](010-kafka-partition-key-by-node.md) — routing (amended: FxHasher)

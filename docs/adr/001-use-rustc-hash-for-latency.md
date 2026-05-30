# ADR 001: Use `rustc-hash` (`FxHashMap`) for aggregator keys

**Status:** Accepted  
**Date:** 2026-05-28  
**Context:** Phase 2 aggregator keys are internal `cgroup_id` (`u64`) from the kernel, not attacker-controlled HTTP keys.

## Decision

Use `rustc_hash::FxHashMap` instead of `std::collections::HashMap` in `finops-user/src/aggregator.rs`.

## Rationale

- Default `HashMap` uses SipHash for HashDoS resistance on untrusted inputs.
- Our keys are fixed-width cgroup IDs on a node agent—no DoS surface from hash collisions.
- FxHash is faster on hot `entry` / `iter` paths during exec storms.

## Consequences

- **Positive:** Lower CPU on map lookups and inserts.
- **Negative:** Do not reuse this map for untrusted string keys without revisiting this ADR.
- **Dependency:** `rustc-hash` in `finops-user/Cargo.toml`.

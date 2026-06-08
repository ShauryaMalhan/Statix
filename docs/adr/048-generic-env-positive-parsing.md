# ADR 048: Generic positive-bounded env parsing in `statix-infra`

**Status:** Accepted  
**Date:** 2026-06-08  
**Context:** `statix-infra::env` duplicated parse logic in `read_env_u64` and `read_env_usize`. The agent also had ad-hoc parsers for `STATIX_WINDOW_SECS` / `STATIX_SAMPLE_INTERVAL_SECS` using `.max(1)` instead of the shared helper. Values like `STATIX_WINDOW_SECS=0` parse successfully and propagate into Tokio `interval(Duration::from_secs(0))`, which panics at runtime.

## Decision

Introduce a private generic `read_env_positive<T>` in `statix-infra/src/env.rs`:

- **Trait bounds:** `FromStr`, `PartialOrd`, `Default`, `Copy`, `Display`
- **Safety rule:** accept parsed value only when `v > T::default()`; otherwise log a warning and return the caller-supplied default
- **Public API unchanged:** `read_env_u64` and `read_env_usize` delegate to the generic; `var()` unchanged

Route agent window/sample interval env reads through `read_env_u64` (defaults `10`) instead of local `parse()?.max(1)` helpers.

## Rationale

- Single mathematical gate prevents zero/negative numeric config from entering timers, backoff, or Kafka tuning paths.
- Preserves warn-on-invalid semantics established in [ADR 035](035-phase7-workspace-restructure.md).
- No new dependencies; zero hot-path impact (env parsed once at startup).

## Consequences

- **Positive:** `STATIX_WINDOW_SECS=0` and similar misconfigurations fail safe with a log line instead of a Tokio panic.
- **Negative:** Legitimate use of `0` as a sentinel is impossible via these helpers — intentional; use a separate env key or non-numeric config if needed.
- **Unchanged:** Gateway `kafka.rs` upper-bound clamps (`clamp`, `max(MIN_CHANNEL_SIZE)`) remain after positive parse.

## References

- `statix-infra/src/env.rs`
- `statix/src/main.rs` — window/sample interval defaults
- [ADR 035](035-phase7-workspace-restructure.md)

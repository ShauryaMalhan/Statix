# Architecture Decision Records (ADRs)

Point-in-time notes on **why** we chose something—not polished docs. When code changes, add a new numbered file; don't rewrite history.

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-use-rustc-hash-for-latency.md) | Use `rustc-hash` (`FxHashMap`) in the aggregator | Accepted |
| [002](002-double-buffer-aggregator.md) | Double-buffered aggregator maps | Accepted |
| [003](003-early-flush-instead-of-cap-eviction.md) | Early flush instead of random key eviction | Accepted |
| [004](004-swap-buffer-before-drain.md) | Flip active buffer before draining on flush | Accepted |

# Statix documentation

| Path | Contents |
|------|----------|
| [PRODUCT.md](PRODUCT.md) | **What Statix is for** — the four goals and the "stay light" rule. Read first. |
| [guides/](guides/) | Validation runbooks, ingest contract, enterprise latency principles, production readiness |
| [adr/](adr/) | Architecture Decision Records — numbered history of *why* |
| [adr/INDEX.md](adr/INDEX.md) | **Full ADR index, grouped by topic** — start here |

**Skills (canonical workflow):** [`.cursor/skills/statix-ebpf-agent/`](../.cursor/skills/statix-ebpf-agent/)

When code changes: add/update an ADR, sync the relevant skill files, and run `make build && make check`.

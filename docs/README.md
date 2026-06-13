# Statix documentation

| Path | Contents |
|------|----------|
| [guides/](guides/) | Validation runbooks, ingest contract, enterprise latency principles, production readiness |
| [adr/](adr/) | Architecture Decision Records — numbered history of *why* |
| [adr/phase55/](adr/phase55/) | Phase 5.5 L8 audit waves (L8, V2, V3) grouped by release train |

**Skills (canonical workflow):** [`.cursor/skills/statix-ebpf-agent/`](../.cursor/skills/statix-ebpf-agent/)

When code changes: add/update an ADR, sync the relevant skill files, and run `make build && make check`.

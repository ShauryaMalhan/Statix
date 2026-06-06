# ADR 044: `finops-agent` → `statix` (company rename)

**Status:** Accepted  
**Date:** 2026-06-07  
**Context:** The product company rebranded to Statix. The node agent crate, binary, deploy artifacts, and Cursor skill still used the `finops-agent` / `statix-ebpf-agent` naming, which diverged from the shipped binary (`statix`) and confused onboarding.

## Decision

1. **Rename** workspace directory `finops-agent/` → `statix/`; Cargo package `name = "statix"`.
2. **Verifier binary** remains a separate target: `[[bin]] name = "statix-ebpf-verify"` (`src/bin/verify_ebpf.rs`).
3. **Deploy:** `deploy/docker/Dockerfile.agent` → `Dockerfile.statix`; `deploy/k8s/agent-daemonset.yaml` → `statix-daemonset.yaml`.
4. **Skill:** `.cursor/skills/statix-ebpf-agent/` → `statix-ebpf-agent/`; skill frontmatter `name: statix-ebpf-agent`.
5. **Repo-wide** string updates (order-sensitive): `statix-ebpf-verify`, `finops_agent`, `finops-agent`, `statix-ebpf-agent`, `Dockerfile.agent`, `agent-daemonset.yaml` → Statix equivalents.
6. **Unchanged:** `statix-common`, `statix-wire`, `statix-infra`, `statix-gateway` crate names (FinOps telemetry stack); only the **node agent** surface adopts Statix branding.

## Rationale

- Aligns repository layout, K8s/Docker labels, and `cargo -p statix` with the production binary name.
- Separates company/product identity (Statix agent) from shared FinOps wire/infra crates.
- Supersedes partial rename notes in [ADR 028](028-statix-wire-and-agent-rename.md) where the directory was still `finops-agent`.

## Consequences

- **Positive:** Single canonical path `statix/`; Makefile `AGENT_DIR` and CI `cargo check -p statix` match docs.
- **Negative:** External docs or forks referencing `finops-agent` need a one-time path update.
- **Migration:** `make build && make check`; rebuild agent image with `-f deploy/docker/Dockerfile.statix`; apply `statix-daemonset.yaml`.

## References

- `statix/Cargo.toml`
- `deploy/docker/Dockerfile.statix`
- `deploy/k8s/statix-daemonset.yaml`
- `.cursor/skills/statix-ebpf-agent/SKILL.md`

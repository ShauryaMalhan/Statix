# ADR 046: Local secrets via `.env` (ClickHouse password)

**Status:** Accepted  
**Date:** 2026-06-07  
**Context:** GitGuardian flagged hardcoded `CLICKHOUSE_PASSWORD` in `docker-compose.yml`. Dev passwords were also recoverable from git history.

## Decision

1. **`docker-compose.yml`** — `CLICKHOUSE_PASSWORD: ${CLICKHOUSE_PASSWORD}` (Compose loads repo-root `.env`).
2. **`.env`** — gitignored; developers copy `.env.example` and set local values.
3. **`.env.example`** — committed template with placeholder password only.
4. **Docs** — curl/CLI examples use `$CLICKHOUSE_PASSWORD` after `source .env`; no literal passwords in tracked files.
5. **History** — `git filter-repo --replace-text` removes `statix_dev` / `finops_dev` from all commits; **rotate** local password after rewrite (old values are compromised).

## Rationale

- Compose env interpolation keeps secrets out of YAML committed to GitHub.
- History rewrite closes the “secret in old commits” gap; rotation closes the “already leaked” gap.

## Consequences

- **Onboarding:** `cp .env.example .env` before `make compose-up`.
- **Remote:** after history rewrite, `git push --force-with-lease` required; collaborators must re-clone or reset.
- **Production:** K8s continues to use `statix-secrets` / `clickhouse-password` — not `.env`.

## References

- [ADR 009](009-finops-api-docker-compose.md) — Compose stack
- [ADR 027](027-api-read-path-clickhouse.md) — read-path env

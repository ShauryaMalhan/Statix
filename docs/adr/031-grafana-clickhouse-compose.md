# ADR 031: Grafana in local Docker Compose (Phase 10)

**Status:** Accepted  
**Date:** 2026-06-05  
**Context:** Phase 10 observability — visualize `statix.workload_metrics` in ClickHouse during local dev ([ADR 026](026-clickhouse-finops-database-init.md)).

## Decision

Add **`grafana`** service to `docker-compose.yml`:

| Setting | Value |
|---------|--------|
| Image | `grafana/grafana:latest` |
| Container | `statix-grafana` |
| Port | `3001:3000` (API stays on host `:3000`) |
| Network | `statix-net` |
| Plugin | `grafana-clickhouse-datasource` via `GF_INSTALL_PLUGINS` |
| Auth (dev only) | Anonymous admin (`GF_AUTH_ANONYMOUS_*`) — **not for production** |
| `depends_on` | `clickhouse` |

**ClickHouse datasource (manual in UI):** server `clickhouse`, port `8123`, user `default`, password `statix_dev`, database `finops`.

## Consequences

- **Positive:** One `make compose-up` brings up dashboards stack alongside ingest pipeline.
- **Negative:** Anonymous admin is insecure; prod must use real Grafana auth + secrets.
- **Negative:** First start downloads plugin — may take a minute.

## References

- `docker-compose.yml`
- [ADR 009](009-finops-api-docker-compose.md)

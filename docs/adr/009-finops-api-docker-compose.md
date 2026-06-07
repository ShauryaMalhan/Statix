# ADR 009: Containerized `finops-api` in Docker Compose

**Status:** Accepted  
**Date:** 2026-05-31  
**Context:** Phase 3 local/prod parity — API must start with Kafka/ClickHouse without a separate `cargo run` on the host.

## Decision

- **`Dockerfile.gateway`** — dev Compose image (`rust:1.86-slim` builder; `debian:bookworm-slim` runtime); `KAFKA_BROKERS=kafka:29092` on `statix-net`.
- **`deploy/docker/Dockerfile.gateway`** — production gateway image (non-root `finops` user, `ca-certificates`, `ENTRYPOINT statix-gateway`); build: `docker build -f deploy/docker/Dockerfile.gateway .`
- **`docker-compose.yml`** — `statix-gateway` service on port `3000`, `depends_on: kafka: service_healthy`, Kafka healthcheck via `kafka-broker-api-versions.sh`. Dev image: root `Dockerfile.gateway` ([ADR 035](035-phase7-workspace-restructure.md)).
- **Makefile** — `make compose-up` (default Phase 3 stack): stops **host-only** `statix-gateway` binaries (by `/proc/exe`, never `fuser -k 3000`), starts compose, verifies `http://127.0.0.1:3000/health`, recreates gateway container if unhealthy. `make compose-down` tears down stack.
- **Agent on host** — `STATIX_INGEST_URL=http://127.0.0.1:3000/ingest` (avoid `localhost` → IPv6 `::1` refused). Do **not** run `make run-api` and compose API together on `:3000`.

## Rationale

- One command brings up ingest + storage; matches ECS/K8s deploy shape.
- In-compose broker hostname `kafka:29092` matches ClickHouse Kafka engine table.
- `fuser -k 3000` breaks Docker port-forward; host/API conflict must be handled without killing `docker-proxy`.

## Consequences

- **Positive:** Repeatable Phase 3 dev; API health-gated before agent ingest.
- **Negative:** Image rebuild after gateway changes: `docker compose build statix-gateway && docker compose up -d statix-gateway` (stale images lack routes such as `/metrics` → 404).
- **Negative:** Dev agent remains host-only (`sudo make run`). Production agent image: [ADR 024](024-agent-production-container.md) (`deploy/docker/Dockerfile.statix`).
- **ClickHouse init:** `deploy/clickhouse/01_init.sql` mounted in `docker-compose.yml` ([ADR 026](026-clickhouse-finops-database-init.md)).
- **Read-path env on `statix-gateway`:** `CLICKHOUSE_URL=http://clickhouse:8123`, `CLICKHOUSE_PASSWORD` from `.env` ([ADR 027](027-api-read-path-clickhouse.md), [046](046-secrets-env-file.md)).
- **Grafana (dev):** `statix-grafana` on host `:3001`, ClickHouse plugin — [ADR 031](031-grafana-clickhouse-compose.md).
- **Code:** `Dockerfile.gateway`, `deploy/docker/Dockerfile.gateway`, `docker-compose.yml`, `Makefile`, `.dockerignore`

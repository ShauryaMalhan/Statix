# ADR 065: One gateway Dockerfile, not a dev copy and a prod copy

**Status:** Accepted  
**Date:** 2026-09-24  
**Supersedes:** the part of [ADR 009](009-finops-api-docker-compose.md) that created a separate dev `Dockerfile.gateway` for Compose. ADR 009 is otherwise unedited.

## Context

The gateway had two Dockerfiles: `deploy/docker/Dockerfile.gateway` (production) and a root `Dockerfile.gateway` used only by `docker-compose.yml`. They began as copies and drifted:

| | root (dev) | `deploy/docker` (prod) |
|---|---|---|
| runs as | root | non-root user `statix` |
| Kafka | `ENV KAFKA_BROKERS=kafka:29092` | none |
| builder base | `rust:1.98.1-slim` | `rust:1.98.1-bookworm` |
| start | `CMD` (overridable by accident) | `ENTRYPOINT` |

The dev image still carried Kafka configuration three and a half months after Kafka was removed in Phase 13 ([ADR 055](../ingest/055-phase13-part1-kafka-removal-rowbinary.md), [057](057-phase13-part2-infra-kafka-strip.md)). And because it ran as root, a permission bug in production — a file the gateway can't read or write as `statix` — could never appear in development. **The stack being tested was not the stack being shipped.**

The duplication also had a running cost: one Rust version bump produced two Dependabot PRs ([ADR 064](../meta/064-dependabot-and-cargo-audit.md)), and ADR 062's rule "all images on the same pinned Rust" had to be applied in two places for one image.

## Decision

Compose builds from `deploy/docker/Dockerfile.gateway`. The root file is deleted. There is one gateway image.

`.github/dependabot.yml` drops its `docker` entry for `/`; the `/deploy/docker` entry now covers every image.

**Verified before deleting:** the production image built through Compose, `/ready` returned `200`, and `whoami` inside the container returned `statix`. The check ran first because the one behaviour that changes — running as non-root — only shows up at runtime, not at build time.

## Consequences

- **Positive:** dev now runs the same image, as the same non-root user, as production. Dead Kafka config is gone. Base-image bumps happen once, as one Dependabot PR.
- **Negative:** the builder stage is `bookworm` rather than `slim`, so the first Compose build pulls a larger base image. It is build-stage only; the runtime stage is `debian:bookworm-slim` in both, so the shipped image is unaffected.
- **Scope:** only `make compose-up` builds this image. `scripts/dev-up.sh` runs the gateway as a VM binary and starts only the `clickhouse` container, so it never used either Dockerfile.
- **Still true:** CI does not build Docker images (ADR 064). A base-image bump must be built by hand; there are now two images to build (`Dockerfile.gateway`, `Dockerfile.statix`), not three.

## References

- [ADR 009](009-finops-api-docker-compose.md) — original Compose gateway image (partly superseded)
- [ADR 062](../meta/062-pin-build-toolchain-versions.md) — one pinned toolchain across images; named this duplication as a carrying cost
- [ADR 064](../meta/064-dependabot-and-cargo-audit.md) — where the duplicate first showed up as two PRs for one bump

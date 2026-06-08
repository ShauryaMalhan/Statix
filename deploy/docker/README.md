# Statix container images

Build all commands from **repo root** (`Statix/`).

## API gateway (`statix-gateway`)

`Dockerfile.gateway` — non-root runtime, `ca-certificates` for Kafka TLS.

```bash
docker build -f deploy/docker/Dockerfile.gateway -t statix-gateway:latest .
docker run --rm -p 3000:3000 \
  -e KAFKA_BROKERS=localhost:9092 \
  statix-gateway:latest
```

## eBPF node agent (`statix`)

`Dockerfile.statix` — builds eBPF bundle + agent; **must run privileged/root**.

```bash
docker build -f deploy/docker/Dockerfile.statix -t statix:latest .
docker run --rm --privileged \
  -e STATIX_INGEST_URL=http://host.docker.internal:3000/ingest \
  -e STATIX_API_TOKEN=dev-secret-change-me \
  -p 9091:9091 \
  statix:latest
```

- BPF ELFs: `/app/bpf/statix-ebpf-{small,large,xlarge}` (`STATIX_BPF_DIR=/app/bpf`).
- Metrics: `http://<pod>:9091/metrics`.

Dev Compose uses [`Dockerfile.gateway`](../../Dockerfile.gateway); agent on host: `sudo -E make run`.

Kubernetes: [../k8s/README.md](../k8s/README.md).

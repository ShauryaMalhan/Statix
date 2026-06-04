# FinOps container images

Build all commands from **repo root** (`finops-core`).

## API gateway (`finops-api`)

`Dockerfile.gateway` — non-root runtime, `ca-certificates` for Kafka TLS.

```bash
docker build -f deploy/docker/Dockerfile.gateway -t finops-gateway:latest .
docker run --rm -p 3000:3000 \
  -e KAFKA_BROKERS=localhost:9092 \
  finops-gateway:latest
```

## eBPF node agent (`finops-agent`)

`Dockerfile.agent` — builds eBPF bundle + agent; **must run privileged/root**.

```bash
docker build -f deploy/docker/Dockerfile.agent -t finops-agent:latest .
docker run --rm --privileged \
  -e FINOPS_INGEST_URL=http://host.docker.internal:3000/ingest \
  -e FINOPS_API_TOKEN=dev-secret-change-me \
  -p 9091:9091 \
  finops-agent:latest
```

- BPF ELFs: `/app/bpf/finops-ebpf-{small,large,xlarge}` (`FINOPS_BPF_DIR=/app/bpf`).
- Metrics: `http://<pod>:9091/metrics`.

Dev Compose uses [`Dockerfile.api`](../../Dockerfile.api); agent on host: `sudo -E make run`.

Kubernetes: [../k8s/README.md](../k8s/README.md).

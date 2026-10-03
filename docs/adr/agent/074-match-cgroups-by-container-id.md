# ADR 074: Match container cgroups to Kubernetes containers by container ID

**Status:** Accepted  
**Date:** 2026-10-03  
**Serves:** Goal 1 — per-container requests/limits need each cgroup tied to the right container ([docs/PRODUCT.md](../../PRODUCT.md))

## Context

Every cgroup under a pod was labelled with the pod's **first** container name. A two-container pod on k3s showed three cgroups — `web`, `sidecar` and the pause sandbox — all labelled `web`:

- the watcher stored one label per pod: `spec.containers.first()`;
- the path parser meant to tell containers apart looked for `cri-container-`, but containerd's folders are `cri-containerd-<id>.scope` (verified on k3s), so it never matched and everything fell back to the first name.

Harmless-looking today, but requests are about to be attached per row: the sidecar would be charged `web`'s request and the pause container would be charged too — the same pod costed three times.

Folder naming is not stable across installs. It varies by container runtime (containerd `cri-containerd-<id>.scope`, CRI-O `crio-<id>.scope` plus `crio-conmon-<id>.scope`, Docker `docker-<id>.scope`), cgroup driver (systemd as above; cgroupfs uses a bare `<id>`), and QoS class (Guaranteed pods sit directly under `kubepods.slice`). Only the containerd + systemd layout was verified live; the others are from the kubelet's and runtimes' known formats.

## Decision

**Match by the 64-hex container ID, never by folder prefix.** Every runtime puts the same ID in the cgroup folder name, and Kubernetes reports it in the pod status as `<runtime>://<id>`.

```
cgroup folder  cri-containerd-af22….scope ──ID──► status: containerd://af22… = "web"
cgroup folder  cri-containerd-b01a….scope ──ID──► status: containerd://b01a… = "sidecar"
cgroup folder  cri-containerd-c793….scope ──ID──► in no status list → pause → no name
```

- `container_id_from_dir_name`: folder name minus `.scope`, the text after the last `-`, accepted only if exactly 64 hex characters.
- `pod_uid_from_path`: the last `pod` in each path component followed by a real UUID (`_` or `-`) — systemd and cgroupfs drivers, all QoS classes. Fixed Guaranteed pods, which the old `-pod` split never matched.
- `PodInfo` per pod (namespace, name, **ID → name** map) replaces the single label. `pod_info_from` builds it from `containerStatuses`, `initContainerStatuses` and `ephemeralContainerStatuses` (init and debug containers have cgroups too), stripping any `<runtime>://` prefix with `split_once("://")`.
- `container_name_in_pod` returns `None` for the pause sandbox (ID in no status list), a container whose status has not arrived yet, and CRI-O's `crio-conmon-<id>` — it carries the real container's ID and would otherwise be charged that container's requests twice.
- Outside Kubernetes (plain Docker) the label is the 12-character short ID, as `docker ps` shows; there is no other name.
- `extract_container_from_path` is deleted.
- The watcher stores `InitApply` pods and merges **once** at `InitDone` (was one full merge per pod, and a duplicate "sync complete" log line); live `Apply` events — including a container starting, which adds its ID to the status — merge immediately.
- New gauge **`statix_k8s_unmatched_cgroups`**: cgroups under a pod UID the watcher doesn't know. An unfamiliar layout shows up as a number, not as silently wrong labels.

### Verified (k3s 1.36, containerd + systemd driver, 2026-10-03)

The two-container `reqtest` pod: `web` (cgroup 10215), `sidecar` (10308), and the pause container with no name (10122). `statix_k8s_unmatched_cgroups 0`. The initial-list log line appears once. Unit tests cover every layout in Context plus the conmon and pause cases, using real k3s path shapes and a real `Pod` deserialised from JSON.

## Consequences

- **Positive:** each row is one container with its own name; the pause sandbox is visible as unnamed overhead instead of a phantom copy of the first container. The pause-label and "stronger cgroup → pod mapping" TODOs are resolved by this.
- **Known transient:** a new container's cgroup appears a moment before Kubernetes reports its ID, so it is unnamed (and, from the next stage, has no request) until the next `Apply` event.
- **Unverified layouts:** CRI-O, Docker and cgroupfs are covered by unit tests, not by a live cluster. The gauge is how a mismatch would surface.

## Follow-up: requests and limits per container (2026-10-04)

Each `PodInfo` also holds **container name → `Resources`** (CPU/memory × request/limit as plain millicores and bytes; **0 = not set**), filled from `spec.containers` and `spec.initContainers` (ephemeral containers cannot set resources). The lookup is cgroup → ID → name → resources, so the pause sandbox, CRI-O conmon and non-Kubernetes containers carry zeros — they reserve nothing, and must not be charged.

Kubernetes quantities are parsed by `cpu_millicores` (`250m`, `1`, `0.5`) and `memory_bytes` — binary (`Ki`…`Ei`, ×1024) and decimal (`k`…`E`, ×1000) suffixes, plain bytes and exponent form (`1e9`). `Mi` vs `M` differ by ~5%, so both are handled. Junk, negative and non-finite input gives `None` → 0, never a panic. A limit-only container needs no special case: the API server copies the limit into the request at admission, so the pod the watcher receives already has both.

Unit-tested with the `reqtest` values (`web` 250m/128Mi, limits 500m/256Mi; `sidecar` 50m/32Mi, no limit → 0). Not yet visible outside the agent: the wire format and ClickHouse columns come in the next stage.

## References

- [ADR 066](066-sample-leaf-cgroups-only.md) — leaf-only sampling (each container is a leaf)
- [ADR 072](../deploy/072-agent-kubernetes-access.md) — how the agent reaches Kubernetes
- [ADR 073](../deploy/073-kube-4-minimum-kubernetes-1-32.md) — the `kube`/`k8s-openapi` versions these types come from

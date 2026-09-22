# ADR 064: Dependabot and `cargo audit`; images move to `rust:1.98.1`

**Status:** Accepted  
**Date:** 2026-09-23  
**Builds on:** [ADR 062](062-pin-build-toolchain-versions.md) — pinning. This ADR is the other half: how pins are kept from going stale, and how vulnerable dependencies get noticed.

## Context

ADR 062 pinned every externally-fetched build tool after an unpinned `cargo install bpf-linker` broke CI on a markdown-only commit. Pinning fixed the silent drift, but created two new problems it did not address:

1. **Pins go stale invisibly.** Nothing reports that a newer version exists, including a security fix.
2. **Nothing checked the dependency tree at all.** ~200 transitive crates, never audited.

The second turned out not to be hypothetical.

## Decision

### `cargo audit` in CI, failing on vulnerabilities only

Added to the userspace job as two steps — install, then run — so a failure names itself rather than reporting "audit failed" when the install is what broke. Pinned to `0.22.2` with an exact-version check plus `--force`, per ADR 062: the runner caches `~/.cargo/bin`, and a cached binary silently ignoring the pin is the exact mechanism that hid the bpf-linker problem for months.

**Left at the default: fail on `vulnerability`, report `unmaintained` / `unsound`.** Three of the five current warnings (`backoff`, `instant`, `rustls-pemfile`) are transitive and cannot be fixed here until upstream moves. `--deny warnings` would make CI permanently red through no fault that can be corrected, and **a check that is always red trains people to ignore it** — so the next real advisory arrives into a sea of red nobody reads.

### Dependabot, weekly, with PR limits

`.github/dependabot.yml` covers `cargo` (root **and** `statix-ebpf`, which is deliberately outside the workspace with its own lockfile), `github-actions`, and `docker` (root **and** `deploy/docker`, because the gateway Dockerfile is still duplicated — an open P0 item, here showing its cost as two PRs for one bump).

Weekly rather than daily; daily is noise. `open-pull-requests-limit` because an unbounded first run opens fifteen PRs that all get ignored.

The value is not the bumping. It is that **each upgrade arrives as one reviewable PR judged by CI** — green or red, before merge. Compare with how the bpf-linker break was discovered: a five-week-old cache expired and an unrelated commit exploded.

### All images move to `rust:1.98.1`

Merged together, never one at a time. The workspace shares one `Cargo.lock`; building parts of it with different compilers means the next dependency that raises an MSRV breaks only one image, later, with nothing related to point at. That failure was already paid for once in `c932c81`.

## What the first audit run found

Four vulnerabilities, all transitive, all fixed by a targeted `cargo update` with no source changes:

| Crate | Fix | Advisory | |
|---|---|---|---|
| `rustls` | 0.23.40 → 0.23.45 | RUSTSEC-2026-0285 | TLS 1.3 handshake confusion — **reachable**, the K8s pod watcher speaks TLS to the API server |
| `h2` | 0.4.14 → 0.4.19 | RUSTSEC-2026-0258 | unbounded empty DATA frames — **reachable**, the gateway serves HTTP |
| `quinn-proto` | 0.11.14 → 0.11.18 | RUSTSEC-2026-0185 | 7.5 high, remote memory exhaustion — **not reachable**, this project never speaks QUIC |
| `crossbeam-epoch` | 0.9.18 → 0.9.21 | RUSTSEC-2026-0204 | invalid pointer deref in a `fmt` impl — barely reachable |

Worth recording because the ordering is counterintuitive: **the highest CVSS (7.5) was the least relevant, and a medium was the most.** CVSS scores the worst case for anyone; reachability decides whether it is your problem. Both are needed, and neither alone is a triage.

## Consequences

- **Positive:** vulnerable dependencies now fail the build instead of sitting unnoticed. Upgrades arrive pre-judged by CI. Four real advisories closed on day one.
- **Negative:** Dependabot will open PRs that fail CI. That is the system working — it found an incompatible bump before you did. Close or pin around them; **do not disable the bot because it brought bad news.**
- **Operational — CI does not build the Docker images.** The two `rust:1.98.1` PRs were green because CI never touched them. All three images were built by hand and the agent artifact checked (three ring-buffer ELF tiers present, binary starts) before merging. Anyone bumping a base image in future must do the same; a green tick means nothing there.
- **Docs:** SKILL.md now states the *rule* ("all images build on the same pinned Rust version — see the Dockerfiles") rather than the *number*, so it cannot go stale on the next bump. ADR 062 still names `rust:1.97.1` and is deliberately unedited — it was accurate when written.
- **Deliberately not merged:** `aya-ebpf 0.1.1 → 0.2.1`. Under Cargo's rules a `0.x` minor bump **is** a breaking change, which is why Dependabot had to edit the manifest rather than only the lockfile. CI proves it compiles and that the verifier loads it on all four kernels; it does not prove the agent still collects correct data. That one needs a live run first.

## References

- [ADR 062](062-pin-build-toolchain-versions.md) — pinning policy (this ADR is its maintenance half)
- [ADR 037](../ebpf/037-phase9-ebpf-verifier-ci.md) — the eBPF verifier matrix that judges `statix-ebpf` bumps
- Commit `8e1cfc5` — virtme-ng pin, cargo audit, Dependabot, the four advisory fixes

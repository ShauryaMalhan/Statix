# What Statix is for

**A read-only Kubernetes waste report.** Open-source core, **self-hosted by each company**: one install per company, their data never leaves their cluster ([ADR 070](adr/meta/070-product-scope-self-hosted-waste-report.md)).

It is an end-to-end pipeline — node agent → gateway → ClickHouse → report — whose only output is *where compute money is wasted*. It is **not** a general monitoring app: no alerting, logs, traces, or dashboards-for-everything. Prometheus/Grafana already do that.

## Goals

| # | Goal | What it answers | Status |
|---|------|-----------------|--------|
| 1 | **Right-size pods by need** | Is this pod's request too big, or is it starving? Usage **and** pressure — CPU wait (PSI), CPU capping (throttling), memory pressure / OOM kills — with working-set memory, compared against each pod's requests/limits. | usage ✅ · working set, pressure, requests/limits: open |
| 2 | **Count the CPU of short-lived jobs** | What did CronJobs / Jobs actually cost? | open — leftover CPU per parent first, per-job names later |
| 3 | **Find zombie pods** | Which pods did nothing for days — near-zero CPU **and** no network traffic? | open |
| 4 | **Show egress cost per workload** | Who is paying for cross-AZ, internet and NAT traffic? | open, largest |

**Later, not now:** automatic in-place resizing, ML forecasting.

## The one design rule: stay light

Every feature has to earn its weight. In order of preference:

1. **Read a file the kernel already maintains** (cgroupfs, procfs) — e.g. `cpu.stat`, `memory.stat`, `cpu.pressure`, `memory.events`. The kernel has done the accounting; we only read it once per window.
2. **Only if (1) cannot answer it**, add eBPF — and prefer attaching once over per-event work on hot paths.
3. **No new moving parts.** One agent per node, one gateway, one ClickHouse. No queues, no sidecars, no extra databases. Kafka was removed for exactly this reason ([ADR 055](adr/ingest/055-phase13-part1-kafka-removal-rowbinary.md)).

Before building anything: **verify the kernel fact it depends on, on a real node**, then build. Numbers that are wrong are worse than numbers that are missing — a waste report nobody trusts is useless.

Open work, ordered by these goals: [TODO.md](../.cursor/skills/statix-ebpf-agent/TODO.md).

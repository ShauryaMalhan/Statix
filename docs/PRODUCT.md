# What Statix is for

**A read-only Kubernetes waste report.** Open-source core, **self-hosted by each company**: one install per company, their data never leaves their cluster ([ADR 070](adr/meta/070-product-scope-self-hosted-waste-report.md)).

It is an end-to-end pipeline — node agent → gateway → ClickHouse → report — whose only output is *where compute money is wasted*. It is **not** a general monitoring app (no alerting, logs, traces, or dashboards-for-everything; Prometheus/Grafana do that), and **not an auto-sizer**: it tells a company what to change and what it saves; the company decides.

## What the report says

Per service (a Deployment/StatefulSet, not a pod — pod names change every deploy):

> **checkout-api** costs **$412/month**. Set CPU to **210m** (now 1000m) and memory to **950 MiB** (now 2 GiB): **saves ~$290/month.**
> *Based on 14 days — CPU p95 180m · p99 240m · max 410m. Memory daily peak 820 MiB.*

**Every insight has this shape:** one recommendation, what it is worth in $, and the evidence underneath. The manager acts on line 1; the engineer trusts it because of line 2. The same shape applies to zombie pods, short-lived job cost and egress.

- **Cost** — what the service *reserves* (its requests) × the company's price per vCPU-hour and GiB-hour. Reserved capacity is what fills nodes, so it is what the company pays for.
- **Recommendation** — CPU and memory fail differently, so they get different rules:
  - **CPU → p95 + 15%.** Too little CPU makes an app *slower* (throttled); a percentile is safe.
  - **Memory → highest daily peak + 15%.** Too little memory gets a process *killed* (OOM); a p95 would crash it 5% of the time. Never recommend memory from a percentile of all samples.
  - Both over **at least 7 days**, so weekly peaks (Monday traffic, nightly batch) are included. Close to the Kubernetes VPA recommender's defaults (p90 + 15% margin, memory from per-24h peaks, 8 days of history), slightly more conservative on CPU.
- **Savings** — (current request − recommendation) × price.
- **Evidence** — p95 / p99 / max CPU and the daily memory peak, computed in ClickHouse from the per-window rows the agent already writes.
- **"Increase this one"** — usage alone can only ever say *shrink*: a service throttled at its limit cannot use more than the limit, so its p95 looks fine while it starves. Pressure signals (throttling, OOM kills) are what make an honest "increase" recommendation possible.

## Goals

| # | Goal | What it answers | Status |
|---|------|-----------------|--------|
| 1 | **Cost and savings per service** | What does each service cost, what should its CPU and memory be set to, and what does that save? Needs requests/limits, service grouping, a price, the report; then pressure signals for "increase this one". | usage ✅ · working set ✅ · requests/limits ✅ · grouping, price, report, pressure: open |
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

//! statix — eBPF agent: attribution, memory sampling, batched stdout or HTTP ingest.
//! Phases 1–4 + 6 shipped; Phase 5 adds ingest auth (`STATIX_API_TOKEN`).

mod aggregator;
mod bpf_memlock;
mod attribution;
mod ebpf_select;
mod loader;
mod memory_sampler;
mod output;

use std::mem::size_of;

use statix_common::StatixEvent;
use tokio::io::unix::AsyncFd;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], 9091))
        .install()
        .unwrap_or_else(|e| log::warn!("Failed to install prometheus recorder: {e}"));
    log::info!("Agent Prometheus metrics exposed on http://0.0.0.0:9091/metrics");
    output::init_http_client();
    if let Some(url) = statix_infra::env::var("STATIX_INGEST_URL") {
        output::init_retry_worker(url);
    }
    check_privileges()?;

    let clock_offset = statix_infra::clock::init_clock_offset();
    log::info!("Clock domain offset: {clock_offset} ns (BPF/monotonic → wall)");
    spawn_clock_recalibration_task();

    let ebpf_path = ebpf_select::resolve_ebpf_path()?;
    let window_secs = statix_infra::env::read_env_u64("STATIX_WINDOW_SECS", 10);
    let sample_secs = statix_infra::env::read_env_u64("STATIX_SAMPLE_INTERVAL_SECS", 10);
    let node = read_node_name();
    let raw_events = statix_infra::env::var("STATIX_RAW_EVENTS").as_deref() == Some("1");

    let mut bpf = loader::load_and_attach(&ebpf_path)?;
    let ring_drops = loader::take_ring_drops_map(&mut bpf)?;
    let mut ring_monitor_handle = loader::spawn_ring_drops_monitor(ring_drops);
    let ring_buf = loader::get_events_ring_buf(&mut bpf)?;
    let mut async_fd = AsyncFd::new(ring_buf)?;

    let cache = attribution::AttributionCache::new();
    let mut agg = aggregator::Aggregator::new(window_secs);

    let mut flush_interval = time::interval(Duration::from_secs(window_secs));
    flush_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    let mut sample_interval = time::interval(Duration::from_secs(sample_secs));
    sample_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    let in_k8s = std::env::var("KUBERNETES_SERVICE_HOST").is_ok();
    let mut k8s_handle = spawn_k8s_watcher(cache.clone());

    if let Some(url) = statix_infra::env::var("STATIX_INGEST_URL") {
        log::info!("Ingest: POST batches to {url}");
        log::info!("Phase 5 (Production Readiness): API must be secured via STATIX_API_TOKEN");
        log::info!("Dev stack: make compose-up (other terminal) until auth is enforced");
    } else {
        log::info!("Ingest: stdout (set STATIX_INGEST_URL for HTTP ingest)");
    }
    log::info!(
        "Agent ready (window={window_secs}s, sample={sample_secs}s, node={node})"
    );
    println!(
        r#"{{"status":"ready","probe":"sched:sched_process_exec","schema_version":{}}}"#,
        output::SCHEMA_VERSION
    );

    for batch in attribution::bootstrap_existing_cgroups(&cache, &mut agg, &node).await {
        output::emit_batch(batch);
    }

    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    let mut poll_interval = time::interval(Duration::from_millis(1));
    poll_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    const DRAIN_BUDGET: usize = 256;

    loop {
        tokio::select! {
            guard_result = async_fd.readable_mut() => {
                let mut guard = guard_result?;
                let rb = guard.get_inner_mut();
                let mut drained = 0usize;
                while drained < DRAIN_BUDGET {
                    let Some(item) = rb.next() else { break };
                    if item.len() < size_of::<StatixEvent>() {
                        log::warn!("Undersized event ({} bytes), skipping", item.len());
                        continue;
                    }
                    let event: &StatixEvent =
                        unsafe { &*(item.as_ptr() as *const StatixEvent) };
                    if raw_events {
                        output::emit_raw(event);
                    }
                    if let Some(batch) = agg.on_statix_event(event, &cache, &node) {
                        output::emit_batch(batch);
                    }
                    drained += 1;
                }
                guard.clear_ready();
            }

            _ = flush_interval.tick() => {
                if let Some(batch) = agg.flush(&node, &cache) {
                    output::emit_batch(batch);
                }
            }

            _ = sample_interval.tick() => {
                for batch in
                    memory_sampler::sample_tracked_cgroups(&cache, &mut agg, &node).await
                {
                    output::emit_batch(batch);
                }
            }

            _ = poll_interval.tick() => {
                let rb = async_fd.get_mut();
                let mut drained = 0usize;
                while drained < DRAIN_BUDGET {
                    let Some(item) = rb.next() else { break };
                    if item.len() < size_of::<StatixEvent>() {
                        continue;
                    }
                    let event: &StatixEvent =
                        unsafe { &*(item.as_ptr() as *const StatixEvent) };
                    if raw_events {
                        output::emit_raw(event);
                    }
                    if let Some(batch) = agg.on_statix_event(event, &cache, &node) {
                        output::emit_batch(batch);
                    }
                    drained += 1;
                }
            }

            _ = &mut k8s_handle, if in_k8s => {
                match k8s_handle.await {
                    Ok(()) => log::warn!("K8s watcher exited unexpectedly"),
                    Err(e) => {
                        log::error!("CRITICAL: K8s watcher task panicked: {e}");
                        metrics::counter!("statix_k8s_watcher_panics_total").increment(1);
                    }
                }
                k8s_handle = spawn_k8s_watcher(cache.clone());
            }

            _ = &mut ring_monitor_handle => {
                match ring_monitor_handle.await {
                    Ok(()) => log::warn!("Ring drops monitor exited unexpectedly"),
                    Err(e) => {
                        log::error!("CRITICAL: Ring drops monitor task panicked: {e}");
                        metrics::counter!("statix_ring_monitor_panics_total").increment(1);
                    }
                }
                // Map was moved into the monitor task; cannot respawn — park this branch.
                ring_monitor_handle = tokio::spawn(async {
                    std::future::pending::<()>().await
                });
            }

            _ = signal::ctrl_c() => {
                log::info!("SIGINT received — flushing partial window");
                if let Some(batch) = agg.flush(&node, &cache) {
                    output::emit_batch(batch);
                }
                println!(r#"{{"status":"shutdown","signal":"SIGINT"}}"#);
                break;
            }

            _ = sigterm.recv() => {
                log::info!("SIGTERM received — flushing partial window for graceful shutdown");
                if let Some(batch) = agg.flush(&node, &cache) {
                    output::emit_batch(batch);
                }
                println!(r#"{{"status":"shutdown","signal":"SIGTERM"}}"#);
                break;
            }
        }
    }

    Ok(())
}

fn spawn_k8s_watcher(cache: attribution::AttributionCache) -> JoinHandle<()> {
    tokio::spawn(async move {
        if std::env::var("KUBERNETES_SERVICE_HOST").is_err() {
            log::info!("Not in K8s — pod watch disabled");
            return;
        }
        match kube::Client::try_default().await {
            Ok(client) => {
                attribution::watch_k8s_pods(cache, client).await;
            }
            Err(e) => {
                log::warn!("K8s client init failed; pod resolution disabled: {e}");
            }
        }
    })
}

fn read_node_name() -> String {
    statix_infra::env::var("STATIX_NODE_NAME")
        .or_else(|| std::env::var("NODE_NAME").ok())
        .unwrap_or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "localhost".into())
        })
}

/// Periodic NTP drift correction — syscalls stay off the ring-buffer hot path.
fn spawn_clock_recalibration_task() {
    let secs = statix_infra::env::read_env_u64("STATIX_CLOCK_RECALIBRATE_SECS", 3600);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(secs));
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            statix_infra::clock::recalibrate_clock_offset();
        }
    });
}

fn check_privileges() -> anyhow::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        anyhow::bail!(
            "Must run as root or with CAP_BPF + CAP_PERFMON (+ CAP_SYS_ADMIN for cgroup reads)."
        );
    }
    Ok(())
}

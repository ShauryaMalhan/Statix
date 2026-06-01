//! finops-user — attribution, memory sampling, batched stdout or HTTP ingest (Phase 3).

mod aggregator;
mod attribution;
mod loader;
mod memory_sampler;
mod output;

use std::mem::size_of;

use finops_common::FinopsEvent;
use tokio::io::unix::AsyncFd;
use tokio::signal;
use tokio::time::{self, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    output::init_http_client();
    if let Ok(url) = std::env::var("FINOPS_INGEST_URL") {
        output::init_retry_worker(url);
    }
    check_privileges()?;

    let ebpf_path = read_ebpf_path()?;
    let window_secs = read_window_secs()?;
    let sample_secs = read_sample_interval_secs()?;
    let node = read_node_name();
    let raw_events = std::env::var("FINOPS_RAW_EVENTS").ok().as_deref() == Some("1");

    let mut bpf = loader::load_and_attach(&ebpf_path)?;
    let ring_buf = loader::get_events_ring_buf(&mut bpf)?;
    let mut async_fd = AsyncFd::new(ring_buf)?;

    let cache = attribution::AttributionCache::new();
    let mut agg = aggregator::Aggregator::new(window_secs);

    let mut flush_interval = time::interval(Duration::from_secs(window_secs));
    flush_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    let mut sample_interval = time::interval(Duration::from_secs(sample_secs));
    sample_interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    let cache_for_k8s = cache.clone();
    tokio::spawn(async move {
        let mut k8s_interval = time::interval(Duration::from_secs(30));
        k8s_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            k8s_interval.tick().await;
            if let Err(e) = attribution::refresh_k8s_pods(&cache_for_k8s).await {
                log::debug!("K8s refresh skipped or failed: {e}");
            }
        }
    });

    if let Ok(url) = std::env::var("FINOPS_INGEST_URL") {
        log::info!("Ingest: POST batches to {url}");
        log::info!("Phase 3: start API first — make compose-up (other terminal)");
    } else {
        log::info!("Ingest: stdout (set FINOPS_INGEST_URL for HTTP ingest)");
    }
    log::info!(
        "Agent ready (window={window_secs}s, sample={sample_secs}s, node={node})"
    );
    println!(
        r#"{{"status":"ready","probe":"sched:sched_process_exec","schema_version":{}}}"#,
        output::SCHEMA_VERSION
    );

    loop {
        tokio::select! {
            guard_result = async_fd.readable_mut() => {
                let mut guard = guard_result?;
                let rb = guard.get_inner_mut();
                while let Some(item) = rb.next() {
                    if item.len() < size_of::<FinopsEvent>() {
                        log::warn!("Undersized event ({} bytes), skipping", item.len());
                        continue;
                    }
                    let event: &FinopsEvent =
                        unsafe { &*(item.as_ptr() as *const FinopsEvent) };
                    if raw_events {
                        output::emit_raw(event);
                    }
                    if let Some(batch) = agg.on_finops_event(event, &cache, &node) {
                        output::emit_batch(&batch);
                    }
                }
                guard.clear_ready();
            }

            _ = flush_interval.tick() => {
                if let Some(batch) = agg.flush(&node, &cache) {
                    output::emit_batch(&batch);
                }
            }

            _ = sample_interval.tick() => {
                for batch in
                    memory_sampler::sample_tracked_cgroups(&cache, &mut agg, &node).await
                {
                    output::emit_batch(&batch);
                }
            }

            _ = signal::ctrl_c() => {
                log::info!("Ctrl+C — flushing partial window");
                if let Some(batch) = agg.flush(&node, &cache) {
                    output::emit_batch(&batch);
                }
                println!(r#"{{"status":"shutdown"}}"#);
                break;
            }
        }
    }

    Ok(())
}

fn read_ebpf_path() -> anyhow::Result<String> {
    std::env::var("FINOPS_EBF_PATH").map_err(|_| {
        anyhow::anyhow!(
            "FINOPS_EBF_PATH is not set. Build eBPF first, then run via make run."
        )
    })
}

fn read_window_secs() -> anyhow::Result<u64> {
    match std::env::var("FINOPS_WINDOW_SECS") {
        Ok(s) => Ok(s.parse::<u64>()?.max(1)),
        Err(_) => Ok(10),
    }
}

fn read_sample_interval_secs() -> anyhow::Result<u64> {
    match std::env::var("FINOPS_SAMPLE_INTERVAL_SECS") {
        Ok(s) => Ok(s.parse::<u64>()?.max(1)),
        Err(_) => Ok(10),
    }
}

fn read_node_name() -> String {
    std::env::var("FINOPS_NODE_NAME")
        .or_else(|_| std::env::var("NODE_NAME"))
        .unwrap_or_else(|_| {
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "localhost".into())
        })
}

fn check_privileges() -> anyhow::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        anyhow::bail!(
            "Must run as root or with CAP_BPF + CAP_PERFMON (+ CAP_SYS_ADMIN for cgroup reads)."
        );
    }
    Ok(())
}

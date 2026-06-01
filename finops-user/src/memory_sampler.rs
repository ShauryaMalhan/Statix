//! Periodic cgroup v2 `memory.current` sampling for tracked workloads.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use finops_common::EVENT_KIND_MEMORY_SAMPLE;

use crate::aggregator::{Aggregator, BatchPayload};
use crate::attribution::AttributionCache;

/// Sample all tracked cgroups. May return multiple batches if early flush fires repeatedly.
///
/// Async so cgroupfs reads run on Tokio's blocking pool instead of freezing the runtime
/// worker that also drains the eBPF ring buffer.
pub async fn sample_tracked_cgroups(
    cache: &AttributionCache,
    aggregator: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let sample_tick_ns = now_ns();
    let mut early_batches = Vec::new();

    // Snapshot paths first — cannot `.await` inside `for_each_memory_current_path`.
    let mut targets: Vec<(u64, Arc<PathBuf>)> = Vec::new();
    cache.for_each_memory_current_path(|cgroup_id, path| {
        targets.push((cgroup_id, path));
    });

    for (cgroup_id, path) in targets {
        let memory_bytes = match read_memory_current_at_async(Arc::clone(&path)).await {
            Ok(v) => v,
            Err(e) => {
                log::debug!("memory.current read failed for {path:?}: {e}");
                continue;
            }
        };

        if let Some(batch) = aggregator.ingest_memory_sample(
            EVENT_KIND_MEMORY_SAMPLE,
            cgroup_id,
            memory_bytes,
            sample_tick_ns,
            0,
            cache,
            node,
        ) {
            early_batches.push(batch);
        }
    }

    early_batches
}

/// Offload sync cgroupfs read to the blocking thread pool; keep stack `[u8; 32]` parse (no `read_to_string`).
async fn read_memory_current_at_async(path: Arc<PathBuf>) -> anyhow::Result<u64> {
    tokio::task::spawn_blocking(move || read_memory_current_at(path.as_path())).await?
}

fn read_memory_current_at(path: &Path) -> anyhow::Result<u64> {
    let mut file = File::open(path)?;

    let mut buf = [0u8; 32];
    let n = file.read(&mut buf)?;
    if n == 0 {
        anyhow::bail!("empty memory.current");
    }

    let raw_str = std::str::from_utf8(&buf[..n])?.trim();
    raw_str.parse::<u64>().map_err(Into::into)
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

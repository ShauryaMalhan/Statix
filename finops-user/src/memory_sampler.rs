//! Periodic cgroup v2 `memory.current` sampling for tracked workloads.

use std::{fs::File, io::Read, path::Path};

use finops_common::EVENT_KIND_MEMORY_SAMPLE;

use crate::aggregator::{Aggregator, BatchPayload};
use crate::attribution::AttributionCache;

/// Sample all tracked cgroups. May return multiple batches if early flush fires repeatedly.
pub fn sample_tracked_cgroups(
    cache: &AttributionCache,
    aggregator: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let sample_tick_ns = now_ns();
    let mut early_batches = Vec::new();

    cache.for_each_memory_current_path(|cgroup_id, memory_current_path| {
        let memory_bytes = match read_memory_current_at(memory_current_path) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("memory.current read failed for {memory_current_path:?}: {e}");
                return;
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
    });

    early_batches
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

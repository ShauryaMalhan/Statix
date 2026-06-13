//! Periodic cgroup v2 `memory.current` sampling for tracked workloads.

use std::path::PathBuf;
use std::sync::Arc;

use statix_common::EVENT_KIND_MEMORY_SAMPLE;

use crate::aggregator::{Aggregator, BatchPayload};
use crate::attribution::{read_memory_current_at, AttributionCache};

/// Sample all tracked cgroups. May return multiple batches if early flush fires repeatedly.
///
/// One `spawn_blocking` per tick reads all cgroup paths (not one task per cgroup).
pub async fn sample_tracked_cgroups(
    cache: &AttributionCache,
    aggregator: &mut Aggregator,
    node: &str,
) -> Vec<BatchPayload> {
    let sample_tick_ns = now_ns();

    let mut targets: Vec<(u64, Arc<PathBuf>)> = Vec::new();
    cache.for_each_memory_current_path(|cgroup_id, path| {
        targets.push((cgroup_id, path));
    });

    let readings = match tokio::task::spawn_blocking(move || {
        let mut results = Vec::with_capacity(targets.len());
        for (cgroup_id, path) in targets {
            match read_memory_current_at(path.as_path()) {
                Ok(v) => results.push((cgroup_id, v)),
                Err(e) => log::debug!("memory.current read failed for {path:?}: {e}"),
            }
        }
        results
    })
    .await
    {
        Ok(results) => results,
        Err(e) => {
            log::error!("Memory sampler blocking task failed: {e}");
            metrics::counter!("statix_memory_sampler_errors_total").increment(1);
            Vec::new()
        }
    };

    let mut early_batches = Vec::new();
    for (cgroup_id, memory_bytes) in readings {
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

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

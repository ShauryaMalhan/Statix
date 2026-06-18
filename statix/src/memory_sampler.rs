//! Periodic cgroup v2 sampling: `memory.current` (gauge) and `cpu.stat` (counter delta).

use std::path::PathBuf;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use statix_common::EVENT_KIND_MEMORY_SAMPLE;

use crate::aggregator::{Aggregator, BatchPayload};
use crate::attribution::{read_cpu_usage_usec_at, read_memory_current_at, AttributionCache};

pub struct Sampler {
    cpu_baseline: FxHashMap<u64, u64>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            cpu_baseline: FxHashMap::default(),
        }
    }

    pub async fn tick(
        &mut self,
        cache: &AttributionCache,
        aggregator: &mut Aggregator,
        node: &str,
    ) -> Vec<BatchPayload> {
        let sample_tick_ns = now_ns();

        let mut targets: Vec<(u64, Arc<PathBuf>, Arc<PathBuf>)> = Vec::new();
        cache.for_each_sample_target(|cgroup_id, mem_path, cpu_path| {
            targets.push((cgroup_id, mem_path, cpu_path));
        });

        let live: FxHashSet<u64> = targets.iter().map(|(id, _, _)| *id).collect();
        self.cpu_baseline.retain(|id, _| live.contains(id));

        let readings = match tokio::task::spawn_blocking(move || {
            let mut results = Vec::with_capacity(targets.len());
            for (cgroup_id, mem_path, cpu_path) in targets {
                let memory_bytes = match read_memory_current_at(mem_path.as_path()) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        log::debug!("memory.current read failed for {mem_path:?}: {e}");
                        None
                    }
                };
                let usage_usec = match read_cpu_usage_usec_at(cpu_path.as_path()) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        log::debug!("cpu.stat read failed for {cpu_path:?}: {e}");
                        metrics::counter!("statix_cpu_sampler_errors_total").increment(1);
                        None
                    }
                };
                results.push((cgroup_id, memory_bytes, usage_usec));
            }
            results
        })
        .await
        {
            Ok(results) => results,
            Err(e) => {
                log::error!("Sampler blocking task failed: {e}");
                metrics::counter!("statix_memory_sampler_errors_total").increment(1);
                Vec::new()
            }
        };

        let mut early_batches = Vec::new();
        for (cgroup_id, memory_bytes, usage_usec) in readings {
            if let Some(memory_bytes) = memory_bytes {
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

            if let Some(current) = usage_usec {
                match self.cpu_baseline.get(&cgroup_id) {
                    Some(&last) => {
                        let delta = current.saturating_sub(last);
                        self.cpu_baseline.insert(cgroup_id, current);
                        if let Some(batch) =
                            aggregator.ingest_cpu_sample(cgroup_id, delta, cache, node)
                        {
                            early_batches.push(batch);
                        }
                    }
                    None => {
                        self.cpu_baseline.insert(cgroup_id, current);
                    }
                }
            }
        }

        early_batches
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

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
                if let Some(delta) = cpu_delta(&mut self.cpu_baseline, cgroup_id, current) {
                    if let Some(batch) =
                        aggregator.ingest_cpu_sample(cgroup_id, delta, cache, node)
                    {
                        early_batches.push(batch);
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

/// Returns delta to ingest, or `None` when priming (baseline only, delta 0).
fn cpu_delta(baseline: &mut FxHashMap<u64, u64>, cgroup_id: u64, current: u64) -> Option<u64> {
    match baseline.get(&cgroup_id) {
        Some(&last) => {
            let delta = current.saturating_sub(last);
            baseline.insert(cgroup_id, current);
            Some(delta)
        }
        None => {
            baseline.insert(cgroup_id, current);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase14_priming_first_read_emits_no_delta() {
        let mut baseline = FxHashMap::default();
        let lifetime_usec = 5_000_000_000u64;
        assert!(cpu_delta(&mut baseline, 42, lifetime_usec).is_none());
        assert_eq!(baseline.get(&42), Some(&lifetime_usec));
    }

    #[test]
    fn phase14_second_read_emits_small_delta() {
        let mut baseline = FxHashMap::default();
        let lifetime_usec = 5_000_000_000u64;
        assert!(cpu_delta(&mut baseline, 42, lifetime_usec).is_none());
        let delta = cpu_delta(&mut baseline, 42, lifetime_usec + 80_000).unwrap();
        assert_eq!(delta, 80_000);
        assert!(delta < 1_000_000, "first billable window must not include lifetime CPU");
    }

    #[test]
    fn phase14_conservation_deltas_sum_to_span() {
        let mut baseline = FxHashMap::default();
        let readings = [1_000u64, 1_050, 1_120, 1_200];
        let mut total_delta = 0u64;
        for &current in &readings {
            if let Some(delta) = cpu_delta(&mut baseline, 1, current) {
                total_delta += delta;
            }
        }
        let span = readings.last().unwrap() - readings.first().unwrap();
        assert_eq!(total_delta, span);
    }

    #[test]
    fn phase14_saturating_sub_guards_regression() {
        let mut baseline = FxHashMap::default();
        assert!(cpu_delta(&mut baseline, 1, 100).is_none());
        assert_eq!(cpu_delta(&mut baseline, 1, 50).unwrap(), 0);
    }
}

//! Time-windowed rollups per cgroup / workload identity.
//!
//! - `FxHashMap`: fast `u64` keys (no SipHash DoS resistance needed).
//! - Double-buffered maps: ping-pong + `.clear()` preserves capacity (no realloc per window).
//! - Early flush at `max_keys`: never drop telemetry (FinOps correctness).

use finops_common::{FinopsEvent, EVENT_KIND_WORKLOAD_IDENTITY};
use rustc_hash::FxHashMap;

use crate::attribution::{AttributionCache, WorkloadLabels};
use crate::output::WorkloadBatchRow;

const DEFAULT_MAX_KEYS: usize = 4096;

#[derive(Clone, Debug, Default)]
struct WorkloadStats {
    exec_count: u32,
    sample_count: u32,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    labels: WorkloadLabels,
}

#[derive(Debug)]
pub struct Aggregator {
    window_start_ns: u64,
    buffers: [FxHashMap<u64, WorkloadStats>; 2],
    active: usize,
    max_keys: usize,
}

impl Aggregator {
    pub fn new(_window_secs: u64) -> Self {
        Self {
            window_start_ns: now_unix_ns(),
            buffers: [
                FxHashMap::with_capacity_and_hasher(DEFAULT_MAX_KEYS, Default::default()),
                FxHashMap::with_capacity_and_hasher(DEFAULT_MAX_KEYS, Default::default()),
            ],
            active: 0,
            max_keys: DEFAULT_MAX_KEYS,
        }
    }

    /// Returns an early flush payload if `max_keys` was reached (no data dropped).
    pub fn on_finops_event(
        &mut self,
        event: &FinopsEvent,
        cache: &AttributionCache,
        node: &str,
    ) -> Option<BatchPayload> {
        match event.kind {
            EVENT_KIND_WORKLOAD_IDENTITY => {
                cache.on_identity_event(event);
                let labels = cache.labels_for_cgroup(event.cgroup_id);
                let entry = self.active_mut().entry(event.cgroup_id).or_default();
                entry.exec_count = entry.exec_count.saturating_add(1);
                entry.labels = labels;
            }
            k if k == finops_common::EVENT_KIND_MEMORY_SAMPLE => {
                self.ingest_memory_sample_inner(
                    k,
                    event.cgroup_id,
                    event.memory_bytes,
                    cache,
                );
            }
            _ => log::warn!("Unknown event kind {}", event.kind),
        }
        self.try_early_flush(node, cache)
    }

    pub fn ingest_memory_sample(
        &mut self,
        _kind: u8,
        cgroup_id: u64,
        memory_bytes: u64,
        _timestamp: u64,
        _cpu_id: u32,
        cache: &AttributionCache,
        node: &str,
    ) -> Option<BatchPayload> {
        self.ingest_memory_sample_inner(
            finops_common::EVENT_KIND_MEMORY_SAMPLE,
            cgroup_id,
            memory_bytes,
            cache,
        );
        self.try_early_flush(node, cache)
    }

    fn ingest_memory_sample_inner(
        &mut self,
        _kind: u8,
        cgroup_id: u64,
        memory_bytes: u64,
        cache: &AttributionCache,
    ) {
        let entry = self.active_mut().entry(cgroup_id).or_default();
        entry.sample_count = entry.sample_count.saturating_add(1);
        entry.memory_bytes_last = memory_bytes;
        if memory_bytes > entry.memory_bytes_max {
            entry.memory_bytes_max = memory_bytes;
        }
        entry.labels = cache.labels_for_cgroup(cgroup_id);
    }

    /// Flush when the active buffer hits `max_keys` (e.g. exec storm), not by deleting keys.
    pub fn try_early_flush(&mut self, node: &str, cache: &AttributionCache) -> Option<BatchPayload> {
        if self.active_len() >= self.max_keys {
            log::info!(
                "Early flush: active buffer reached max_keys ({})",
                self.max_keys
            );
            self.flush(node, cache)
        } else {
            None
        }
    }

    pub fn flush(&mut self, node: &str, cache: &AttributionCache) -> Option<BatchPayload> {
        let flush_idx = self.active;
        if self.buffers[flush_idx].is_empty() {
            self.reset_window();
            return None;
        }

        let window_start_ns = self.window_start_ns;
        let window_end_ns = now_unix_ns();

        // Flip first so ingest paths use a fresh buffer while we drain the old one.
        self.active = 1 - self.active;
        self.reset_window();

        let workloads: Vec<WorkloadBatchRow> = self.buffers[flush_idx]
            .iter()
            .map(|(cgroup_id, s)| {
                let labels = cache.labels_for_cgroup(*cgroup_id);
                WorkloadBatchRow {
                    cgroup_id: *cgroup_id,
                    namespace: labels.namespace.or_else(|| s.labels.namespace.clone()),
                    pod: labels.pod.or_else(|| s.labels.pod.clone()),
                    container: labels.container.or_else(|| s.labels.container.clone()),
                    k8s_resolved: labels.k8s_resolved || s.labels.k8s_resolved,
                    memory_bytes_max: s.memory_bytes_max,
                    memory_bytes_last: s.memory_bytes_last,
                    exec_count: s.exec_count,
                    sample_count: s.sample_count,
                }
            })
            .collect();

        self.buffers[flush_idx].clear();

        Some(BatchPayload {
            window_start_ns,
            window_end_ns,
            node: node.to_string(),
            workloads,
        })
    }

    fn active_mut(&mut self) -> &mut FxHashMap<u64, WorkloadStats> {
        &mut self.buffers[self.active]
    }

    fn active_len(&self) -> usize {
        self.buffers[self.active].len()
    }

    fn reset_window(&mut self) {
        self.window_start_ns = now_unix_ns();
    }
}

pub struct BatchPayload {
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub workloads: Vec<WorkloadBatchRow>,
}

fn now_unix_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

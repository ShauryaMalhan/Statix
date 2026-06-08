//! Time-windowed rollups per cgroup / workload identity.
//!
//! - `FxHashMap`: fast `u64` keys (no SipHash DoS resistance needed).
//! - Double-buffered maps: ping-pong + `.clear()` preserves capacity (no realloc per window).
//! - Early flush at `max_keys`: never drop telemetry (FinOps correctness).
//! - Atomic `clock_offset_ns` (statix-infra): maps BPF monotonic timestamps to wall-clock.

use std::cell::RefCell;
use std::sync::Arc;
use statix_common::{StatixEvent, EVENT_KIND_WORKLOAD_IDENTITY};
use statix_infra::clock::{clock_offset_ns, mono_now_ns};
use rand::rngs::SmallRng;
use rand::{RngCore, SeedableRng};
use rustc_hash::FxHashMap;

use crate::attribution::{AttributionCache, WorkloadLabels, DEFAULT_LABELS};
use statix_wire::WorkloadRow;

thread_local! {
    static TL_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_entropy());
}

fn fast_batch_id() -> String {
    let mut bytes = [0u8; 16];
    TL_RNG.with(|rng| rng.borrow_mut().fill_bytes(&mut bytes));
    // UUID v4 variant bits (non-crypto correlation id — no getrandom on hot path).
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

const DEFAULT_MAX_KEYS: usize = 4096;

#[derive(Clone, Debug)]
struct WorkloadStats {
    exec_count: u32,
    sample_count: u32,
    memory_bytes_max: u64,
    memory_bytes_last: u64,
    labels: Arc<WorkloadLabels>,
}

impl Default for WorkloadStats {
    fn default() -> Self {
        Self {
            exec_count: 0,
            sample_count: 0,
            memory_bytes_max: 0,
            memory_bytes_last: 0,
            labels: Arc::clone(&DEFAULT_LABELS),
        }
    }
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
        let window_start_ns = mono_now_ns().saturating_add(clock_offset_ns());

        Self {
            window_start_ns,
            buffers: [
                FxHashMap::with_capacity_and_hasher(DEFAULT_MAX_KEYS, Default::default()),
                FxHashMap::with_capacity_and_hasher(DEFAULT_MAX_KEYS, Default::default()),
            ],
            active: 0,
            max_keys: DEFAULT_MAX_KEYS,
        }
    }

    /// Monotonic ns → wall ns via lock-free atomic offset (refreshed in background).
    #[inline]
    fn mono_to_wall(&self, mono_ns: u64) -> u64 {
        mono_ns.saturating_add(clock_offset_ns())
    }

    /// Current wall time in the same domain as converted BPF event timestamps.
    fn wall_now_ns(&self) -> u64 {
        self.mono_to_wall(mono_now_ns())
    }

    /// Returns an early flush payload if `max_keys` was reached (no data dropped).
    pub fn on_statix_event(
        &mut self,
        event: &StatixEvent,
        cache: &AttributionCache,
        node: &str,
    ) -> Option<BatchPayload> {
        let wall_timestamp = self.mono_to_wall(event.timestamp);

        match event.kind {
            EVENT_KIND_WORKLOAD_IDENTITY => {
                cache.on_identity_event(event);
                let entry = self.active_mut().entry(event.cgroup_id).or_default();
                entry.exec_count = entry.exec_count.saturating_add(1);
                entry.labels = cache.labels_for_cgroup(event.cgroup_id);
            }
            k if k == statix_common::EVENT_KIND_MEMORY_SAMPLE => {
                self.ingest_memory_sample_inner(
                    k,
                    event.cgroup_id,
                    event.memory_bytes,
                    cache,
                );
            }
            _ => log::warn!("Unknown event kind {}", event.kind),
        }

        if wall_timestamp > 0 {
            log::trace!(
                "event kind={} cgroup_id={} wall_timestamp_ns={wall_timestamp}",
                event.kind,
                event.cgroup_id
            );
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
            statix_common::EVENT_KIND_MEMORY_SAMPLE,
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

    pub fn flush(&mut self, node: &str, _cache: &AttributionCache) -> Option<BatchPayload> {
        let flush_idx = self.active;
        if self.buffers[flush_idx].is_empty() {
            self.reset_window();
            return None;
        }

        let window_start_ns = self.window_start_ns;
        let window_end_ns = self.wall_now_ns();

        // Flip first so ingest paths use a fresh buffer while we drain the old one.
        self.active = 1 - self.active;
        self.reset_window();

        let workloads: Vec<WorkloadRow> = self.buffers[flush_idx]
            .iter()
            .map(|(cgroup_id, s)| WorkloadRow {
                cgroup_id: *cgroup_id,
                namespace: s.labels.namespace.clone(),
                pod: s.labels.pod.clone(),
                container: s.labels.container.clone(),
                k8s_resolved: s.labels.k8s_resolved,
                memory_bytes_max: s.memory_bytes_max,
                memory_bytes_last: s.memory_bytes_last,
                exec_count: s.exec_count,
                sample_count: s.sample_count,
            })
            .collect();

        self.buffers[flush_idx].clear();

        let batch_id = fast_batch_id();

        Some(BatchPayload {
            window_start_ns,
            window_end_ns,
            node: node.to_string(),
            batch_id,
            agent_version: env!("CARGO_PKG_VERSION"),
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
        self.window_start_ns = self.wall_now_ns();
    }
}

pub struct BatchPayload {
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub batch_id: String,
    pub agent_version: &'static str,
    pub workloads: Vec<WorkloadRow>,
}

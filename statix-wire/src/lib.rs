//! Shared HTTP ingest wire types for statix ↔ statix-gateway.

use serde::{Deserialize, Serialize};

/// POST `/ingest` batch envelope (agent → gateway).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestBatch {
    pub schema_version: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub batch_id: String,
    pub agent_version: String,
    pub workloads: Vec<WorkloadRow>,
}

/// One workload rollup inside a batch window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadRow {
    pub cgroup_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    pub k8s_resolved: bool,
    pub memory_bytes_max: u64,
    pub memory_bytes_last: u64,
    pub exec_count: u32,
    pub sample_count: u32,
}

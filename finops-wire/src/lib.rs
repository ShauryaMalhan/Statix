//! Shared HTTP ingest and Kafka JSONEachRow types for finops-agent ↔ finops-gateway.

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

/// Denormalized row for Kafka / ClickHouse `JSONEachRow` (one message per workload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatRow {
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: String,
    pub batch_id: String,
    pub agent_version: String,
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

impl FlatRow {
    /// Build one Kafka/ClickHouse row from a batch envelope and a workload line.
    pub fn from_ingest(batch: &IngestBatch, row: &WorkloadRow) -> Self {
        Self {
            window_start_ns: batch.window_start_ns,
            window_end_ns: batch.window_end_ns,
            node: batch.node.clone(),
            batch_id: batch.batch_id.clone(),
            agent_version: batch.agent_version.clone(),
            cgroup_id: row.cgroup_id,
            namespace: row.namespace.clone(),
            pod: row.pod.clone(),
            container: row.container.clone(),
            k8s_resolved: row.k8s_resolved,
            memory_bytes_max: row.memory_bytes_max,
            memory_bytes_last: row.memory_bytes_last,
            exec_count: row.exec_count,
            sample_count: row.sample_count,
        }
    }
}

impl IngestBatch {
    pub fn into_flat_rows(self) -> Vec<FlatRow> {
        let window_start_ns = self.window_start_ns;
        let window_end_ns = self.window_end_ns;
        let node = self.node;
        let batch_id = self.batch_id;
        let agent_version = self.agent_version;
        self.workloads
            .into_iter()
            .map(|row| FlatRow {
                window_start_ns,
                window_end_ns,
                node: node.clone(),
                batch_id: batch_id.clone(),
                agent_version: agent_version.clone(),
                cgroup_id: row.cgroup_id,
                namespace: row.namespace,
                pod: row.pod,
                container: row.container,
                k8s_resolved: row.k8s_resolved,
                memory_bytes_max: row.memory_bytes_max,
                memory_bytes_last: row.memory_bytes_last,
                exec_count: row.exec_count,
                sample_count: row.sample_count,
            })
            .collect()
    }
}

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
    /// CPU microseconds consumed during this window (delta of cgroup cpu.stat usage_usec).
    #[serde(default)]
    pub cpu_usage_usec: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_row_v2_missing_cpu_defaults_to_zero() {
        let json = r#"{
            "cgroup_id": 1,
            "k8s_resolved": false,
            "memory_bytes_max": 0,
            "memory_bytes_last": 0,
            "exec_count": 0,
            "sample_count": 0
        }"#;
        let row: WorkloadRow = serde_json::from_str(json).unwrap();
        assert_eq!(row.cpu_usage_usec, 0);
    }

    #[test]
    fn workload_row_v3_round_trip() {
        let row = WorkloadRow {
            cgroup_id: 42,
            namespace: None,
            pod: None,
            container: None,
            k8s_resolved: false,
            memory_bytes_max: 100,
            memory_bytes_last: 50,
            exec_count: 1,
            sample_count: 2,
            cpu_usage_usec: 12345,
        };
        let json = serde_json::to_string(&row).unwrap();
        let parsed: WorkloadRow = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cpu_usage_usec, 12345);
    }
}

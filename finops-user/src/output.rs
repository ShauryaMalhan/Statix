//! Batched JSON (schema v2) and optional raw per-event debug output.

use finops_common::FinopsEvent;
use serde::Serialize;

use crate::aggregator::BatchPayload;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize)]
pub struct BatchJson<'a> {
    pub schema_version: u32,
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub node: &'a str,
    pub workloads: &'a [WorkloadBatchRow],
}

#[derive(Clone, Serialize)]
pub struct WorkloadBatchRow {
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

pub fn emit_batch(payload: &BatchPayload) {
    let batch = BatchJson {
        schema_version: SCHEMA_VERSION,
        window_start_ns: payload.window_start_ns,
        window_end_ns: payload.window_end_ns,
        node: &payload.node,
        workloads: &payload.workloads,
    };
    match serde_json::to_string(&batch) {
        Ok(json) => println!("{json}"),
        Err(e) => log::error!("batch JSON serialisation failed: {e}"),
    }
}

#[derive(Serialize)]
struct RawEventJson<'a> {
    kind: u8,
    pid: u32,
    tgid: u32,
    cpu_id: u32,
    cgroup_id: u64,
    timestamp_ns: u64,
    memory_bytes: u64,
    comm: &'a str,
}

pub fn emit_raw(event: &FinopsEvent) {
    let comm = comm_to_str(&event.comm);
    let ev = RawEventJson {
        kind: event.kind,
        pid: event.pid,
        tgid: event.tgid,
        cpu_id: event.cpu_id,
        cgroup_id: event.cgroup_id,
        timestamp_ns: event.timestamp,
        memory_bytes: event.memory_bytes,
        comm,
    };
    if let Ok(json) = serde_json::to_string(&ev) {
        println!("{json}");
    }
}

fn comm_to_str(comm: &[u8; 16]) -> &str {
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);
    std::str::from_utf8(&comm[..end]).unwrap_or("<invalid-utf8>")
}

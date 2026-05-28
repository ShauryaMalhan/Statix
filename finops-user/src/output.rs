//! output.rs — Event serialisation and stdout emission.
//!
//! Responsibility: convert a raw `ProcessEvent` (kernel bytes) into a
//! human-readable JSON line on stdout.
//!
//! Phase 1: stdout JSON.
//! Phase 3: replace `emit` with a gRPC send to the ingestion service.
//!          The `ProcessEvent` type and the field mapping stay identical.

use finops_common::ProcessEvent;
use serde::Serialize;

/// Wire format for one event emitted on stdout.
///
/// Separate from `ProcessEvent` so that:
///   - `ProcessEvent` stays in `finops-common` (no_std, no serde dependency).
///   - We control JSON field names independently of the kernel struct.
///   - We convert `comm: [u8; 16]` to `comm: &str` here, not in the kernel struct.
#[derive(Serialize)]
struct EventJson<'a> {
    pid:          u32,
    tgid:         u32,
    cpu_id:       u32,
    timestamp_ns: u64,
    comm:         &'a str,
}

/// Serialise a `ProcessEvent` to a compact JSON line and print to stdout.
pub fn emit(event: &ProcessEvent) {
    let comm_str = comm_to_str(&event.comm);

    let ev = EventJson {
        pid:          event.pid,
        tgid:         event.tgid,
        cpu_id:       event.cpu_id,
        timestamp_ns: event.timestamp,
        comm:         comm_str,
    };

    match serde_json::to_string(&ev) {
        Ok(json) => println!("{json}"),
        Err(e)   => log::error!("JSON serialisation failed: {e}"),
    }
}

/// Extract a UTF-8 process name from the kernel's fixed-size comm array.
///
/// The kernel null-terminates at TASK_COMM_LEN (16 bytes). We find the first
/// null byte and slice up to it. Falls back to a placeholder on invalid UTF-8
/// (rare in practice — process names are ASCII).
fn comm_to_str(comm: &[u8; 16]) -> &str {
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);
    std::str::from_utf8(&comm[..end]).unwrap_or("<invalid-utf8>")
}

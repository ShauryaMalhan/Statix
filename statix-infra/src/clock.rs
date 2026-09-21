//! Wall-clock helper.
//!
//! Window bounds are read directly from the system clock. A cached
//! monotonic→wall offset used to live here; it went stale whenever the host
//! paused (laptop sleep, hypervisor pause, live migration), stamping rows
//! minutes or hours in the past. See ADR 063.

use std::time::{SystemTime, UNIX_EPOCH};

/// Unix epoch wall time in nanoseconds.
pub fn wall_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
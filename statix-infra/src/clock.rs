//! Wall and monotonic clock helpers (BPF `bpf_ktime_get_ns` domain alignment).

use std::time::{SystemTime, UNIX_EPOCH};

/// `CLOCK_MONOTONIC` nanoseconds since boot (BPF timestamp domain).
pub fn mono_now_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `timespec` is valid; CLOCK_MONOTONIC is always available on Linux.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    if rc != 0 {
        return 0;
    }
    (t.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(t.tv_nsec as u64)
}

/// Unix epoch wall time in nanoseconds.
pub fn wall_unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Offset between wall clock and `CLOCK_MONOTONIC` at agent start (BPF domain).
pub fn calibrate_clock_offset_ns() -> u64 {
    let mono = mono_now_ns();
    let wall = wall_unix_ns();
    wall.saturating_sub(mono)
}

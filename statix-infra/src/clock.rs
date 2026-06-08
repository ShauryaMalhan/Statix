//! Wall and monotonic clock helpers (BPF `bpf_ktime_get_ns` domain alignment).
//!
//! Hot path: [`clock_offset_ns`] — single `AtomicU64` load (`Ordering::Relaxed`).
//! Background: [`recalibrate_clock_offset`] — periodic NTP drift correction (not on ring-buffer path).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static CLOCK_OFFSET_NS: AtomicU64 = AtomicU64::new(0);

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

/// One-shot calibration: `wall_unix - CLOCK_MONOTONIC`.
pub fn calibrate_clock_offset_ns() -> u64 {
    let mono = mono_now_ns();
    let wall = wall_unix_ns();
    wall.saturating_sub(mono)
}

/// Initialize the global offset (call once at agent startup before the hot path runs).
pub fn init_clock_offset() -> u64 {
    let offset = calibrate_clock_offset_ns();
    CLOCK_OFFSET_NS.store(offset, Ordering::Relaxed);
    offset
}

/// Hot-path read: BPF/monotonic ns + this offset → wall Unix ns.
#[inline]
pub fn clock_offset_ns() -> u64 {
    CLOCK_OFFSET_NS.load(Ordering::Relaxed)
}

/// Background recalibration after NTP steps or VM clock catch-up. Not for ring-buffer drain.
pub fn recalibrate_clock_offset() -> u64 {
    let offset = calibrate_clock_offset_ns();
    let prev = CLOCK_OFFSET_NS.swap(offset, Ordering::Relaxed);
    if prev != offset {
        log::info!("Clock domain offset recalibrated: {prev} → {offset} ns");
    }
    offset
}

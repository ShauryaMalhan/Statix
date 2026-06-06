//! BPF map load prerequisites for pre-5.11 kernels.
//!
//! Before Linux 5.11, BPF map memory counts against `RLIMIT_MEMLOCK` (often 64 KiB).
//! Our 512 KiB `EVENTS` ring buffer exceeds that on kernel 5.10 without a bump.

use std::io;

use libc::{rlimit, setrlimit, RLIMIT_MEMLOCK, RLIM_INFINITY};

/// Raise `RLIMIT_MEMLOCK` to infinity so large ring-buffer maps can be created on 5.10.
pub fn bump_memlock_rlimit() -> io::Result<()> {
    let lim = rlimit {
        rlim_cur: RLIM_INFINITY,
        rlim_max: RLIM_INFINITY,
    };
    if unsafe { setrlimit(RLIMIT_MEMLOCK, &lim) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

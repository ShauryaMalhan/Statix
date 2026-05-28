// no_std means: do not link the Rust standard library.
// This is mandatory for code that runs in kernel space, where there is no
// operating system underneath to provide malloc, file I/O, threads, etc.
// finops-ebpf imports this crate, so it must be no_std compatible.
#![no_std]

/// A single process-execution event captured by the eBPF kernel probe.
///
/// This struct is the ONLY thing that crosses the kernel/user-space boundary.
/// It is written by the eBPF program (kernel side) and read by the Rust agent
/// (user side). Both sides import this exact definition.
///
/// Memory layout lesson:
///   #[repr(C)] locks the layout to match C struct rules:
///     - Fields are placed in declaration order
///     - Each field is aligned to its own size (u32 → 4-byte boundary, u64 → 8-byte)
///     - The compiler cannot reorder or remove padding silently
///
///   Without #[repr(C)], the Rust compiler might silently place `timestamp` at
///   offset 8 on one build and offset 12 on another. The kernel write would be
///   interpreted as garbage by the user-space reader.
///
/// Byte map of this struct (24 bytes total + 16 for comm = 40 bytes):
///   offset 0  : pid       (4 bytes, u32)
///   offset 4  : tgid      (4 bytes, u32)
///   offset 8  : cpu_id    (4 bytes, u32)
///   offset 12 : _pad      (4 bytes, u32) ← explicit padding to align timestamp
///   offset 16 : timestamp (8 bytes, u64) ← must be 8-byte aligned
///   offset 24 : comm      (16 bytes, [u8; 16])
///   total     : 40 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProcessEvent {
    /// The Process ID. On Linux, PID identifies the specific thread.
    pub pid: u32,

    /// Thread Group ID. For the main thread of a process, TGID == PID.
    /// For spawned threads, all share the same TGID (= the parent's PID).
    /// This is what `ps` and `top` show as "PID".
    pub tgid: u32,

    /// Which CPU core executed this syscall. Useful for per-CPU analysis.
    pub cpu_id: u32,

    /// Explicit 4-byte padding to push `timestamp` to an 8-byte boundary.
    /// The alternative (letting the compiler add implicit padding) is
    /// dangerous: it creates an invisible gap where the two sides might
    /// disagree on layout. Always make padding explicit.
    pub _pad: u32,

    /// Monotonic nanoseconds since boot (from bpf_ktime_get_ns).
    /// "Monotonic" means it never goes backwards — safe for duration math.
    /// Stored as u64 because nanoseconds since boot overflows u32 in ~4 seconds.
    pub timestamp: u64,

    /// Process name from the kernel task struct (comm field).
    /// Maximum 16 bytes including null terminator — this is TASK_COMM_LEN,
    /// a hard kernel constant. Always a fixed-size array, never a pointer,
    /// because pointers across kernel/user boundary are meaningless (different
    /// virtual address spaces).
    pub comm: [u8; 16],
}

// This impl is only compiled when the `user` feature is enabled
// (i.e., when finops-user depends on finops-common with features = ["user"]).
//
// aya::Pod is a marker trait that tells Aya's ring buffer consumer:
// "it is safe to interpret the raw bytes coming out of the kernel ring buffer
//  directly as a &ProcessEvent without any deserialization step."
//
// This is safe ONLY because:
//   1. #[repr(C)] gives us a deterministic byte layout
//   2. ProcessEvent contains only primitive integer types (no pointers, no references)
//   3. All bit patterns are valid values for every field type
#[cfg(feature = "user")]
unsafe impl aya::Pod for ProcessEvent {}

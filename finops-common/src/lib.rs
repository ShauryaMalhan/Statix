#![no_std]

/// Discriminator for [`FinopsEvent`] records in the shared ring buffer.
pub const EVENT_KIND_WORKLOAD_IDENTITY: u8 = 1;
/// Memory sample (kernel or user-space sampler).
pub const EVENT_KIND_MEMORY_SAMPLE: u8 = 2;

/// Unified ring-buffer record (64 bytes). Identity and memory samples share one map.
///
/// Byte map:
///   offset 0  : kind           (1 byte, u8)
///   offset 1  : _pad           (7 bytes)
///   offset 8  : pid            (4 bytes, u32) — identity only
///   offset 12 : tgid           (4 bytes, u32) — identity only
///   offset 16 : cpu_id         (4 bytes, u32)
///   offset 20 : _pad2          (4 bytes, u32)
///   offset 24 : cgroup_id      (8 bytes, u64)
///   offset 32 : timestamp      (8 bytes, u64)
///   offset 40 : memory_bytes   (8 bytes, u64) — memory sample; 0 for identity
///   offset 48 : comm           (16 bytes, [u8; 16]) — identity only
///   total     : 64 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FinopsEvent {
    pub kind: u8,
    pub _pad: [u8; 7],
    pub pid: u32,
    pub tgid: u32,
    pub cpu_id: u32,
    pub _pad2: u32,
    pub cgroup_id: u64,
    pub timestamp: u64,
    pub memory_bytes: u64,
    pub comm: [u8; 16],
}

/// Phase 1 alias — same layout as identity events without `kind` set in old builds.
/// Deprecated: use [`FinopsEvent`] with `EVENT_KIND_WORKLOAD_IDENTITY`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProcessEvent {
    pub pid: u32,
    pub tgid: u32,
    pub cpu_id: u32,
    pub _pad: u32,
    pub timestamp: u64,
    pub comm: [u8; 16],
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for FinopsEvent {}

#[cfg(feature = "user")]
unsafe impl aya::Pod for ProcessEvent {}

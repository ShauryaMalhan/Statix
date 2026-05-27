//! loader.rs — eBPF program lifecycle: load, verify, attach probes.
//!
//! Responsibility: everything that happens before the event loop starts.
//! Returns a fully initialised `Ebpf` handle with the kprobe already attached.

use std::{convert::TryFrom, fs};

use aya::{maps::RingBuf, programs::KProbe, Ebpf};

/// Read the BPF ELF from disk, pass it through the kernel verifier,
/// JIT-compile it, and attach the execve kprobe.
///
/// On success the returned `Ebpf` owns all kernel resources (maps, programs).
/// Dropping it automatically detaches probes and frees kernel memory.
pub fn load_and_attach(ebpf_path: &str) -> anyhow::Result<Ebpf> {
    log::info!("Loading eBPF program from: {ebpf_path}");
    let bytes = fs::read(ebpf_path)?;

    // Ebpf::load() sequence:
    //   1. Parse BPF ELF sections (programs + map definitions)
    //   2. Allocate BPF maps in kernel memory
    //   3. Run the kernel verifier — static proof of memory safety
    //   4. JIT-compile verified bytecode to native x86_64 instructions
    let mut bpf = Ebpf::load(&bytes)?;
    log::info!("eBPF program loaded and kernel-verified");

    attach_execve_probe(&mut bpf)?;
    Ok(bpf)
}

/// Attach the execve kprobe to `__x64_sys_execve`.
///
/// Fires every time any process on this machine calls execve() —
/// i.e., starts a new program. Read-only: zero impact on the traced
/// process's execution path.
fn attach_execve_probe(bpf: &mut Ebpf) -> anyhow::Result<()> {
    let program: &mut KProbe = bpf
        .program_mut("finops_execve")
        .ok_or_else(|| anyhow::anyhow!("BPF program 'finops_execve' not found in ELF"))?
        .try_into()?;

    program.load()?;

    // "__x64_sys_execve" is the x86_64 kernel symbol for the execve syscall.
    // The `0` offset means we attach at function entry (not mid-function).
    // On arm64 this would be "__arm64_sys_execve" — Phase 2 will switch to
    // architecture-neutral tracepoints (sched:sched_process_exec).
    program.attach("__x64_sys_execve", 0)?;
    log::info!("kprobe attached to __x64_sys_execve");
    Ok(())
}

/// Extract a handle to the EVENTS ring buffer map from the loaded BPF object.
///
/// "EVENTS" must match the static name declared in finops-ebpf/src/main.rs:
///   static EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);
///
/// The returned `RingBuf` borrows from `bpf` — both must stay alive together.
pub fn get_ring_buf(bpf: &mut Ebpf) -> anyhow::Result<RingBuf<&mut aya::maps::MapData>> {
    RingBuf::try_from(
        bpf.map_mut("EVENTS")
            .ok_or_else(|| anyhow::anyhow!("Ring buffer map 'EVENTS' not found in BPF object"))?,
    )
    .map_err(Into::into)
}

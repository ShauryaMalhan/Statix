// ============================================================
// finops-ebpf/src/main.rs  —  The Kernel Observer
// ============================================================
#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_ktime_get_ns},
    macros::{kprobe, map},
    maps::RingBuf,
    programs::ProbeContext,
};
// aya-log-ebpf is not used in Phase 1 — no BPF-side logging macros here.
// Add it back in Phase 2 for structured kernel-side event logging via BPF map.
use finops_common::ProcessEvent;

// 256 KB ring buffer — holds ~6,500 ProcessEvent structs (40 bytes each)
// before the oldest are overwritten. Must be a power of 2.
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

#[kprobe]
pub fn finops_execve(ctx: ProbeContext) -> u32 {
    capture_event(&ctx);
    0 // Always return 0 — never disrupt the traced process
}

fn capture_event(_ctx: &ProbeContext) {
    // ── Reserve a ring buffer slot ────────────────────────────
    //
    // Critical verifier rule: every code path that calls bpf_ringbuf_reserve
    // MUST call either bpf_ringbuf_submit OR bpf_ringbuf_discard before
    // the BPF program exits. The verifier tracks this as a "reference" (id=2
    // in the error we saw) and rejects programs that don't release it.
    //
    // This is why we CANNOT use Rust's `?` operator after reserving: `?`
    // would return early (via Err propagation), dropping the entry without
    // calling submit/discard — the verifier sees the unreleased reference
    // and rejects the program with "Unreleased reference id=N".
    //
    // Fix: use explicit match arms and always reach either submit or discard.
    // We also use a raw pointer (*mut ProcessEvent) to write fields — raw
    // pointers don't borrow `entry` in Rust's type system, so we can still
    // call entry.submit() or entry.discard() after writing through the pointer.
    let mut entry = match EVENTS.reserve::<ProcessEvent>(0) {
        Some(e) => e,
        // Buffer full → drop this event. No reference was created, safe exit.
        None => return,
    };

    // Get a raw pointer to the reserved slot.
    // This does NOT borrow `entry` (raw pointers bypass borrow checking),
    // so we can still call entry.submit() later.
    let ptr: *mut ProcessEvent = entry.as_mut_ptr();

    // ── Fill all fields via the raw pointer ───────────────────
    //
    // All field writes go through the raw pointer in a single unsafe block.
    // We write EVERY field before submit — the verifier requires that we
    // don't submit uninitialized memory.
    //
    // For comm: if bpf_get_current_comm() fails (rare — happens if the
    // task_struct is being torn down), we zero-fill and still submit.
    // We'd rather emit a partial event than drop it — the user-space side
    // can detect zero-filled comm and annotate accordingly.
    unsafe {
        let pid_tgid = bpf_get_current_pid_tgid();
        (*ptr).pid       = pid_tgid as u32;          // lower 32 bits = PID (thread)
        (*ptr).tgid      = (pid_tgid >> 32) as u32;  // upper 32 bits = TGID (process)
        (*ptr).cpu_id    = aya_ebpf::helpers::bpf_get_smp_processor_id();
        (*ptr)._pad      = 0;
        (*ptr).timestamp = bpf_ktime_get_ns();

        (*ptr).comm = match bpf_get_current_comm() {
            Ok(comm) => comm,
            Err(_)   => [0u8; 16], // zero-fill: emit event with empty name
        };
    }

    // ── Submit ────────────────────────────────────────────────
    //
    // submit() atomically makes this event visible to the user-space consumer
    // and releases the reference the verifier was tracking.
    // After this call, `entry` is consumed (Rust moves it). No more access.
    entry.submit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

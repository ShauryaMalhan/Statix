#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{
        bpf_get_current_comm, bpf_get_current_cgroup_id, bpf_get_current_pid_tgid,
        bpf_get_smp_processor_id, bpf_ktime_get_ns,
    },
    macros::{map, tracepoint},
    maps::{PerCpuArray, RingBuf},
    programs::TracePointContext,
};
use statix_common::{StatixEvent, EVENT_KIND_WORKLOAD_IDENTITY};

include!(concat!(env!("OUT_DIR"), "/ring_config.rs"));

/// Ring buffer submit flag: suppress userspace wakeup (see `man bpf_ringbuf_submit`).
const BPF_RB_NO_WAKEUP: u64 = 1;

#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(RING_BUF_BYTES, 0);

/// Per-CPU drops when `EVENTS.reserve` fails (key 0 only).
#[map]
static RING_DROPS: PerCpuArray<u64> = PerCpuArray::with_max_entries(1, 0);

#[map]
static WAKEUP_COUNTER: PerCpuArray<u32> = PerCpuArray::with_max_entries(1, 0);

#[tracepoint(name = "sched_process_exec", category = "sched")]
pub fn statix_sched_process_exec(ctx: TracePointContext) -> u32 {
    capture_identity(&ctx);
    0
}

#[inline(always)]
fn record_ring_drop() {
    if let Some(ptr) = RING_DROPS.get_ptr_mut(0) {
        // SAFETY: Per-CPU slot; preemption disabled for the duration of this program.
        unsafe {
            *ptr = (*ptr).wrapping_add(1);
        }
    }
}

fn capture_identity(_ctx: &TracePointContext) {
    let mut entry = match EVENTS.reserve::<StatixEvent>(0) {
        Some(e) => e,
        None => {
            record_ring_drop();
            return;
        }
    };

    let ptr: *mut StatixEvent = entry.as_mut_ptr();

    // SAFETY: Exclusive ring-buffer slot until submit().
    unsafe {
        let pid_tgid = bpf_get_current_pid_tgid();
        (*ptr).kind = EVENT_KIND_WORKLOAD_IDENTITY;
        (*ptr)._pad = [0u8; 7];
        (*ptr).pid = pid_tgid as u32;
        (*ptr).tgid = (pid_tgid >> 32) as u32;
        (*ptr).cpu_id = bpf_get_smp_processor_id();
        (*ptr)._pad2 = 0;
        (*ptr).cgroup_id = bpf_get_current_cgroup_id();
        (*ptr).timestamp = bpf_ktime_get_ns();
        (*ptr).memory_bytes = 0;

        (*ptr).comm = match bpf_get_current_comm() {
            Ok(comm) => comm,
            Err(_) => [0u8; 16],
        };
    }

    let wakeup_flag = match WAKEUP_COUNTER.get_ptr_mut(0) {
        Some(ptr) => unsafe {
            let count = (*ptr).wrapping_add(1);
            *ptr = count;
            if count & 63 == 0 { 0 } else { BPF_RB_NO_WAKEUP }
        },
        None => 0, // fallback: always wake
    };
    entry.submit(wakeup_flag);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

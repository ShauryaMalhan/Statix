#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{
        bpf_get_current_comm, bpf_get_current_cgroup_id, bpf_get_current_pid_tgid,
        bpf_get_smp_processor_id, bpf_ktime_get_ns,
    },
    macros::{map, tracepoint},
    maps::RingBuf,
    programs::TracePointContext,
};
use finops_common::{FinopsEvent, EVENT_KIND_WORKLOAD_IDENTITY};

include!(concat!(env!("OUT_DIR"), "/ring_config.rs"));

#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(RING_BUF_BYTES, 0);

#[tracepoint(name = "sched_process_exec", category = "sched")]
pub fn finops_sched_process_exec(ctx: TracePointContext) -> u32 {
    capture_identity(&ctx);
    0
}

fn capture_identity(_ctx: &TracePointContext) {
    let mut entry = match EVENTS.reserve::<FinopsEvent>(0) {
        Some(e) => e,
        None => return,
    };

    let ptr: *mut FinopsEvent = entry.as_mut_ptr();

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

    entry.submit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

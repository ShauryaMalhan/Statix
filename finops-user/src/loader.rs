//! loader.rs — eBPF program lifecycle: load, verify, attach probes.

use std::{convert::TryFrom, fs};

use aya::{maps::RingBuf, programs::TracePoint, Ebpf};

pub fn load_and_attach(ebpf_path: &str) -> anyhow::Result<Ebpf> {
    log::info!("Loading eBPF program from: {ebpf_path}");
    let bytes = fs::read(ebpf_path)?;

    let mut bpf = Ebpf::load(&bytes)?;
    log::info!("eBPF program loaded and kernel-verified");

    attach_sched_process_exec(&mut bpf)?;
    Ok(bpf)
}

fn attach_sched_process_exec(bpf: &mut Ebpf) -> anyhow::Result<()> {
    let program: &mut TracePoint = bpf
        .program_mut("finops_sched_process_exec")
        .ok_or_else(|| {
            anyhow::anyhow!("BPF program 'finops_sched_process_exec' not found in ELF")
        })?
        .try_into()?;

    program.load()?;
    program.attach("sched", "sched_process_exec")?;
    log::info!("tracepoint attached to sched:sched_process_exec");
    Ok(())
}

pub fn get_events_ring_buf(bpf: &mut Ebpf) -> anyhow::Result<RingBuf<&mut aya::maps::MapData>> {
    RingBuf::try_from(
        bpf.map_mut("EVENTS")
            .ok_or_else(|| anyhow::anyhow!("Ring buffer map 'EVENTS' not found in BPF object"))?,
    )
    .map_err(Into::into)
}

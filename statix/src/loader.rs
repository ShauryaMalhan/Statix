//! loader.rs — eBPF program lifecycle: load, verify, attach probes.

use std::{convert::TryFrom, fs};

use std::time::Duration;

use aya::maps::{MapData, PerCpuArray, RingBuf};
use aya::{programs::TracePoint, Ebpf};

pub fn load_and_attach(ebpf_path: &str) -> anyhow::Result<Ebpf> {
    log::info!("Loading eBPF program from: {ebpf_path}");
    crate::bpf_memlock::bump_memlock_rlimit()
        .map_err(|e| anyhow::anyhow!("failed to raise RLIMIT_MEMLOCK for BPF maps: {e}"))?;
    let bytes = fs::read(ebpf_path)?;

    let mut bpf = Ebpf::load(&bytes)?;
    log::info!("eBPF program loaded and kernel-verified");

    attach_sched_process_exec(&mut bpf)?;
    Ok(bpf)
}

fn attach_sched_process_exec(bpf: &mut Ebpf) -> anyhow::Result<()> {
    let program: &mut TracePoint = bpf
        .program_mut("statix_sched_process_exec")
        .ok_or_else(|| {
            anyhow::anyhow!("BPF program 'statix_sched_process_exec' not found in ELF")
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

pub fn take_ring_drops_map(bpf: &mut Ebpf) -> anyhow::Result<PerCpuArray<MapData, u64>> {
    let map = bpf
        .take_map("RING_DROPS")
        .ok_or_else(|| anyhow::anyhow!("Per-CPU map 'RING_DROPS' not found in BPF object"))?;
    PerCpuArray::try_from(map).map_err(Into::into)
}

/// Poll `RING_DROPS` every 10s; log and export cumulative drop count when non-zero.
pub fn spawn_ring_drops_monitor(ring_drops: PerCpuArray<MapData, u64>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match ring_drops.get(&0, 0) {
                Ok(values) => {
                    let total_drops: u64 = values.iter().copied().sum();
                    if total_drops > 0 {
                        log::error!(
                            "SEVERE: eBPF ring buffer overflow! {} events silently dropped.",
                            total_drops
                        );
                        metrics::counter!("statix_ring_drops_total").absolute(total_drops);
                    }
                }
                Err(e) => log::warn!("Failed to read RING_DROPS map: {e}"),
            }
        }
    });
}

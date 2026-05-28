//! finops-user — eBPF-powered process telemetry agent.
//!
//! Loads the kernel probe, reads events from the ring buffer, and emits
//! structured JSON to stdout. Requires root or CAP_BPF + CAP_PERFMON.
//!
//! Usage (via Makefile):
//!   make build   # compile eBPF bytecode + this binary
//!   make run     # sets FINOPS_EBF_PATH and runs the agent

mod loader;
mod output;

use std::mem::size_of;

use finops_common::ProcessEvent;
use tokio::io::unix::AsyncFd;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    check_privileges()?;

    let ebpf_path = read_ebpf_path()?;

    // Load the BPF ELF, run it through the kernel verifier, attach kprobe.
    // The returned handle owns all kernel resources — dropped at end of main.
    let mut bpf = loader::load_and_attach(&ebpf_path)?;

    // Get a handle to the ring buffer that the kernel probe writes into.
    // `ring_buf` borrows from `bpf`; both must stay alive through the loop.
    let ring_buf = loader::get_ring_buf(&mut bpf)?;

    // AsyncFd bridges the ring buffer's kernel file descriptor into Tokio's
    // epoll reactor. The async task sleeps at zero CPU cost until the kernel
    // signals new events via epoll — no polling, no busy-wait.
    let mut async_fd = AsyncFd::new(ring_buf)?;

    log::info!("Listening for execve events. Press Ctrl+C to stop.");
    println!(r#"{{"status":"ready","probe":"__x64_sys_execve"}}"#);

    loop {
        tokio::select! {
            // New events available in the ring buffer.
            guard_result = async_fd.readable_mut() => {
                let mut guard = guard_result?;
                let rb = guard.get_inner_mut();

                while let Some(item) = rb.next() {
                    if item.len() < size_of::<ProcessEvent>() {
                        log::warn!("Undersized event ({} bytes), skipping", item.len());
                        continue;
                    }
                    // SAFETY: kernel wrote exactly one ProcessEvent into this slot.
                    // ProcessEvent is #[repr(C)] with only primitive integer fields —
                    // all bit patterns are valid (documented by the Pod impl in common).
                    let event: &ProcessEvent =
                        unsafe { &*(item.as_ptr() as *const ProcessEvent) };
                    output::emit(event);
                }

                // Re-arm epoll: don't wake until the kernel signals new data.
                guard.clear_ready();
            }

            // Graceful shutdown on Ctrl+C.
            _ = signal::ctrl_c() => {
                log::info!("Ctrl+C received — shutting down cleanly");
                println!(r#"{{"status":"shutdown"}}"#);
                break;
            }
        }
    }

    // `bpf` drops here: Aya detaches all kprobes and frees kernel memory.
    Ok(())
}

/// Resolve the eBPF bytecode path from the environment.
///
/// Set by the Makefile automatically. In a container deployment, point this
/// to the bundled ELF in the image (e.g. /usr/lib/finops/finops-ebpf).
fn read_ebpf_path() -> anyhow::Result<String> {
    std::env::var("FINOPS_EBF_PATH").map_err(|_| {
        anyhow::anyhow!(
            "FINOPS_EBF_PATH is not set.\n\
             Build the eBPF program first, then run via 'make run'.\n\
             Manual: FINOPS_EBF_PATH=<path> ./finops-user"
        )
    })
}

/// Abort early if the process lacks the kernel privileges needed to load BPF programs.
fn check_privileges() -> anyhow::Result<()> {
    // SAFETY: geteuid() is always safe to call.
    if unsafe { libc::geteuid() } != 0 {
        anyhow::bail!(
            "Must run as root or with CAP_BPF + CAP_PERFMON.\n\
             Development: sudo ./target/release/finops-user\n\
             Production:  setcap 'cap_bpf,cap_perfmon=eip' ./finops-user"
        );
    }
    Ok(())
}

//! CI-only: run the kernel BPF verifier via Aya load (no probe attach).
//!
//! Usage: `statix-ebpf-verify <path-to-ebpf-elf>`
//! Requires CAP_BPF / root (virtme-ng guest in CI).

use std::{fs, process};

use aya::Ebpf;
use statix::bpf_memlock::bump_memlock_rlimit;

fn main() {
    if let Err(e) = run() {
        eprintln!("statix-ebpf-verify: {e:#}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: statix-ebpf-verify <elf-path>")?;
    bump_memlock_rlimit()?;
    let bytes = fs::read(&path)?;
    let _bpf = Ebpf::load(&bytes)?;
    println!("kernel verifier accepted {path}");
    Ok(())
}

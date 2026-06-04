//! Resolve which prebuilt eBPF ELF to load (CPU-tier bundle or explicit override).

use std::path::PathBuf;

const BPF_BUNDLE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/bpf");

/// `FINOPS_EBF_PATH` overrides; else pick `finops-ebpf-{small,large,xlarge}` from `FINOPS_BPF_DIR` (default `target/bpf`).
pub fn resolve_ebpf_path() -> anyhow::Result<String> {
    if let Ok(path) = std::env::var("FINOPS_EBF_PATH") {
        return Ok(path);
    }
    select_ebpf_by_cpus()
}

fn select_ebpf_by_cpus() -> anyhow::Result<String> {
    let cpus = num_cpus::get();
    let (variant, bytes) = match cpus {
        0..=8 => ("finops-ebpf-small", 512 * 1024),
        9..=64 => ("finops-ebpf-large", 4 * 1024 * 1024),
        _ => ("finops-ebpf-xlarge", 8 * 1024 * 1024),
    };

    log::info!(
        "Detected {cpus} cores — loading {variant} ({bytes} byte ring buffer)"
    );

    let dir = std::env::var("FINOPS_BPF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(BPF_BUNDLE_DIR));

    let path = dir.join(variant);
    if !path.is_file() {
        anyhow::bail!(
            "eBPF variant not found: {}. Run `make build-ebpf` (builds small/large/xlarge bundle).",
            path.display()
        );
    }

    Ok(path.to_string_lossy().into_owned())
}

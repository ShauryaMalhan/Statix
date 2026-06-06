//! Resolve which prebuilt eBPF ELF to load (CPU-tier bundle or explicit override).

use std::path::PathBuf;

const BPF_BUNDLE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/bpf");

/// `STATIX_EBF_PATH` overrides; else pick `statix-ebpf-{small,large,xlarge}` from `STATIX_BPF_DIR` (default `target/bpf`).
pub fn resolve_ebpf_path() -> anyhow::Result<String> {
    if let Some(path) = statix_infra::env::var("STATIX_EBF_PATH") {
        return Ok(path);
    }
    select_ebpf_by_cpus()
}

fn select_ebpf_by_cpus() -> anyhow::Result<String> {
    let cpus = num_cpus::get();
    let (variant, bytes) = match cpus {
        0..=8 => ("statix-ebpf-small", 512 * 1024),
        9..=64 => ("statix-ebpf-large", 4 * 1024 * 1024),
        _ => ("statix-ebpf-xlarge", 8 * 1024 * 1024),
    };

    log::info!(
        "Detected {cpus} cores — loading {variant} ({bytes} byte ring buffer)"
    );

    let dir = statix_infra::env::var("STATIX_BPF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(BPF_BUNDLE_DIR));

    let path = dir.join(variant);
    if !path.is_file() {
        anyhow::bail!(
            "eBPF variant not found: {}. Run `make build-ebpf` (builds small/large/xlarge bundle).",
            path.display()
        );
    }

    Ok(path.to_string_lossy().into_owned())
}

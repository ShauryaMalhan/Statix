fn main() {
    let size: u32 = std::env::var("FINOPS_RING_BUF_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512 * 1024);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let path = std::path::Path::new(&out_dir).join("ring_config.rs");
    std::fs::write(
        &path,
        format!("pub const RING_BUF_BYTES: u32 = {size};\n"),
    )
    .unwrap();

    println!("cargo:rerun-if-env-changed=FINOPS_RING_BUF_BYTES");
}

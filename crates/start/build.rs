fn main() {
    // Build scripts are compiled for the host, so `cfg(windows)` is still true
    // when a Windows developer cross-compiles a Linux binary. Inspect Cargo's
    // target metadata before invoking the Windows resource compiler.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon_path = manifest_dir.join("../../apps/src-tauri/icons/icon.ico");

    println!("cargo:rerun-if-changed={}", icon_path.display());
    if !icon_path.is_file() {
        panic!("Windows icon not found: {}", icon_path.display());
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon_path.to_string_lossy().as_ref());
    res.compile()
        .expect("failed to compile Windows resources (icon)");
}

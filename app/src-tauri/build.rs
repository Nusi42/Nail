fn main() {
    // tauri-build links the VC runtime statically by default by writing an
    // empty `msvcrt.lib` into its OUT_DIR and adding that directory to the
    // native link search path. In `cargo test --workspace` that search path
    // leaks into the doctests of every other crate, which then fail to link
    // (`__CxxFrameHandler3` unresolved). Static linking only matters for
    // distributed (release) binaries, so keep it there and use the dynamic
    // runtime for debug builds and tests.
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let windows = tauri_build::WindowsAttributes::new().static_vc_runtime(release);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}

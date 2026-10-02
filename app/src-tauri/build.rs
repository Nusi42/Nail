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
    // Delay-load comctl32: Nail.exe's manifest (from tauri-build) selects
    // Common Controls 6, test executables have no manifest and get v5, which
    // lacks TaskDialogIndirect. Imported eagerly, that made the shell's test
    // binaries fail to start (STATUS_ENTRYPOINT_NOT_FOUND) once they linked
    // more window code (M4c print windows); delay-loaded, the function is
    // only looked up when called (never in tests).
    if std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc") {
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}

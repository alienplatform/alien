fn main() {
    napi_build::setup();

    // The addon's `.node` (cdylib) resolves the `napi_*` symbols at load time
    // against the host (Node/Bun), which napi-build wires up for the cdylib.
    // The `cargo test` harness, however, links a plain executable with no host
    // present, so those symbols are undefined at link time. Defer their
    // resolution for every locally-linked artifact (which `rustc-link-arg`
    // covers: tests, benches, bins, and the cdylib) — the pure
    // translation/mapper tests never actually call into napi at runtime.
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            println!("cargo:rustc-link-arg=-undefined");
            println!("cargo:rustc-link-arg=dynamic_lookup");
        }
        Ok("linux") => {
            println!("cargo:rustc-link-arg=-Wl,--unresolved-symbols=ignore-all");
        }
        _ => {}
    }
}

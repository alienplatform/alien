fn main() {
    napi_build::setup();

    // `cargo test` links a plain executable with no Node/Bun host, so the `napi_*`
    // symbols the cdylib resolves at load time are undefined at link time. Defer
    // resolution for every locally-linked artifact — the mapper tests never call
    // into napi.
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

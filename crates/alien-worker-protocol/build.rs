fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/wait_until.proto");
    println!("cargo:rerun-if-changed=proto/control.proto");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Compile wait_until proto
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("alien_worker.wait_until_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/wait_until.proto"], &["proto/"])?;

    // Compile control proto
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("alien_worker.control_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/control.proto"], &["proto/"])?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/wait_until.proto");
    println!("cargo:rerun-if-changed=proto/control.proto");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Only compile protos if grpc feature is enabled
    #[cfg(feature = "grpc")]
    {
        compile_protos(&out_dir)?;
    }

    #[cfg(not(feature = "grpc"))]
    {
        println!("cargo:warning=Skipping protobuf compilation (grpc feature not enabled)");
    }

    Ok(())
}

#[cfg(feature = "grpc")]
fn compile_protos(out_dir: &std::path::PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Compile wait_until proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.wait_until_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/wait_until.proto"], &["proto/"])?;

    // Compile control proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.control_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/control.proto"], &["proto/"])?;

    Ok(())
}

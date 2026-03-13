fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/storage.proto");
    println!("cargo:rerun-if-changed=proto/build.proto");
    println!("cargo:rerun-if-changed=proto/artifact_registry.proto");
    println!("cargo:rerun-if-changed=proto/vault.proto");
    println!("cargo:rerun-if-changed=proto/kv.proto");
    println!("cargo:rerun-if-changed=proto/queue.proto");
    println!("cargo:rerun-if-changed=proto/function.proto");
    println!("cargo:rerun-if-changed=proto/container.proto");
    println!("cargo:rerun-if-changed=proto/service_account.proto");
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
    // Compile storage proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.storage_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/storage.proto"], &["proto/"])?;

    // Compile build proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.build_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/build.proto"], &["proto/"])?;

    // Compile artifact registry proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.artifact_registry_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/artifact_registry.proto"], &["proto/"])?;

    // Compile vault proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.vault_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/vault.proto"], &["proto/"])?;

    // Compile kv proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.kv_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/kv.proto"], &["proto/"])?;

    // Compile queue proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.queue_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/queue.proto"], &["proto/"])?;

    // Compile function proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.function_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/function.proto"], &["proto/"])?;

    // Compile container proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.container_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/container.proto"], &["proto/"])?;

    // Compile service account proto
    tonic_build::configure()
        .build_server(cfg!(feature = "grpc"))
        .build_client(cfg!(feature = "grpc"))
        .file_descriptor_set_path(out_dir.join("alien_bindings.service_account_descriptor.bin"))
        .type_attribute(
            ".",
            "#[allow(clippy::doc_lazy_continuation, clippy::enum_variant_names)]",
        )
        .compile_protos(&["proto/service_account.proto"], &["proto/"])?;

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

use super::*;

#[tokio::test]
async fn source_artifact_cache_key_is_shared_for_equivalent_cloud_builds() {
    let src_dir = tempdir().unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(
        src_dir.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let toolchain = ToolchainConfig::Rust {
        binary_name: "app".to_string(),
    };
    let targets = vec![BinaryTarget::LinuxX64];
    let gcp = BuildSettings {
        output_directory: src_dir.path().join("out").to_string_lossy().into_owned(),
        platform: PlatformBuildSettings::Gcp {},
        targets: Some(targets.clone()),
        cache_url: None,
        override_base_image: Some("registry.example.com/base:tag".to_string()),
        debug_mode: false,
    };
    let azure = BuildSettings {
        platform: PlatformBuildSettings::Azure {},
        override_base_image: Some("registry.example.com/base:other-tag".to_string()),
        ..gcp.clone()
    };

    let gcp_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &gcp,
        &targets,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();
    let azure_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &azure,
        &targets,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();

    assert_eq!(
        gcp_key, azure_key,
        "direct workloads must ignore the Worker runtime-base override"
    );
    let gcp_daemon_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &gcp,
        &targets,
        crate::toolchain::WorkloadKind::Daemon,
    )
    .await
    .unwrap();
    let azure_daemon_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &azure,
        &targets,
        crate::toolchain::WorkloadKind::Daemon,
    )
    .await
    .unwrap();
    assert_eq!(
        gcp_daemon_key, azure_daemon_key,
        "Daemon artifacts must ignore the Worker runtime-base override"
    );

    let gcp_worker_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &gcp,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    let azure_worker_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &azure,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    assert_ne!(
        gcp_worker_key, azure_worker_key,
        "Worker artifacts must include their runtime base in the cache key"
    );

    let docker_toolchain = ToolchainConfig::Docker {
        dockerfile: None,
        build_args: None,
        target: None,
    };
    let gcp_docker_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &docker_toolchain,
        &gcp,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    let azure_docker_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &docker_toolchain,
        &azure,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    assert_eq!(
        gcp_docker_key, azure_docker_key,
        "Dockerfile builds own their base and must ignore the source Worker override"
    );

    let local_a = BuildSettings {
        platform: PlatformBuildSettings::Local {},
        ..gcp
    };
    let local_b = BuildSettings {
        platform: PlatformBuildSettings::Local {},
        ..azure
    };
    let local_a_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &local_a,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    let local_b_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &local_b,
        &targets,
        crate::toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    assert_eq!(
        local_a_key, local_b_key,
        "Local Workers run from scratch and must ignore the cloud runtime base"
    );
}

#[tokio::test]
async fn rust_source_artifact_cache_key_includes_local_path_dependencies() {
    let workspace_dir = tempdir().unwrap();
    let app_dir = workspace_dir.path().join("app");
    let dep_dir = workspace_dir.path().join("dep");
    std::fs::create_dir_all(app_dir.join("src")).unwrap();
    std::fs::create_dir_all(dep_dir.join("src")).unwrap();
    std::fs::write(
        workspace_dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\", \"dep\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        app_dir.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndep = { path = \"../dep\" }\n",
    )
    .unwrap();
    std::fs::write(app_dir.join("src/main.rs"), "fn main() { dep::value(); }\n").unwrap();
    std::fs::write(
        dep_dir.join("Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dep_dir.join("src/lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();

    let toolchain = ToolchainConfig::Rust {
        binary_name: "app".to_string(),
    };
    let targets = vec![BinaryTarget::LinuxX64];
    let settings = BuildSettings {
        output_directory: workspace_dir
            .path()
            .join("out")
            .to_string_lossy()
            .into_owned(),
        platform: PlatformBuildSettings::Gcp {},
        targets: Some(targets.clone()),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let first_key = compute_source_artifact_cache_key(
        app_dir.to_str().unwrap(),
        &toolchain,
        &settings,
        &targets,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();

    std::fs::write(dep_dir.join("src/lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();

    let second_key = compute_source_artifact_cache_key(
        app_dir.to_str().unwrap(),
        &toolchain,
        &settings,
        &targets,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();

    assert_ne!(first_key, second_key);
}

#[tokio::test]
async fn rust_source_artifact_cache_key_includes_workspace_toolchain_files() {
    // Toolchain files live at the workspace root, not inside the member's
    // package directory, so this must use a real `[workspace]` layout —
    // otherwise package_dir == workspace_root and hash_source_directory
    // picks the files up as ordinary source, masking a broken/deleted
    // workspace-root hashing loop.
    let workspace_dir = tempdir().unwrap();
    let app_dir = workspace_dir.path().join("app");
    std::fs::create_dir_all(app_dir.join("src")).unwrap();
    std::fs::write(
        workspace_dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        app_dir.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(app_dir.join("src/main.rs"), "fn main() {}\n").unwrap();

    let toolchain = ToolchainConfig::Rust {
        binary_name: "app".to_string(),
    };
    let targets = vec![BinaryTarget::LinuxX64];
    let settings = BuildSettings {
        output_directory: workspace_dir
            .path()
            .join("out")
            .to_string_lossy()
            .into_owned(),
        platform: PlatformBuildSettings::Gcp {},
        targets: Some(targets.clone()),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let key = |dir: &Path| {
        let dir = dir.to_str().unwrap().to_string();
        let toolchain = toolchain.clone();
        let settings = settings.clone();
        let targets = targets.clone();
        async move {
            compute_source_artifact_cache_key(
                &dir,
                &toolchain,
                &settings,
                &targets,
                crate::toolchain::WorkloadKind::Container,
            )
            .await
            .unwrap()
        }
    };

    let without_toolchain_file = key(&app_dir).await;

    std::fs::write(
        workspace_dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.84.0\"\n",
    )
    .unwrap();
    let with_pinned_toolchain = key(&app_dir).await;
    assert_ne!(
        without_toolchain_file, with_pinned_toolchain,
        "pinning the compiler via a workspace-root rust-toolchain.toml must invalidate the artifact cache key"
    );

    std::fs::write(
        workspace_dir.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.85.0\"\n",
    )
    .unwrap();
    let with_changed_toolchain = key(&app_dir).await;
    assert_ne!(
        with_pinned_toolchain, with_changed_toolchain,
        "changing the content of the workspace-root rust-toolchain.toml must invalidate the artifact cache key"
    );

    std::fs::create_dir_all(workspace_dir.path().join(".cargo")).unwrap();
    std::fs::write(
        workspace_dir.path().join(".cargo/config.toml"),
        "[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
    )
    .unwrap();
    let with_cargo_config = key(&app_dir).await;
    assert_ne!(
        with_changed_toolchain, with_cargo_config,
        "changing rustflags via workspace-root .cargo/config.toml must invalidate the artifact cache key"
    );
}

#[tokio::test]
async fn source_artifact_cache_key_differs_across_target_triples() {
    let src_dir = tempdir().unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(
        src_dir.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let toolchain = ToolchainConfig::Rust {
        binary_name: "app".to_string(),
    };
    let key_for = |targets: Vec<BinaryTarget>| {
        let dir = src_dir.path().to_str().unwrap().to_string();
        let out = src_dir.path().join("out").to_string_lossy().into_owned();
        let toolchain = toolchain.clone();
        async move {
            let settings = BuildSettings {
                output_directory: out,
                platform: PlatformBuildSettings::Gcp {},
                targets: Some(targets.clone()),
                cache_url: None,
                override_base_image: None,
                debug_mode: false,
            };
            compute_source_artifact_cache_key(
                &dir,
                &toolchain,
                &settings,
                &targets,
                crate::toolchain::WorkloadKind::Container,
            )
            .await
            .unwrap()
        }
    };

    let x64_key = key_for(vec![BinaryTarget::LinuxX64]).await;
    let arm64_key = key_for(vec![BinaryTarget::LinuxArm64]).await;
    assert_ne!(
        x64_key, arm64_key,
        "different target triples must not share build artifacts"
    );
}

/// Reuse invariant, end to end at the cache layer: after one platform's build
/// produces artifacts, an equivalent-target build for another platform finds
/// them (one build total), while a build for a different triple misses even
/// though the tarball file exists (two builds total).
#[tokio::test]
async fn equivalent_platform_build_reuses_artifact_but_differing_triple_rebuilds() {
    let src_dir = tempdir().unwrap();
    std::fs::create_dir_all(src_dir.path().join("src")).unwrap();
    std::fs::write(
        src_dir.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(src_dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let toolchain = ToolchainConfig::Rust {
        binary_name: "app".to_string(),
    };
    let out_root = tempdir().unwrap();
    let settings_for = |platform: PlatformBuildSettings, targets: &[BinaryTarget]| BuildSettings {
        output_directory: out_root.path().to_string_lossy().into_owned(),
        platform,
        targets: Some(targets.to_vec()),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };
    let x64 = vec![BinaryTarget::LinuxX64];
    let arm64 = vec![BinaryTarget::LinuxArm64];

    // "First build" (gcp, linux-x64): produce the hashed artifact directory
    // exactly as build_resource finalizes it.
    let gcp_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &settings_for(PlatformBuildSettings::Gcp {}, &x64),
        &x64,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();
    let gcp_dir = out_root.path().join("build").join("gcp");
    let artifact_dir = gcp_dir.join("app-12345678");
    fs::create_dir_all(&artifact_dir).await.unwrap();
    fs::write(artifact_dir.join("linux-x64.oci.tar"), b"oci")
        .await
        .unwrap();
    // Also stage an arm64 tarball so the differing-triple case below is
    // decided by the cache key, not by a missing target file.
    fs::write(artifact_dir.join("linux-arm64.oci.tar"), b"oci")
        .await
        .unwrap();
    write_artifact_cache_metadata(&artifact_dir, &gcp_key)
        .await
        .unwrap();

    // "Second build" (azure, same source, same linux-x64 target): the key
    // matches and the sibling-platform lookup finds the gcp artifacts, so
    // no second compile happens.
    let azure_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &settings_for(PlatformBuildSettings::Azure {}, &x64),
        &x64,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();
    assert_eq!(gcp_key, azure_key, "equivalent platforms must share keys");

    let azure_dir = out_root.path().join("build").join("azure");
    fs::create_dir_all(&azure_dir).await.unwrap();
    let reused = find_cached_artifact_dir(&azure_dir, "app", &x64, &azure_key)
        .await
        .unwrap();
    assert_eq!(
        reused,
        Some(artifact_dir.clone()),
        "same inputs + equivalent targets must reuse the one built artifact"
    );

    // "Third build" (aws, linux-arm64): the tarball file exists, but the
    // key differs, so the lookup misses and a real build would run.
    let aws_key = compute_source_artifact_cache_key(
        src_dir.path().to_str().unwrap(),
        &toolchain,
        &settings_for(
            PlatformBuildSettings::Aws {
                managing_account_id: None,
            },
            &arm64,
        ),
        &arm64,
        crate::toolchain::WorkloadKind::Container,
    )
    .await
    .unwrap();
    assert_ne!(gcp_key, aws_key);

    let aws_dir = out_root.path().join("build").join("aws");
    fs::create_dir_all(&aws_dir).await.unwrap();
    let miss = find_cached_artifact_dir(&aws_dir, "app", &arm64, &aws_key)
        .await
        .unwrap();
    assert_eq!(miss, None, "a differing triple must trigger its own build");
}

#[tokio::test]
async fn artifact_cache_lookup_reuses_sibling_platform_directory() {
    let temp_root = tempdir().unwrap();
    let build_root = temp_root.path().join("build");
    let gcp_dir = build_root.join("gcp");
    let azure_dir = build_root.join("azure");
    let cached_dir = gcp_dir.join("alien-manager-abcdef12");

    fs::create_dir_all(&cached_dir).await.unwrap();
    fs::create_dir_all(&azure_dir).await.unwrap();
    fs::write(cached_dir.join("linux-x64.oci.tar"), b"oci")
        .await
        .unwrap();
    write_artifact_cache_metadata(&cached_dir, "cache-key")
        .await
        .unwrap();

    let found = find_cached_artifact_dir(
        &azure_dir,
        "alien-manager",
        &[BinaryTarget::LinuxX64],
        "cache-key",
    )
    .await
    .unwrap();

    assert_eq!(found, Some(cached_dir));
}

#[tokio::test]
async fn finalize_artifact_dir_reuses_existing_final_directory() {
    let temp_root = tempdir().unwrap();
    let temp_dir = temp_root.path().join(".agent-tmp-1234");
    let final_dir = temp_root.path().join("agent-abcdef12");

    fs::create_dir_all(&temp_dir).await.unwrap();
    fs::write(temp_dir.join("linux-x64.oci.tar"), b"new-build")
        .await
        .unwrap();

    fs::create_dir_all(&final_dir).await.unwrap();
    fs::write(final_dir.join("linux-x64.oci.tar"), b"existing-build")
        .await
        .unwrap();

    let resolved = finalize_artifact_dir(&temp_dir, &final_dir, "build")
        .await
        .unwrap();

    assert_eq!(resolved, final_dir.to_string_lossy());
    assert!(final_dir.exists());
    assert!(!temp_dir.exists());
    assert_eq!(
        fs::read(final_dir.join("linux-x64.oci.tar")).await.unwrap(),
        b"existing-build"
    );
}

#[test]
fn temp_artifact_dir_is_hidden_and_unique() {
    let build_output_dir = PathBuf::from("/tmp/build-output");

    let first = temp_artifact_dir(&build_output_dir, "agent");
    let second = temp_artifact_dir(&build_output_dir, "agent");

    assert_ne!(first, second);
    assert_eq!(first.parent().unwrap(), build_output_dir.as_path());
    assert!(first
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(".agent-tmp-"));
    assert!(second
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(".agent-tmp-"));
}

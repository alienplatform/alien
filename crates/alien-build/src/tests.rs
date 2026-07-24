use super::*;
use crate::base_images::*;
use crate::cache::*;
use crate::push::*;
use crate::settings::PushSettings;
use alien_core::Worker;
use dockdash::Image;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig};
use oci_client::manifest::{
    OciImageIndex, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
};
use oci_client::Reference;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

mod cache_tests;
mod registry_tests;

#[tokio::test]
async fn materializing_same_artifact_preserves_its_contents() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let absolute = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("image.oci.tar");
    std::fs::write(&absolute, b"oci archive").unwrap();
    let relative = absolute
        .strip_prefix(std::env::current_dir().unwrap())
        .unwrap();

    assert_ne!(relative, absolute);
    assert!(!materialize_complete_oci_tarball(&absolute, relative)
        .await
        .unwrap());
    assert_eq!(std::fs::read(absolute).unwrap(), b"oci archive");
}

fn toolchain_output(
    entrypoint: Option<Vec<String>>,
    runtime_command: Vec<String>,
) -> toolchain::ToolchainOutput {
    toolchain::ToolchainOutput {
        build_strategy: toolchain::ImageBuildStrategy::FromScratch { layers: vec![] },
        entrypoint,
        runtime_command,
    }
}

/// Pins the ENTRYPOINT/CMD contract shared by the base-image and
/// from-scratch build paths (see also tests/image_shape_tests.rs).
#[test]
fn image_entrypoint_and_cmd_contract() {
    // Worker: base entrypoint kept, CMD is the separator + binary.
    let worker = toolchain_output(None, vec!["--".to_string(), "./bin".to_string()]);
    assert_eq!(
        image_entrypoint_and_cmd(&worker),
        (None, Some(vec!["--".to_string(), "./bin".to_string()]))
    );

    // Direct entrypoint (Container/Daemon): binary is the entrypoint, no CMD.
    let direct = toolchain_output(Some(vec!["/app/bin".to_string()]), vec![]);
    assert_eq!(
        image_entrypoint_and_cmd(&direct),
        (Some(vec!["/app/bin".to_string()]), None)
    );

    // Local from-scratch (host process): no entrypoint, CMD is the binary.
    let local = toolchain_output(None, vec!["./bin".to_string()]);
    assert_eq!(
        image_entrypoint_and_cmd(&local),
        (None, Some(vec!["./bin".to_string()]))
    );

    // Explicit entrypoint with a nonempty command keeps both.
    let both = toolchain_output(
        Some(vec!["/app/bin".to_string()]),
        vec!["serve".to_string()],
    );
    assert_eq!(
        image_entrypoint_and_cmd(&both),
        (
            Some(vec!["/app/bin".to_string()]),
            Some(vec!["serve".to_string()])
        )
    );
}

#[test]
fn runtime_base_override_only_applies_to_workers() {
    let direct_bases = vec!["cgr.dev/chainguard/wolfi-base:latest".to_string()];
    let runtime_base = "registry.example.com/alien-base:feature";

    assert_eq!(
        base_images_for_workload(&direct_bases, None, toolchain::WorkloadKind::Worker),
        direct_bases,
        "without an override the declared default bases must be preserved"
    );
    assert_eq!(
        base_images_for_workload(
            &direct_bases,
            Some(runtime_base),
            toolchain::WorkloadKind::Worker,
        ),
        vec![runtime_base.to_string()]
    );
    for workload in [
        toolchain::WorkloadKind::Container,
        toolchain::WorkloadKind::Daemon,
    ] {
        assert_eq!(
            base_images_for_workload(&direct_bases, Some(runtime_base), workload),
            direct_bases,
            "{} must not inherit the Worker runtime base",
            workload.as_str()
        );
    }
}

#[test]
fn requested_host_binary_only_gates_container_skip() {
    use BinaryTarget::*;
    // None (defaults to host OS) and empty → containers still build.
    assert!(!requested_host_binary_only(None));
    assert!(!requested_host_binary_only(Some(&[])));
    // Explicit non-Linux-only → nothing for a container to build, skip it.
    assert!(requested_host_binary_only(Some(&[DarwinArm64])));
    assert!(requested_host_binary_only(Some(&[WindowsX64])));
    assert!(requested_host_binary_only(Some(&[DarwinArm64, WindowsX64])));
    // Any Linux target present → containers build for it.
    assert!(!requested_host_binary_only(Some(&[LinuxArm64])));
    assert!(!requested_host_binary_only(Some(&[LinuxX64])));
    assert!(!requested_host_binary_only(Some(&[
        DarwinArm64,
        LinuxArm64
    ])));
}

#[test]
fn local_build_strips_daemon_only_compute_cluster() {
    let cluster = alien_core::ComputeCluster::new("host-runtime".to_string())
        .capacity_group(alien_core::CapacityGroup {
            group_id: "general".to_string(),
            instance_type: Some("m8i.xlarge".to_string()),
            profile: None,
            min_size: 1,
            max_size: 1,
            scale_policy: None,
            nested_virtualization: Some(true),
        })
        .build();
    let daemon = Daemon::new("host-loader".to_string())
        .cluster("host-runtime".to_string())
        .permissions("loader".to_string())
        .code(DaemonCode::Image {
            image: "registry.example.com/host-loader:latest".to_string(),
        })
        .build();
    let mut stack = Stack::new("host-loader-stack".to_string())
        .add(cluster, alien_core::ResourceLifecycle::Frozen)
        .add(daemon, alien_core::ResourceLifecycle::Live)
        .build();

    strip_local_daemon_only_compute_clusters(&mut stack, Platform::Local);

    assert!(!stack.resources.contains_key("host-runtime"));
    let daemon = stack
        .resources()
        .find(|(id, _)| *id == "host-loader")
        .and_then(|(_, entry)| entry.config.downcast_ref::<Daemon>())
        .expect("daemon should remain");
    assert_eq!(daemon.cluster, None);
}

#[tokio::test]
async fn machines_build_rejects_workers_before_writing_artifacts() {
    let output = tempdir().unwrap();
    let worker = Worker::new("job".to_string())
        .permissions("execution".to_string())
        .code(WorkerCode::Image {
            image: "registry.example.com/job:latest".to_string(),
        })
        .build();
    let stack = Stack::new("machines-worker".to_string())
        .add(worker, alien_core::ResourceLifecycle::Live)
        .build();
    let settings = BuildSettings {
        output_directory: output.path().display().to_string(),
        platform: PlatformBuildSettings::Machines {},
        targets: Some(BinaryTarget::LINUX.to_vec()),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    let error = build_stack(stack, &settings)
        .await
        .expect_err("machines worker should fail build-time preflight");

    assert_eq!(error.code, "STACK_PROCESSOR_FAILED");
    let serialized = serde_json::to_string(&error).expect("error should serialize");
    assert!(serialized.contains("MACHINES_UNSUPPORTED_RESOURCE"));
    assert!(!output.path().join("build").join("machines").exists());
}

#[test]
fn source_cache_hash_ignores_build_artifacts() {
    let src = tempdir().unwrap();
    std::fs::create_dir_all(src.path().join(".alien-build")).unwrap();
    std::fs::create_dir_all(src.path().join("node_modules")).unwrap();
    std::fs::write(src.path().join("package.json"), "{}").unwrap();
    std::fs::write(
        src.path().join(".alien-build/__alien_bootstrap.ts"),
        "generated",
    )
    .unwrap();
    std::fs::write(
        src.path().join(".18ba89dff9ff58bf-00000000.bun-build"),
        "generated",
    )
    .unwrap();
    std::fs::write(src.path().join("node_modules/module.js"), "dependency").unwrap();

    let mut files = Vec::new();
    collect_source_files(src.path(), src.path(), &mut files).unwrap();
    files.sort();

    assert_eq!(files, vec![PathBuf::from("package.json")]);
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// True if a real OCI registry answers at `base/v2/` (200 or 401). Used to gate the
/// multi-arch push test. Run one with: `docker run -d -p 5050:5000 registry:2`.
async fn registry_available(base: &str) -> bool {
    match reqwest::get(format!("{base}/v2/")).await {
        Ok(resp) => resp.status().is_success() || resp.status().as_u16() == 401,
        Err(_) => false,
    }
}

fn test_container(name: &str, image: String) -> Container {
    Container::new(name.to_string())
        .code(ContainerCode::Image { image })
        .cpu(alien_core::ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .permissions("container-execution".to_string())
        .build()
}

#[test]
fn retryable_image_pull_detects_oci_server_errors() {
    let error = dockdash::Error::ImagePull {
        image_ref: "ghcr.io/example/base:tag".to_string(),
        message: "Failed to pull layer blob sha256:abc".to_string(),
        source: Some(Box::new(
            oci_client::errors::OciDistributionError::ServerError {
                code: 502,
                url: "https://ghcr.io/v2/example/base/blobs/sha256:abc".to_string(),
                message: "Bad Gateway".to_string(),
            },
        )),
    };

    assert!(is_retryable_dockdash_image_pull_error(&error));
}

#[test]
fn retryable_image_pull_detects_opaque_transport_errors() {
    let error = dockdash::Error::ImagePull {
        image_ref: "ghcr.io/example/base:tag".to_string(),
        message: "Failed to pull layer blob sha256:abc".to_string(),
        source: Some(Box::new(std::io::Error::other(
            "error sending request for url (https://ghcr.io/v2/example/base/blobs/sha256:abc): client error (SendRequest): connection error",
        ))),
    };

    assert!(is_retryable_dockdash_image_pull_error(&error));
}

#[test]
fn retryable_image_pull_rejects_auth_and_not_found_errors() {
    let auth_error = dockdash::Error::ImagePull {
        image_ref: "ghcr.io/example/base:tag".to_string(),
        message: "Failed to pull layer blob sha256:abc".to_string(),
        source: Some(Box::new(
            oci_client::errors::OciDistributionError::UnauthorizedError {
                url: "https://ghcr.io/v2/example/base/blobs/sha256:abc".to_string(),
            },
        )),
    };
    let missing_error = dockdash::Error::ImagePull {
        image_ref: "ghcr.io/example/base:tag".to_string(),
        message: "Failed to pull manifest".to_string(),
        source: Some(Box::new(
            oci_client::errors::OciDistributionError::ImageManifestNotFoundError(
                "ghcr.io/example/base:tag".to_string(),
            ),
        )),
    };

    assert!(!is_retryable_dockdash_image_pull_error(&auth_error));
    assert!(!is_retryable_dockdash_image_pull_error(&missing_error));
}

#[test]
fn oci_tarball_target_maps_runtime_platform_ids() {
    assert_eq!(
        oci_tarball_target(Path::new("/x/linux-aarch64.oci.tar")),
        Some(BinaryTarget::LinuxArm64)
    );
    assert_eq!(
        oci_tarball_target(Path::new("linux-x64.oci.tar")),
        Some(BinaryTarget::LinuxX64)
    );
    assert_eq!(oci_tarball_target(Path::new("stack.json")), None);
    assert_eq!(oci_tarball_target(Path::new("linux-arm64.oci.tar")), None); // CLI spelling, not a tarball name
}

#[test]
fn select_linux_tarballs_keeps_only_linux_sorted() {
    let files = vec![
        PathBuf::from("/b/windows-x64.oci.tar"),
        PathBuf::from("/b/linux-x64.oci.tar"),
        PathBuf::from("/b/darwin-aarch64.oci.tar"),
        PathBuf::from("/b/linux-aarch64.oci.tar"),
    ];
    let selected = select_linux_tarballs(&files);
    assert_eq!(
        selected.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![BinaryTarget::LinuxArm64, BinaryTarget::LinuxX64], // sorted by runtime id: linux-aarch64 < linux-x64
    );
}

#[test]
fn assemble_image_index_sets_oci_index_shape() {
    let entry = image_index_entry(
        BinaryTarget::LinuxArm64,
        "sha256:abc".to_string(),
        123,
        OCI_IMAGE_MEDIA_TYPE.to_string(),
    );
    let platform = entry.platform.as_ref().unwrap();
    assert_eq!(platform.architecture, "arm64");
    assert_eq!(platform.os, "linux");

    let index = assemble_image_index(vec![entry]);
    assert_eq!(index.schema_version, 2);
    assert_eq!(
        index.media_type.as_deref(),
        Some(OCI_IMAGE_INDEX_MEDIA_TYPE)
    );
    assert_eq!(index.manifests.len(), 1);
    assert_eq!(index.manifests[0].digest, "sha256:abc");
    assert_eq!(index.manifests[0].size, 123);
}

#[test]
fn manifest_media_type_reads_field_or_none() {
    assert_eq!(
        manifest_media_type(br#"{"mediaType":"application/vnd.oci.image.manifest.v1+json"}"#),
        Some("application/vnd.oci.image.manifest.v1+json".to_string())
    );
    assert_eq!(manifest_media_type(br#"{"schemaVersion":2}"#), None);
    assert_eq!(manifest_media_type(b"not json"), None);
}

#[test]
fn collect_push_targets_groups_resources_that_share_local_image_directory() {
    let temp_root = tempdir().unwrap();
    let shared_dir = temp_root.path().join("shared-image");
    let unique_dir = temp_root.path().join("unique-image");
    std::fs::create_dir_all(&shared_dir).unwrap();
    std::fs::create_dir_all(&unique_dir).unwrap();

    let shared_image = shared_dir.to_string_lossy().into_owned();
    let unique_image = unique_dir.to_string_lossy().into_owned();

    let messaging_gateway = test_container("messaging-gateway", shared_image.clone());
    let billing_worker = test_container("billing-worker", shared_image);
    let postgres = test_container("postgres", unique_image);
    let remote = test_container("remote", "registry.example.com/remote:latest".to_string());

    let mut stack = Stack::new("push-dedupe".to_string())
        .add(messaging_gateway, alien_core::ResourceLifecycle::Frozen)
        .add(billing_worker, alien_core::ResourceLifecycle::Frozen)
        .add(postgres, alien_core::ResourceLifecycle::Frozen)
        .add(remote, alien_core::ResourceLifecycle::Frozen)
        .build();

    let targets = collect_push_targets(&stack).unwrap();

    assert_eq!(targets.len(), 2);
    assert_eq!(
        targets[0].resource_names,
        vec![
            "messaging-gateway".to_string(),
            "billing-worker".to_string()
        ]
    );
    assert_eq!(
        targets[0].resource_ids,
        vec![
            "messaging-gateway".to_string(),
            "billing-worker".to_string()
        ]
    );
    assert_eq!(targets[0].resource_type, "container");
    assert_eq!(targets[0].local_image_dir, shared_dir);
    assert_eq!(targets[1].resource_names, vec!["postgres".to_string()]);

    let mut updates = targets[0].push_result_updates("registry.example.com/shared:tag".into());
    updates.extend(targets[1].push_result_updates("registry.example.com/postgres:tag".into()));
    apply_pushed_images(&mut stack, updates);

    let images = stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .and_then(|container| match &container.code {
                    ContainerCode::Image { image } => Some((id.clone(), image.clone())),
                    ContainerCode::Source { .. } => None,
                })
        })
        .collect::<HashMap<_, _>>();

    assert_eq!(
        images.get("messaging-gateway").unwrap(),
        "registry.example.com/shared:tag"
    );
    assert_eq!(
        images.get("billing-worker").unwrap(),
        "registry.example.com/shared:tag"
    );
    assert_eq!(
        images.get("postgres").unwrap(),
        "registry.example.com/postgres:tag"
    );
    assert_eq!(
        images.get("remote").unwrap(),
        "registry.example.com/remote:latest"
    );
}

#[test]
fn collect_push_targets_handles_daemons_like_other_compute() {
    let temp_root = tempdir().unwrap();
    let daemon_dir = temp_root.path().join("daemon-image");
    std::fs::create_dir_all(&daemon_dir).unwrap();

    let local_daemon = Daemon::new("agent".to_string())
        .permissions("execution".to_string())
        .code(DaemonCode::Image {
            image: daemon_dir.to_string_lossy().into_owned(),
        })
        .build();
    let remote_daemon = Daemon::new("collector".to_string())
        .permissions("execution".to_string())
        .code(DaemonCode::Image {
            image: "registry.example.com/collector:latest".to_string(),
        })
        .build();

    let mut stack = Stack::new("daemon-push".to_string())
        .add(local_daemon, alien_core::ResourceLifecycle::Live)
        .add(remote_daemon, alien_core::ResourceLifecycle::Live)
        .build();

    let targets = collect_push_targets(&stack).unwrap();
    assert_eq!(
        targets.len(),
        1,
        "only the local-dir daemon is queued for push"
    );
    assert_eq!(targets[0].resource_names, vec!["agent".to_string()]);
    assert_eq!(targets[0].resource_type, "daemon");
    assert_eq!(targets[0].local_image_dir, daemon_dir);

    let updates = targets[0].push_result_updates("registry.example.com/agent:tag".into());
    apply_pushed_images(&mut stack, updates);
    let agent = stack
        .resources()
        .find(|(id, _)| *id == "agent")
        .and_then(|(_, e)| e.config.downcast_ref::<Daemon>().cloned())
        .expect("agent daemon should exist");
    assert_eq!(
        agent.code,
        DaemonCode::Image {
            image: "registry.example.com/agent:tag".to_string()
        }
    );

    // An unbuilt source daemon fails fast, same as workers and containers.
    let source_daemon = Daemon::new("raw".to_string())
        .permissions("execution".to_string())
        .code(DaemonCode::Source {
            src: ".".to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: "raw".to_string(),
            },
        })
        .build();
    let source_stack = Stack::new("daemon-source".to_string())
        .add(source_daemon, alien_core::ResourceLifecycle::Live)
        .build();
    let error = match collect_push_targets(&source_stack) {
        Err(error) => error,
        Ok(_) => panic!("source daemon must be rejected"),
    };
    assert!(error.to_string().contains("Run 'alien build' first"));
}

#[tokio::test]
async fn test_pull_and_export_alpine() {
    if !docker_available() {
        eprintln!("Skipping test_pull_and_export_alpine: docker not available");
        return;
    }

    tracing_subscriber::fmt::try_init().ok();

    let build_dir = tempdir().unwrap();
    let settings = BuildSettings {
        output_directory: build_dir.path().to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: Some(vec![BinaryTarget::LinuxX64]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    // Pull alpine:latest (small, always available)
    let result = pull_and_export_image(
        "alpine:latest",
        "test-alpine",
        "test-stack",
        &settings,
        build_dir.path(),
    )
    .await;

    assert!(
        result.is_ok(),
        "Should successfully pull and export alpine:latest"
    );

    let image_dir = result.unwrap();
    let image_path = PathBuf::from(&image_dir);

    // Verify directory exists and has content hash
    assert!(image_path.exists(), "Image directory should exist");
    assert!(
        image_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("test-alpine-"),
        "Directory should have content hash suffix"
    );

    // Verify OCI tarball was created
    let tarball_path = image_path.join("linux-x64.oci.tar");
    assert!(tarball_path.exists(), "OCI tarball should exist");

    // Verify tarball is valid OCI format
    let image = Image::from_tarball(&tarball_path).expect("OCI tarball should be valid");

    let metadata = image
        .get_metadata()
        .expect("Should be able to read image metadata");

    // Alpine has a CMD
    assert!(
        metadata.cmd.is_some() || metadata.entrypoint.is_some(),
        "Alpine image should have entrypoint or cmd"
    );
}

#[tokio::test]
async fn test_pull_nonexistent_image_fails() {
    if !docker_available() {
        eprintln!("Skipping test_pull_nonexistent_image_fails: docker not available");
        return;
    }

    tracing_subscriber::fmt::try_init().ok();

    let build_dir = tempdir().unwrap();
    let settings = BuildSettings {
        output_directory: build_dir.path().to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: Some(vec![BinaryTarget::LinuxX64]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    // Try to pull non-existent image
    let result = pull_and_export_image(
        "this-image-definitely-does-not-exist-xyz123:nonexistent",
        "test-nonexistent",
        "test-stack",
        &settings,
        build_dir.path(),
    )
    .await;

    // Should fail with docker pull error
    assert!(result.is_err(), "Should fail for non-existent image");
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("docker pull failed") || err_str.contains("not found"),
        "Error should mention docker pull failure: {}",
        err_str
    );
}

#[tokio::test]
async fn test_pull_and_export_produces_hash() {
    if !docker_available() {
        eprintln!("Skipping test_pull_and_export_produces_hash: docker not available");
        return;
    }

    tracing_subscriber::fmt::try_init().ok();

    let build_dir = tempdir().unwrap();
    let settings = BuildSettings {
        output_directory: build_dir.path().to_str().unwrap().to_string(),
        platform: PlatformBuildSettings::Test {},
        targets: Some(vec![BinaryTarget::LinuxX64]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
    };

    // Pull alpine image
    let result = pull_and_export_image(
        "alpine:latest",
        "test-alpine",
        "test-stack",
        &settings,
        build_dir.path(),
    )
    .await
    .expect("Pull should succeed");

    // Verify directory name has hash suffix
    let path = PathBuf::from(&result);
    let dir_name = path.file_name().unwrap().to_str().unwrap();

    // Should be in format: test-alpine-XXXXXXXX (8 char hash)
    assert!(
        dir_name.starts_with("test-alpine-"),
        "Should have container name prefix"
    );

    let hash_part = dir_name.strip_prefix("test-alpine-").unwrap();
    assert_eq!(hash_part.len(), 8, "Hash should be 8 characters");
    assert!(
        hash_part.chars().all(|c| c.is_ascii_hexdigit()),
        "Hash should be hexadecimal"
    );

    // Verify hash is based on tarball content
    // (Pulling same tag multiple times might get different content if image updated,
    // which is exactly why we hash - to detect changes!)
    let tarball_path = path.join("linux-x64.oci.tar");
    assert!(tarball_path.exists(), "Tarball should exist");
}

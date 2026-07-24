use super::*;

/// End-to-end: build two arches into one resource dir, push, and assert the pushed tag
/// resolves to a real multi-arch manifest list (not a single overwritten arch).
/// Gated on docker + a local registry (`docker run -d -p 5050:5000 registry:2`).
#[tokio::test]
async fn multiarch_push_produces_manifest_list() {
    use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

    const REGISTRY: &str = "localhost:5050";
    if !docker_available() {
        eprintln!("Skipping multiarch_push_produces_manifest_list: docker not available");
        return;
    }
    if !registry_available(&format!("http://{REGISTRY}")).await {
        eprintln!(
            "Skipping multiarch_push_produces_manifest_list: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
        );
        return;
    }

    let src = tempfile::tempdir().unwrap();
    let build_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        src.path().join("Dockerfile"),
        "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
    )
    .unwrap();

    // Build both linux arches into the same resource dir.
    for target in [BinaryTarget::LinuxArm64, BinaryTarget::LinuxX64] {
        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };
        let context = ToolchainContext {
            src_dir: src.path().to_path_buf(),
            build_dir: build_dir.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: target,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            workload: crate::toolchain::WorkloadKind::Container,
        };
        toolchain
            .build(&context)
            .await
            .expect("docker build should succeed");
    }
    assert!(build_dir.path().join("linux-aarch64.oci.tar").exists());
    assert!(build_dir.path().join("linux-x64.oci.tar").exists());

    let container = Container::new("web".to_string())
        .code(ContainerCode::Image {
            image: build_dir.path().to_string_lossy().into_owned(),
        })
        .cpu(alien_core::ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .permissions("web".to_string())
        .build();
    let stack = Stack::new("multiarch-test".to_string())
        .add(container, alien_core::ResourceLifecycle::Live)
        .build();

    let push_settings = PushSettings {
        repository: format!("{REGISTRY}/alien-multiarch-test"),
        destination_label: None,
        options: dockdash::PushOptions {
            auth: dockdash::RegistryAuth::Anonymous,
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        },
    };

    let pushed = push_stack(stack, Platform::Aws, &push_settings)
        .await
        .expect("push should succeed");

    let image_uri = pushed
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
        .find_map(|c| match &c.code {
            ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .expect("container should carry a pushed image URI");
    assert!(
        image_uri.contains(REGISTRY),
        "expected a registry URI, got {image_uri}"
    );

    // The pushed tag must resolve to an image index with both linux arches.
    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    });
    let reference = Reference::try_from(image_uri.as_str()).unwrap();
    let (bytes, _digest) = client
        .pull_manifest_raw(
            &reference,
            &dockdash::RegistryAuth::Anonymous,
            &[
                OCI_IMAGE_INDEX_MEDIA_TYPE,
                "application/vnd.docker.distribution.manifest.list.v2+json",
            ],
        )
        .await
        .expect("should pull a manifest list");
    let index: OciImageIndex =
        serde_json::from_slice(&bytes).expect("pushed tag should be an image index");
    let mut platforms: Vec<(String, String)> = index
        .manifests
        .iter()
        .filter_map(|m| {
            m.platform
                .as_ref()
                .map(|p| (p.os.clone(), p.architecture.clone()))
        })
        .collect();
    platforms.sort();
    assert_eq!(
        platforms,
        vec![
            ("linux".to_string(), "amd64".to_string()),
            ("linux".to_string(), "arm64".to_string()),
        ],
        "pushed tag must be a real multi-arch index"
    );
}

/// End-to-end: build a single arch into a resource dir, push, and assert the pushed tag
/// resolves to a plain image manifest (not an index). This is the path every current
/// single-platform release (aws/gcp/azure) takes, so the direct branch must stay intact.
/// Gated on docker + a local registry (`docker run -d -p 5050:5000 registry:2`).
#[tokio::test]
async fn singlearch_push_produces_single_manifest() {
    use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

    const REGISTRY: &str = "localhost:5050";
    if !docker_available() {
        eprintln!("Skipping singlearch_push_produces_single_manifest: docker not available");
        return;
    }
    if !registry_available(&format!("http://{REGISTRY}")).await {
        eprintln!(
            "Skipping singlearch_push_produces_single_manifest: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
        );
        return;
    }

    let src = tempfile::tempdir().unwrap();
    let build_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        src.path().join("Dockerfile"),
        "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
    )
    .unwrap();

    // Build a single linux arch into the resource dir.
    let toolchain = DockerToolchain {
        dockerfile: None,
        build_args: None,
        target: None,
    };
    let context = ToolchainContext {
        src_dir: src.path().to_path_buf(),
        build_dir: build_dir.path().to_path_buf(),
        cache_store: None,
        cache_prefix: "test".to_string(),
        build_target: BinaryTarget::LinuxArm64,
        runtime_platform_name: "aws".to_string(),
        debug_mode: false,
        workload: crate::toolchain::WorkloadKind::Container,
    };
    toolchain
        .build(&context)
        .await
        .expect("docker build should succeed");
    assert!(build_dir.path().join("linux-aarch64.oci.tar").exists());

    let container = Container::new("web".to_string())
        .code(ContainerCode::Image {
            image: build_dir.path().to_string_lossy().into_owned(),
        })
        .cpu(alien_core::ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .permissions("web".to_string())
        .build();
    let stack = Stack::new("singlearch-test".to_string())
        .add(container, alien_core::ResourceLifecycle::Live)
        .build();

    let push_settings = PushSettings {
        repository: format!("{REGISTRY}/alien-singlearch-test"),
        destination_label: None,
        options: dockdash::PushOptions {
            auth: dockdash::RegistryAuth::Anonymous,
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        },
    };

    let pushed = push_stack(stack, Platform::Aws, &push_settings)
        .await
        .expect("push should succeed");

    let image_uri = pushed
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
        .find_map(|c| match &c.code {
            ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .expect("container should carry a pushed image URI");
    assert!(
        image_uri.contains(REGISTRY),
        "expected a registry URI, got {image_uri}"
    );

    // The pushed tag must resolve to a plain image manifest, NOT an index: it has a
    // `config` descriptor and no `manifests` array.
    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    });
    let reference = Reference::try_from(image_uri.as_str()).unwrap();
    let (bytes, _digest) = client
        .pull_manifest_raw(
            &reference,
            &dockdash::RegistryAuth::Anonymous,
            &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
        )
        .await
        .expect("should pull a manifest");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("pushed tag should be valid JSON");
    assert!(
        value.get("config").is_some(),
        "single-arch push must produce an image manifest with a config descriptor, got: {value}"
    );
    assert!(
        value.get("manifests").is_none(),
        "single-arch push must not produce a manifest index, got: {value}"
    );
}

/// End-to-end seam: build two arches into two separate partial outputs (one per native
/// runner), run `merge_build_outputs` to combine them, load the merged stack exactly as
/// the release path does (deserialize stack.json), then push — asserting the merged dir
/// resolves to a real multi-arch index. This exercises the merge→load→push chain as one
/// flow, not as independent halves. Gated on docker + a local registry.
#[tokio::test]
async fn merge_then_push_produces_manifest_list() {
    use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};

    const REGISTRY: &str = "localhost:5050";
    if !docker_available() {
        eprintln!("Skipping merge_then_push_produces_manifest_list: docker not available");
        return;
    }
    if !registry_available(&format!("http://{REGISTRY}")).await {
        eprintln!(
            "Skipping merge_then_push_produces_manifest_list: no registry at {REGISTRY} (run: docker run -d -p 5050:5000 registry:2)"
        );
        return;
    }

    let src = tempfile::tempdir().unwrap();
    let input_root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::write(
        src.path().join("Dockerfile"),
        "FROM alpine:latest\nCMD [\"echo\", \"hi\"]\n",
    )
    .unwrap();

    // Build each arch into its own partial: <input>/<partial>/build/aws/<dir>/<tarball>,
    // with a stack.json whose code.image is that partial's absolute artifact dir — the
    // exact shape a native-runner `alien build --output-dir` upload produces.
    for (partial, target, dir_name) in [
        ("arm", BinaryTarget::LinuxArm64, "web-aaaa1111"),
        ("x64", BinaryTarget::LinuxX64, "web-bbbb2222"),
    ] {
        let platform_dir = input_root.path().join(partial).join("build").join("aws");
        let artifact_dir = platform_dir.join(dir_name);
        std::fs::create_dir_all(&artifact_dir).unwrap();

        let toolchain = DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        };
        let context = ToolchainContext {
            src_dir: src.path().to_path_buf(),
            build_dir: artifact_dir.clone(),
            cache_store: None,
            cache_prefix: "test".to_string(),
            build_target: target,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            workload: crate::toolchain::WorkloadKind::Container,
        };
        toolchain
            .build(&context)
            .await
            .expect("docker build should succeed");

        let image = artifact_dir
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let container = Container::new("web".to_string())
            .code(ContainerCode::Image { image })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("web".to_string())
            .build();
        let stack = Stack::new("merge-push-test".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();
        std::fs::write(
            platform_dir.join("stack.json"),
            serde_json::to_string_pretty(&stack).unwrap(),
        )
        .unwrap();
    }

    // Merge the two partials into one .alien.
    let platforms = crate::merge::merge_build_outputs(input_root.path(), out.path())
        .expect("merge should succeed");
    assert_eq!(platforms, vec!["aws".to_string()]);

    // Load the merged stack the way the release path does, then push it.
    let merged_json = std::fs::read_to_string(out.path().join("build/aws/stack.json")).unwrap();
    let merged_stack: Stack =
        serde_json::from_str(&merged_json).expect("merged stack.json should deserialize");

    let push_settings = PushSettings {
        repository: format!("{REGISTRY}/alien-merge-push-test"),
        destination_label: None,
        options: dockdash::PushOptions {
            auth: dockdash::RegistryAuth::Anonymous,
            protocol: dockdash::ClientProtocol::Http,
            ..Default::default()
        },
    };

    let pushed = push_stack(merged_stack, Platform::Aws, &push_settings)
        .await
        .expect("push of the merged stack should succeed");

    let image_uri = pushed
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
        .find_map(|c| match &c.code {
            ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .expect("container should carry a pushed image URI");

    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    });
    let reference = Reference::try_from(image_uri.as_str()).unwrap();
    let (bytes, _digest) = client
        .pull_manifest_raw(
            &reference,
            &dockdash::RegistryAuth::Anonymous,
            &[
                OCI_IMAGE_INDEX_MEDIA_TYPE,
                "application/vnd.docker.distribution.manifest.list.v2+json",
            ],
        )
        .await
        .expect("should pull a manifest list");
    let index: OciImageIndex =
        serde_json::from_slice(&bytes).expect("merged-then-pushed tag should be an image index");
    let mut platforms: Vec<(String, String)> = index
        .manifests
        .iter()
        .filter_map(|m| {
            m.platform
                .as_ref()
                .map(|p| (p.os.clone(), p.architecture.clone()))
        })
        .collect();
    platforms.sort();
    assert_eq!(
        platforms,
        vec![
            ("linux".to_string(), "amd64".to_string()),
            ("linux".to_string(), "arm64".to_string()),
        ],
        "merged stack must push as a real multi-arch index"
    );
}

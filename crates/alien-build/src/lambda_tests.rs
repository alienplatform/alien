use super::*;
use tempfile::tempdir;

#[tokio::test]
async fn aws_worker_source_cache_does_not_reuse_zstd_cloud_images() {
    let src = tempdir().unwrap();
    std::fs::create_dir(src.path().join("src")).unwrap();
    std::fs::write(
        src.path().join("Cargo.toml"),
        "[package]\nname=\"job\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(src.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let mut settings = BuildSettings {
        platform: PlatformBuildSettings::Gcp {},
        output_directory: src.path().display().to_string(),
        targets: Some(vec![BinaryTarget::LinuxArm64]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
        rebuild: false,
        pull_base_images: false,
    };
    let toolchain = ToolchainConfig::Rust {
        binary_name: "job".to_string(),
    };
    let gcp = compute_source_artifact_cache_key(
        src.path().to_str().unwrap(),
        &toolchain,
        &settings,
        &[BinaryTarget::LinuxArm64],
        toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    settings.platform = PlatformBuildSettings::Aws {
        managing_account_id: None,
    };
    let aws = compute_source_artifact_cache_key(
        src.path().to_str().unwrap(),
        &toolchain,
        &settings,
        &[BinaryTarget::LinuxArm64],
        toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    assert_ne!(
        aws, gcp,
        "a cached zstd image must not satisfy an AWS Worker build"
    );
}

async fn lambda_publish_archives(images: &Path, base: Option<&str>) {
    for target in [BinaryTarget::LinuxX64, BinaryTarget::LinuxArm64] {
        let layer = DockDashLayer::builder()
            .unwrap()
            .compression(dockdash::LayerCompression::Gzip)
            .data(
                "/var/task/handler.py",
                b"def handle(event, context):\n    return {\"ok\": True, \"echo\": event}\n",
                Some(0o644),
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        let mut builder = DockDashImage::builder()
            .platform("linux", &target.to_dockdash_arch())
            .layer(layer)
            .cmd(vec!["handler.handle".to_string()])
            .output_to(images.join(format!("{}.oci.tar", target.runtime_platform_id())));
        if let Some(base) = base {
            builder = builder.from(base);
        }
        builder.build().await.unwrap();
    }
}

#[tokio::test]
async fn aws_worker_publish_selects_arm64_but_containers_keep_both_architectures() {
    let registry = container_registry::ContainerRegistry::builder()
        .build_for_testing()
        .run_in_background();
    let repository = format!("localhost:{}/tests/worker", registry.bound_addr().port());
    let images = tempdir().unwrap();
    lambda_publish_archives(images.path(), None).await;
    let options = dockdash::PushOptions {
        auth: dockdash::RegistryAuth::Anonymous,
        protocol: dockdash::ClientProtocol::Http,
        monolithic_push: dockdash::MonolithicPushPolicy::Always,
        ..Default::default()
    };
    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    });
    for resource_type in ["worker", "container"] {
        let uri = push_resource_images(
            "job",
            "job",
            resource_type,
            images.path(),
            &Platform::Aws,
            &repository,
            &options,
        )
        .await
        .unwrap();
        let reference = Reference::try_from(uri.as_str()).unwrap();
        let (bytes, _) = client
            .pull_manifest_raw(
                &reference,
                &options.auth,
                &[
                    OCI_IMAGE_MEDIA_TYPE,
                    IMAGE_MANIFEST_MEDIA_TYPE,
                    "application/vnd.oci.image.index.v1+json",
                ],
            )
            .await
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if resource_type == "worker" {
            assert!(
                manifest.get("manifests").is_none(),
                "Lambda must receive a single image manifest"
            );
            let image = client
                .pull(
                    &reference,
                    &options.auth,
                    vec!["application/vnd.oci.image.layer.v1.tar+gzip"],
                )
                .await
                .unwrap();
            let config: serde_json::Value = serde_json::from_slice(&image.config.data).unwrap();
            assert_eq!(config["architecture"], "arm64");
            assert_eq!(image.layers.len(), 1);
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(
                image.layers[0].data.as_slice(),
            ));
            assert!(tar
                .entries()
                .unwrap()
                .any(|entry| entry.unwrap().path().unwrap().as_ref()
                    == Path::new("var/task/handler.py")));
        } else {
            assert_eq!(manifest["manifests"].as_array().unwrap().len(), 2);
        }
    }
}

#[tokio::test]
async fn aws_worker_publish_rejects_missing_arm64_before_upload() {
    let images = tempdir().unwrap();
    std::fs::write(images.path().join("linux-x64.oci.tar"), b"not an image").unwrap();
    let error = push_resource_images(
        "job",
        "job",
        "worker",
        images.path(),
        &Platform::Aws,
        "localhost:1/unused",
        &dockdash::PushOptions::default(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("Linux ARM64"));
}

#[tokio::test]
#[ignore = "requires Zig, cargo-zigbuild and network access to the Worker base image"]
async fn aws_worker_source_image_uses_gzip() {
    let src = tempdir().unwrap();
    let out = tempdir().unwrap();
    std::fs::create_dir(src.path().join("src")).unwrap();
    std::fs::write(
        src.path().join("Cargo.toml"),
        "[package]\nname=\"job\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(src.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let settings = BuildSettings {
        platform: PlatformBuildSettings::Aws {
            managing_account_id: None,
        },
        output_directory: out.path().display().to_string(),
        targets: Some(vec![BinaryTarget::LinuxArm64]),
        cache_url: None,
        override_base_image: None,
        debug_mode: false,
        rebuild: false,
        pull_base_images: false,
    };
    let archive = out.path().join("linux-aarch64.oci.tar");
    build_target_to_file(
        src.path().to_str().unwrap(),
        &ToolchainConfig::Rust {
            binary_name: "job".to_string(),
        },
        "job",
        "lambda-source",
        &settings,
        &BinaryTarget::LinuxArm64,
        &archive,
        toolchain::WorkloadKind::Worker,
    )
    .await
    .unwrap();
    let image = DockDashImage::from_tarball(&archive).unwrap();
    let binary = image.read_file("/app/job").await.unwrap().unwrap();
    assert_eq!(&binary[..4], b"\x7fELF");
    // Inspect the generated OCI manifest, including the application layer.
    let mut tar = tar::Archive::new(std::fs::File::open(&archive).unwrap());
    let mut blobs = std::collections::HashMap::new();
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        blobs.insert(path, bytes);
    }
    let index: serde_json::Value = serde_json::from_slice(&blobs["index.json"]).unwrap();
    let digest = index["manifests"][0]["digest"]
        .as_str()
        .unwrap()
        .strip_prefix("sha256:")
        .unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&blobs[&format!("blobs/sha256/{digest}")]).unwrap();
    assert!(
        manifest["layers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|layer| layer["mediaType"].as_str().unwrap().ends_with("gzip")),
        "every layer must be Lambda-compatible"
    );
}

/// Run against a disposable ECR repository, then create and invoke a Lambda
/// with the printed image reference. This exercises the production publisher
/// with both Linux architectures and a real Lambda runtime base.
#[tokio::test]
#[ignore = "requires a disposable ECR repository and ECR credentials"]
async fn lambda_publish_acceptance_image() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("dockdash=info,oci_client=info")
        .try_init();
    let repository = std::env::var("ALIEN_TEST_LAMBDA_PUBLISH_REPOSITORY").unwrap();
    let password = std::env::var("ALIEN_TEST_LAMBDA_PUBLISH_PASSWORD").unwrap();
    let images = tempdir().unwrap();
    lambda_publish_archives(images.path(), Some("public.ecr.aws/lambda/python:3.12")).await;
    let options = dockdash::PushOptions {
        auth: dockdash::RegistryAuth::Basic("AWS".to_string(), password),
        ..Default::default()
    };
    let uri = push_resource_images(
        "lambda-acceptance",
        "lambda-acceptance",
        "worker",
        images.path(),
        &Platform::Aws,
        &repository,
        &options,
    )
    .await
    .unwrap();
    println!("LAMBDA_ACCEPTANCE_IMAGE={uri}");
}

#[tokio::test]
#[ignore = "requires Docker, a disposable ECR repository and ECR credentials"]
async fn lambda_docker_publish_acceptance_image() {
    use crate::toolchain::{docker::DockerToolchain, Toolchain, ToolchainContext};
    let src = tempdir().unwrap();
    let images = tempdir().unwrap();
    std::fs::write(src.path().join("Dockerfile"), "FROM public.ecr.aws/lambda/python:3.12\nCOPY handler.py /var/task/handler.py\nCMD [\"handler.handle\"]\n").unwrap();
    std::fs::write(
        src.path().join("handler.py"),
        "def handle(event, context):\n    return {\"ok\": True, \"echo\": event}\n",
    )
    .unwrap();
    for target in [BinaryTarget::LinuxX64, BinaryTarget::LinuxArm64] {
        DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        }
        .build(&ToolchainContext {
            src_dir: src.path().to_path_buf(),
            build_dir: images.path().to_path_buf(),
            cache_store: None,
            cache_prefix: "lambda-acceptance".to_string(),
            build_target: target,
            runtime_platform_name: "aws".to_string(),
            debug_mode: false,
            pull_base_images: false,
            workload: toolchain::WorkloadKind::Worker,
        })
        .await
        .unwrap();
    }
    let repository = std::env::var("ALIEN_TEST_LAMBDA_PUBLISH_REPOSITORY").unwrap();
    let options = dockdash::PushOptions {
        auth: dockdash::RegistryAuth::Basic(
            "AWS".to_string(),
            std::env::var("ALIEN_TEST_LAMBDA_PUBLISH_PASSWORD").unwrap(),
        ),
        ..Default::default()
    };
    let uri = push_resource_images(
        "lambda-docker-acceptance",
        "lambda-docker-acceptance",
        "worker",
        images.path(),
        &Platform::Aws,
        &repository,
        &options,
    )
    .await
    .unwrap();
    println!("LAMBDA_ACCEPTANCE_IMAGE={uri}");
}

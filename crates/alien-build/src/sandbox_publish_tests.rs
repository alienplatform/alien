use super::*;
use tempfile::tempdir;

async fn archive(directory: &Path, target: BinaryTarget, contents: &[u8]) {
    let layer = DockDashLayer::builder()
        .unwrap()
        .compression(dockdash::LayerCompression::Gzip)
        .data("/app/payload", contents, Some(0o644))
        .unwrap()
        .build()
        .await
        .unwrap();
    DockDashImage::builder()
        .platform("linux", &target.to_dockdash_arch())
        .layer(layer)
        .output_to(directory.join(format!("{}.oci.tar", target.runtime_platform_id())))
        .build()
        .await
        .unwrap();
}

/// Publish through the real registry protocol and pin the result by digest, as an AWS release
/// does with the image the deployment's bundle names in its `FROM`. Fresh push tags must not
/// manufacture a new sandbox build for identical ARM64 bytes.
#[tokio::test]
async fn repeated_sandbox_pushes_preserve_the_arm64_image_identity() {
    let registry = container_registry::ContainerRegistry::builder()
        .build_for_testing()
        .run_in_background();
    let repository = format!("localhost:{}/tests/sandbox", registry.bound_addr().port());
    let images = tempdir().unwrap();
    archive(images.path(), BinaryTarget::LinuxArm64, b"arm-v1").await;
    archive(images.path(), BinaryTarget::LinuxX64, b"x64-v1").await;
    let options = dockdash::PushOptions {
        auth: dockdash::RegistryAuth::Anonymous,
        protocol: dockdash::ClientProtocol::Http,
        monolithic_push: dockdash::MonolithicPushPolicy::Always,
        ..Default::default()
    };
    let publish = || async {
        let pushed = push_resource_images(
            "sandbox",
            "sandbox",
            "sandbox",
            images.path(),
            &Platform::Aws,
            &repository,
            &options,
        )
        .await?;
        pin_to_digest("sandbox", &pushed, &options).await
    };
    let first = publish().await.unwrap();
    let second = publish().await.unwrap();
    assert_eq!(
        first, second,
        "the same ARM64 image pushed under fresh tags must keep one reference"
    );
    assert!(first.starts_with(&format!("{repository}@sha256:")));
    let client = OciClient::new(OciClientConfig {
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    });
    let pulled = client
        .pull(
            &Reference::try_from(first.as_str()).unwrap(),
            &options.auth,
            vec!["application/vnd.oci.image.layer.v1.tar+gzip"],
        )
        .await
        .unwrap();
    let config: serde_json::Value = serde_json::from_slice(&pulled.config.data).unwrap();
    assert_eq!(config["architecture"], "arm64");
    assert_eq!(config["os"], "linux");

    archive(images.path(), BinaryTarget::LinuxX64, b"x64-v2").await;
    assert_eq!(
        publish().await.unwrap(),
        first,
        "an unused architecture cannot roll the sandbox"
    );
    archive(images.path(), BinaryTarget::LinuxArm64, b"arm-v2").await;
    assert_ne!(
        publish().await.unwrap(),
        first,
        "changed executable content requires a new version"
    );
}

use super::*;
use crate::sandbox_bundle::{write_bundle, AgentSource};
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

/// Publish through the real registry protocol and render the actual build bundle.
/// Fresh push tags must not manufacture a new sandbox build for identical ARM64 bytes.
#[tokio::test]
async fn repeated_sandbox_pushes_preserve_the_arm64_bundle_identity() {
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
    let publish = || {
        push_resource_images(
            "sandbox",
            "sandbox",
            "sandbox",
            images.path(),
            &Platform::Aws,
            &repository,
            &options,
        )
    };
    let first = publish().await.unwrap();
    let second = publish().await.unwrap();
    let agent = AgentSource::Image("example.com/agent:v1".to_string());
    let first_bundle = images.path().join("first.zip");
    let second_bundle = images.path().join("second.zip");
    write_bundle(&first_bundle, &first, &agent).unwrap();
    write_bundle(&second_bundle, &second, &agent).unwrap();
    assert_eq!(
        std::fs::read(&first_bundle).unwrap(),
        std::fs::read(&second_bundle).unwrap(),
        "the same ARM64 image pushed under fresh tags must produce identical bundle bytes"
    );
    assert_eq!(first, second);
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
    let changed = publish().await.unwrap();
    assert_ne!(
        changed, first,
        "changed executable content requires a new version"
    );
    write_bundle(&second_bundle, &changed, &agent).unwrap();
    assert_ne!(
        std::fs::read(&first_bundle).unwrap(),
        std::fs::read(&second_bundle).unwrap()
    );
    write_bundle(
        &second_bundle,
        &first,
        &AgentSource::Image("example.com/agent:v2".to_string()),
    )
    .unwrap();
    assert_ne!(
        std::fs::read(&first_bundle).unwrap(),
        std::fs::read(&second_bundle).unwrap(),
        "an agent upgrade must remain a real build change"
    );
}

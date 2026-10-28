//! Cold-build checks use their own builder and subprocess environment.
use super::DockerToolchain;
use crate::toolchain::{Toolchain, ToolchainContext, WorkloadKind};
use alien_core::BinaryTarget;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    fs::{self, File, FileTimes},
    path::Path,
    process::Command,
    time::{Duration, UNIX_EPOCH},
};

#[tokio::test]
async fn cold_docker_builds_preserve_unchanged_image_digests() {
    if !Command::new("docker")
        .arg("info")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("Skipping cold image build test: Docker is not available");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let name = format!(
        "image-repro-{}",
        directory
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
    );
    DockerToolchain::create_builder(&name, "default")
        .await
        .unwrap();
    let result = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "toolchain::docker::repro_tests::cold_build_subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env("BUILDX_BUILDER", &name)
        .env("IMAGE_REPRO_TEST", directory.path())
        .env_remove("SOURCE_DATE_EPOCH")
        .output();
    let removed = Command::new("docker")
        .args(["buildx", "rm", "--force", &name])
        .output()
        .unwrap();
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let result = result.unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn archive_json(archive: &Path, name: &str) -> Value {
    let mut archive = tar::Archive::new(File::open(archive).unwrap());
    let entry = archive
        .entries()
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.path().unwrap() == Path::new(name))
        .unwrap();
    serde_json::from_reader(entry).unwrap()
}

#[tokio::test]
#[ignore = "Subprocess entry point invoked by the cold-build test"]
async fn cold_build_subprocess() {
    let root = env::var_os("IMAGE_REPRO_TEST").unwrap();
    let root = Path::new(&root);
    let source = root.join("source");
    let output = root.join("output");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&output).unwrap();
    fs::write(
        source.join("Dockerfile"),
        "FROM scratch\nCOPY app /app\nCMD [\"/app\"]\n",
    )
    .unwrap();
    let file = source.join("app");
    fs::write(&file, b"unchanged bytes").unwrap();
    let target = BinaryTarget::linux_container_target();
    let context = ToolchainContext {
        src_dir: source,
        build_dir: output.clone(),
        cache_store: None,
        cache_prefix: "image-repro".into(),
        build_target: target,
        runtime_platform_name: "kubernetes".into(),
        debug_mode: false,
        pull_base_images: false,
        workload: WorkloadKind::Container,
    };
    let mut digests = Vec::new();
    for index in 0..4 {
        if index == 2 {
            fs::write(&file, b"changed bytes").unwrap();
        }
        File::open(&file)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(UNIX_EPOCH + Duration::from_secs(10000 + index * 1000)),
            )
            .unwrap();
        // Only this test's builder is pruned; warm layers must not hide a regression.
        let pruned = Command::new("docker")
            .args([
                "buildx",
                "prune",
                "--builder",
                &env::var("BUILDX_BUILDER").unwrap(),
                "--all",
                "--force",
            ])
            .output()
            .unwrap();
        assert!(
            pruned.status.success(),
            "{}",
            String::from_utf8_lossy(&pruned.stderr)
        );
        let toolchain = DockerToolchain {
            dockerfile: None,
            target: None,
            build_args: (index == 3)
                .then(|| HashMap::from([("SOURCE_DATE_EPOCH".into(), "1234".into())])),
        };
        toolchain.build(&context).await.unwrap();
        let archive = output.join(format!("{}.oci.tar", target.runtime_platform_id()));
        let index_json = archive_json(&archive, "index.json");
        let digest = index_json["manifests"][0]["digest"].as_str().unwrap();
        let manifest = archive_json(&archive, &format!("blobs/{}", digest.replace(':', "/")));
        let config = archive_json(
            &archive,
            &format!(
                "blobs/{}",
                manifest["config"]["digest"]
                    .as_str()
                    .unwrap()
                    .replace(':', "/")
            ),
        );
        assert_eq!(
            config["created"],
            if index == 3 {
                "1970-01-01T00:20:34Z"
            } else {
                "1970-01-01T00:00:00Z"
            }
        );
        digests.push(digest.to_string());
    }
    assert_eq!(
        digests[0], digests[1],
        "checkout timestamps must not change the image"
    );
    assert_ne!(
        digests[1], digests[2],
        "changed contents must change the image"
    );
    assert_ne!(
        digests[2], digests[3],
        "explicit source timestamps must be honored"
    );
}

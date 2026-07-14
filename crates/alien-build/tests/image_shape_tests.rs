//! ALIEN-225 image-shape and native-binding build tests.
//!
//! Asserts the final build model on real `bun build --compile` outputs:
//!
//! - Worker source images bundle `alien-worker-runtime` (the base image's
//!   entrypoint) with `CMD ["--", "./<bin>"]`;
//! - Container/Daemon source images set the compiled binary as the DIRECT
//!   image entrypoint — no runtime, no `--` separator, no CMD;
//! - a compiled TypeScript Container with a staged native bindings addon
//!   actually SERVES HTTP and round-trips a real local kv binding (the
//!   compile-smoke pattern from `packages/package-layout/steps/compile.ts`,
//!   exercised through the real `TypeScriptToolchain` staging path).
//!
//! The image-shape test needs network access to pull base images (the same
//! requirement as the existing `typescript_integration_tests`). The binding
//! test needs the bindings package dist (`pnpm --dir packages/bindings run
//! build`) and the dev addon (`pnpm exec napi build --platform --release` in
//! `crates/alien-bindings-node`).

use alien_build::build_stack;
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_build::toolchain::typescript::TypeScriptToolchain;
use alien_build::toolchain::{ImageBuildStrategy, Toolchain, ToolchainContext, WorkloadKind};
use alien_core::permissions::PermissionProfile;
use alien_core::{
    BinaryTarget, Container, ContainerCode, Daemon, DaemonCode, ResourceLifecycle, Worker,
    WorkerCode,
};
use dockdash::Image;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use tokio::process::Command as TokioCommand;

fn workspace_root() -> PathBuf {
    workspace_root::get_workspace_root()
}

fn bun_available() -> bool {
    Command::new("bun").arg("--version").output().is_ok()
}

/// Write a minimal TypeScript project (`package.json` + `index.ts`).
fn write_project(dir: &Path, name: &str, entry_source: &str) {
    std::fs::create_dir_all(dir).expect("create project dir");
    std::fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "main": "./index.ts",
        }))
        .unwrap(),
    )
    .expect("write package.json");
    std::fs::write(dir.join("index.ts"), entry_source).expect("write index.ts");
}

/// Link the built `@alienplatform/sdk` into the project's node_modules —
/// required by the generated Worker bootstrap.
fn install_sdk_package(project_dir: &Path) {
    link_workspace_package(project_dir, "packages/sdk", "@alienplatform/sdk");
}

/// Link the built `@alienplatform/bindings` into the project's node_modules.
fn install_bindings_package(project_dir: &Path) {
    link_workspace_package(project_dir, "packages/bindings", "@alienplatform/bindings");
}

/// Symlink a real workspace package into the app's node_modules (the shape a
/// pnpm-linked install produces). Node/bun resolution follows the symlink to
/// the real location, so the package's own workspace node_modules resolve its
/// transitive dependencies.
fn link_workspace_package(project_dir: &Path, workspace_rel: &str, package_name: &str) {
    let src = workspace_root().join(workspace_rel);
    assert!(
        src.join("dist").is_dir(),
        "{} has no dist/ — run pnpm install (workspace prepare builds it) first",
        src.display()
    );
    let dest = project_dir.join("node_modules").join(package_name);
    std::fs::create_dir_all(dest.parent().expect("scope dir")).expect("create node_modules");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&src, &dest).expect("link workspace package");
    #[cfg(not(unix))]
    panic!("these tests require a unix host");
}

fn find_image_tarball(output_dir: &Path, resource_name: &str, target: BinaryTarget) -> PathBuf {
    let prefix = format!("{resource_name}-");
    for entry in std::fs::read_dir(output_dir).expect("read build output dir") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() && name.starts_with(&prefix) {
            let tarball = path.join(format!("{}.oci.tar", target.runtime_platform_id()));
            assert!(
                tarball.is_file(),
                "expected OCI tarball at {}",
                tarball.display()
            );
            return tarball;
        }
    }
    panic!(
        "no image directory with prefix '{prefix}' in {}",
        output_dir.display()
    );
}

/// One TS stack with a Worker, a Container, and a Daemon, all built from
/// source for linux-x64; asserts the OCI image metadata per compute type.
#[tokio::test]
async fn typescript_source_image_shapes_per_compute_type() {
    if !bun_available() {
        eprintln!("Skipping typescript_source_image_shapes_per_compute_type: bun not available");
        return;
    }
    std::env::set_var("ALIEN_SKIP_DEPENDENCY_INSTALL", "1");
    tracing_subscriber::fmt::try_init().ok();

    let src_root = tempdir().expect("temp src dir");

    // Worker: needs a default export and the SDK for the generated bootstrap.
    let worker_dir = src_root.path().join("worker-app");
    write_project(
        &worker_dir,
        "worker-app",
        "export default { name: 'worker-app' };",
    );
    install_sdk_package(&worker_dir);

    // Container/Daemon: plain apps, compiled directly (no bootstrap, no SDK).
    let container_dir = src_root.path().join("container-app");
    write_project(
        &container_dir,
        "container-app",
        "console.log('container-app up');",
    );
    let daemon_dir = src_root.path().join("daemon-app");
    write_project(&daemon_dir, "daemon-app", "console.log('daemon-app up');");

    let worker = Worker::new("shape-worker".to_string())
        .code(WorkerCode::Source {
            src: worker_dir.to_string_lossy().into_owned(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("worker-bin".to_string()),
            },
        })
        .memory_mb(512)
        .timeout_seconds(60)
        .permissions("execution".to_string())
        .build();
    let container = Container::new("shape-container".to_string())
        .code(ContainerCode::Source {
            src: container_dir.to_string_lossy().into_owned(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("container-bin".to_string()),
            },
        })
        .cpu(alien_core::ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .permissions("execution".to_string())
        .build();
    let daemon = Daemon::new("shape-daemon".to_string())
        .code(DaemonCode::Source {
            src: daemon_dir.to_string_lossy().into_owned(),
            toolchain: alien_core::ToolchainConfig::TypeScript {
                binary_name: Some("daemon-bin".to_string()),
            },
        })
        .permissions("execution".to_string())
        .build();

    let stack = alien_core::Stack::new("shape-stack".to_string())
        .permission("execution", PermissionProfile::new())
        .add(worker, ResourceLifecycle::Live)
        .add(container, ResourceLifecycle::Live)
        .add(daemon, ResourceLifecycle::Live)
        .build();

    let output_dir = tempdir().expect("temp output dir");
    // Match the host architecture (Depot CI runners and dev macs are arm64;
    // x64 hosts stay x64): the staged addon is the HOST-built dev addon, so a
    // hardcoded x64 target can never be satisfied on an arm64 machine.
    let target = BinaryTarget::linux_container_target();
    // E2E builds the branch's alien-base image before exercising image
    // assembly. Accept that immutable image here when supplied so the test
    // validates the code under review instead of the independently-published
    // `latest` tag.
    let worker_base_image = std::env::var("ALIEN_OVERRIDE_BASE_IMAGE")
        .ok()
        .filter(|image| !image.is_empty());
    let settings = BuildSettings {
        output_directory: output_dir.path().to_string_lossy().into_owned(),
        platform: PlatformBuildSettings::Test {},
        targets: Some(vec![target]),
        cache_url: None,
        override_base_image: worker_base_image,
        debug_mode: false,
    };

    build_stack(stack, &settings)
        .await
        .expect("stack build should succeed");

    let platform_dir = output_dir.path().join("build").join("test");

    // Worker image: alien-base entrypoint (the runtime) + separator CMD.
    let worker_tarball = find_image_tarball(&platform_dir, "shape-worker", target);
    let worker_meta = Image::from_tarball(&worker_tarball)
        .expect("worker image should load")
        .get_metadata()
        .expect("worker image metadata");
    let worker_entrypoint = worker_meta
        .entrypoint
        .as_deref()
        .expect("Worker images must keep the runtime entrypoint");
    assert_eq!(
        worker_entrypoint,
        &["/app/alien-worker-runtime".to_string()],
        "Worker entrypoint must use the renamed runtime binary"
    );
    assert_eq!(
        worker_meta.cmd.as_deref(),
        Some(&["--".to_string(), "./worker-bin".to_string()][..]),
        "Worker CMD must be the -- separator plus the app binary"
    );

    // Container/Daemon images: the compiled binary IS the entrypoint.
    for (resource, binary) in [
        ("shape-container", "container-bin"),
        ("shape-daemon", "daemon-bin"),
    ] {
        let tarball = find_image_tarball(&platform_dir, resource, target);
        let meta = Image::from_tarball(&tarball)
            .unwrap_or_else(|e| panic!("{resource} image should load: {e}"))
            .get_metadata()
            .unwrap_or_else(|e| panic!("{resource} image metadata: {e}"));
        assert_eq!(
            meta.entrypoint.as_deref(),
            Some(&[format!("/app/{binary}")][..]),
            "{resource}: the compiled binary must be the direct entrypoint"
        );
        assert!(
            meta.cmd.as_deref().is_none_or(|cmd| cmd.is_empty()),
            "{resource}: no CMD and no -- separator, got {:?}",
            meta.cmd
        );
        assert!(
            meta.entrypoint
                .iter()
                .flatten()
                .chain(meta.cmd.iter().flatten())
                .all(|part| !part.contains("alien-worker-runtime") && part != "--"),
            "{resource}: direct images must not reference the runtime or the separator"
        );
    }
}

/// Compile a TypeScript Container that uses the ordinary
/// `@alienplatform/bindings` entry and Bun's `export default { fetch }` server
/// convention through the real toolchain. Then run the binary from a different
/// directory and probe it over HTTP. This proves the generated wrapper both
/// registers the embedded addon before app evaluation and preserves Bun's
/// entry-module server lifecycle. The request also performs a real local-kv
/// put/get round-trip.
#[tokio::test]
async fn compiled_typescript_container_serves_local_binding() {
    if !bun_available() {
        eprintln!("Skipping compiled_typescript_container_serves_local_binding: bun not available");
        return;
    }
    std::env::set_var("ALIEN_SKIP_DEPENDENCY_INSTALL", "1");
    tracing_subscriber::fmt::try_init().ok();

    let host_target = BinaryTarget::current_os();
    let host_triple = match host_target {
        BinaryTarget::LinuxX64 => "linux-x64-gnu",
        BinaryTarget::LinuxArm64 => "linux-arm64-gnu",
        BinaryTarget::DarwinArm64 => "darwin-arm64",
        BinaryTarget::WindowsX64 => {
            eprintln!(
                "Skipping compiled_typescript_container_serves_local_binding: no windows addon"
            );
            return;
        }
    };

    // The dev addon must exist; the test wires it into the app's node_modules
    // as the TARGET prebuild package (@alienplatform/bindings-<triple>), so
    // the toolchain exercises the npm-install staging source.
    let addon_file_name = format!("alien-bindings-node.{host_triple}.node");
    let dev_addon = workspace_root()
        .join("crates/alien-bindings-node")
        .join(&addon_file_name);
    assert!(
        dev_addon.is_file(),
        "dev addon missing at {} — build it with `pnpm exec napi build --platform --release` in crates/alien-bindings-node",
        dev_addon.display()
    );

    let value = "hello-from-alien-build";
    let app_source = format!(
        r#"import {{ kv }} from "@alienplatform/bindings"

const store = kv("smoke-kv")
const seeded = store.set("smoke-key", "{value}")

export default {{
  async fetch(): Promise<Response> {{
    await seeded
    const got = await store.getText("smoke-key")
    if (got !== "{value}") {{
      return new Response(`MISMATCH: got ${{JSON.stringify(got)}}`, {{ status: 500 }})
    }}
    return new Response(`OK ${{got}}`)
  }}
}}
"#
    );

    let src_root = tempdir().expect("temp src dir");
    let app_dir = src_root.path().join("bindings-app");
    write_project(&app_dir, "bindings-app", &app_source);
    install_bindings_package(&app_dir);

    // Install the prebuild package for the host triple, backed by the dev addon.
    let prebuild_dir = app_dir
        .join("node_modules")
        .join(format!("@alienplatform/bindings-{host_triple}"));
    std::fs::create_dir_all(&prebuild_dir).expect("create prebuild dir");
    std::fs::copy(&dev_addon, prebuild_dir.join(&addon_file_name)).expect("install prebuild");

    // Build as a local Container: direct executable image, binary left in
    // build_dir and runnable on the host.
    let build_dir = tempdir().expect("temp build dir");
    let toolchain = TypeScriptToolchain {
        binary_name: Some("bindings-app".to_string()),
    };
    let context = ToolchainContext {
        src_dir: app_dir.clone(),
        build_dir: build_dir.path().to_path_buf(),
        cache_store: None,
        cache_prefix: "bindings-smoke".to_string(),
        build_target: host_target,
        runtime_platform_name: "local".to_string(),
        debug_mode: false,
        workload: WorkloadKind::Container,
    };

    let output = toolchain
        .build(&context)
        .await
        .expect("toolchain build should succeed");
    assert!(
        matches!(
            output.build_strategy,
            ImageBuildStrategy::FromBaseImage { .. }
        ),
        "local containers package from a direct-executable base image"
    );
    assert_eq!(
        output.entrypoint,
        Some(vec!["/app/bindings-app".to_string()])
    );
    assert!(output.runtime_command.is_empty());

    // The staged addon stays in place after the build: it is a shared
    // singleton path that concurrent builds (parallel containers in one
    // stack, parallel tests) embed simultaneously, so removing it would
    // yank it out from under another compile. The binary still carries its
    // own embedded copy — proven below by running it from an unrelated cwd.
    let staged = app_dir.join("node_modules/@alienplatform/bindings/dist/alien-bindings.node");
    assert!(
        staged.exists(),
        "staged addon should remain for concurrent builds"
    );

    // Run the compiled binary from an unrelated cwd against a real local kv,
    // and prove the default-export server actually listens.
    let binary = build_dir.path().join("bindings-app");
    assert!(binary.is_file(), "compiled binary should exist");
    let kv_data_dir = tempdir().expect("kv data dir");
    let run_cwd = tempdir().expect("run cwd");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve probe port");
    let port = listener.local_addr().expect("probe address").port();
    drop(listener);

    let mut child = TokioCommand::new(&binary)
        .current_dir(run_cwd.path())
        .kill_on_drop(true)
        .env("PORT", port.to_string())
        .env("ALIEN_DEPLOYMENT_TYPE", "local")
        .env(
            "ALIEN_SMOKE_KV_BINDING",
            serde_json::json!({
                "service": "local-kv",
                "dataDir": kv_data_dir.path().to_string_lossy(),
            })
            .to_string(),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("compiled binary should start");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{port}/");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let probe = loop {
        match client.get(&url).send().await {
            Ok(response) => {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|error| format!("failed to read response: {error}"));
                break Ok((status, body));
            }
            Err(error) => {
                if let Some(status) = child.try_wait().expect("check compiled binary") {
                    break Err(format!(
                        "compiled binary exited early with {status}: {error}"
                    ));
                }
                if tokio::time::Instant::now() >= deadline {
                    break Err(format!("HTTP probe timed out: {error}"));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };

    let _ = child.start_kill();
    let output = child
        .wait_with_output()
        .await
        .expect("collect compiled binary output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let (status, body) =
        probe.unwrap_or_else(|error| panic!("{error}\nstdout: {stdout}\nstderr: {stderr}"));
    assert!(
        status.is_success() && body == format!("OK {value}"),
        "unexpected HTTP response {status}: {body}\nstdout: {stdout}\nstderr: {stderr}"
    );
}

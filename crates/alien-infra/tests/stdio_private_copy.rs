//! The private copy of a controller binary `StdioTransport::verified` runs, and the environment
//! its process gets.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use alien_infra::step_protocol::{StdioTransport, Transport};
use sha2::{Digest, Sha256};

fn request() -> alien_infra::step_protocol::protocol::StepRequest {
    serde_json::from_value(serde_json::json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "d", "resourceId": "zone",
        "resourceType": "acme/dns-zone", "platform": "test", "resourcePrefix": "p",
        "initialSetupAuthority": "directSetup", "lifecycle": "live",
        "resource": { "type": "acme/dns-zone", "id": "zone" },
        "previousResource": null, "dependencies": {},
        "deploymentConfig": {
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" }
        },
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": "2099-01-01T00:00:00Z"
    }))
    .unwrap()
}

fn program(transport: &StdioTransport) -> PathBuf {
    let debug = format!("{transport:?}");
    let start = debug.find("program: \"").unwrap() + "program: \"".len();
    let end = start + debug[start..].find('"').unwrap();
    PathBuf::from(&debug[start..end])
}

fn write_exec(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The folder the tests put private copies under, apart from the binary's.
fn copies(dir: &Path) -> PathBuf {
    dir.join("copies")
}

fn runs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".run-")
        })
        .collect()
}

#[tokio::test]
async fn the_copy_is_private_under_its_folder_and_removed_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("controller-x");
    let bytes = b"#!/bin/sh\nexit 0\n";
    write_exec(&binary, bytes);
    let sha = format!("{:x}", Sha256::digest(bytes));
    let transport =
        StdioTransport::verified("acme", &binary, &sha, &copies(dir.path()), Vec::new())
            .await
            .unwrap();
    let copy = program(&transport);
    assert_ne!(copy, binary);
    let run_dir = copy.parent().unwrap();
    assert_eq!(run_dir.parent().unwrap(), copies(dir.path()));
    let dir_mode = std::fs::metadata(run_dir).unwrap().permissions().mode() & 0o777;
    let file_mode = std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "dir mode {dir_mode:o}");
    assert_eq!(file_mode, 0o500, "file mode {file_mode:o}");
    assert_eq!(std::fs::read(&copy).unwrap(), bytes);
    drop(transport);
    assert!(!run_dir.exists(), "the private copy outlived its transport");
}

#[tokio::test]
async fn a_mismatched_binary_is_refused_and_leaves_no_copy() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("controller-x");
    write_exec(&binary, b"#!/bin/sh\necho evil\n");
    let sha = format!("{:x}", Sha256::digest(b"#!/bin/sh\nexit 0\n"));
    let error = StdioTransport::verified("acme", &binary, &sha, &copies(dir.path()), Vec::new())
        .await
        .err()
        .expect("refused");
    assert!(error.to_string().contains("acme"), "{error}");
    assert!(runs(&copies(dir.path())).is_empty());
}

#[tokio::test]
async fn a_read_only_folder_fails_with_an_error_naming_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("controller-x");
    let bytes = b"#!/bin/sh\nexit 0\n";
    write_exec(&binary, bytes);
    std::fs::create_dir_all(copies(dir.path())).unwrap();
    std::fs::set_permissions(copies(dir.path()), std::fs::Permissions::from_mode(0o555)).unwrap();
    let sha = format!("{:x}", Sha256::digest(bytes));
    let result =
        StdioTransport::verified("acme", &binary, &sha, &copies(dir.path()), Vec::new()).await;
    std::fs::set_permissions(copies(dir.path()), std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = result.err().expect("a read-only folder fails");
    assert!(error.to_string().contains("acme"), "{error}");
}

#[tokio::test]
async fn the_child_sees_the_host_network_settings_and_the_transports_env_but_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("env.txt");
    let binary = dir.path().join("controller-x");
    let bytes = format!(
        "#!/bin/sh\nenv > '{}'\nread line\necho '{{}}'\n",
        out.display()
    );
    write_exec(&binary, bytes.as_bytes());
    let sha = format!("{:x}", Sha256::digest(bytes.as_bytes()));
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "parent-secret");
    std::env::set_var("ALIEN_TOKEN", "parent-token");
    std::env::set_var("HTTPS_PROXY", "http://proxy.internal:3128");
    let transport = StdioTransport::verified(
        "acme",
        &binary,
        &sha,
        &copies(dir.path()),
        vec![("GIVEN".to_string(), "yes".to_string())],
    )
    .await
    .unwrap();
    let _ = transport.step(&request(), Duration::from_secs(10)).await;
    let env = std::fs::read_to_string(&out).unwrap();
    assert!(
        !env.contains("parent-secret") && !env.contains("parent-token"),
        "{env}"
    );
    assert!(
        env.contains("HTTPS_PROXY=http://proxy.internal:3128"),
        "{env}"
    );
    let names: Vec<&str> = env
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .collect();
    for name in &names {
        assert!(
            [
                "PATH",
                "HOME",
                "TMPDIR",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "no_proxy",
                "SSL_CERT_FILE",
                "SSL_CERT_DIR",
                "GIVEN",
                "PWD",
                "SHLVL",
                "_",
                "OLDPWD"
            ]
            .contains(name),
            "unexpected variable {name}"
        );
    }
    assert!(names.contains(&"GIVEN"));
}

/// A rebuild that empties the binary's folder, as `alien extensions build` empties
/// target/alien-extension, leaves the copy in its own folder, and the process still starts again.
#[tokio::test]
async fn a_process_starts_again_after_its_binarys_folder_is_emptied() {
    let dir = tempfile::tempdir().unwrap();
    let built = dir.path().join("built");
    std::fs::create_dir_all(&built).unwrap();
    let binary = built.join("controller-x");
    // Answers one line with a non-reply and exits, so the next step must start it again.
    let bytes = b"#!/bin/sh\nread line\nexit 0\n";
    write_exec(&binary, bytes);
    let sha = format!("{:x}", Sha256::digest(bytes));
    let transport =
        StdioTransport::verified("acme", &binary, &sha, &copies(dir.path()), Vec::new())
            .await
            .unwrap();
    let first = transport.step(&request(), Duration::from_secs(10)).await;
    assert!(
        format!("{first:?}").contains("exited before answering"),
        "{first:?}"
    );

    std::fs::remove_dir_all(&built).unwrap();

    let second = transport.step(&request(), Duration::from_secs(10)).await;
    assert!(
        format!("{second:?}").contains("exited before answering"),
        "{second:?}"
    );
}

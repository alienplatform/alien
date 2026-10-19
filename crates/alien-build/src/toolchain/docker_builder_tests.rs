//! Exercise builder selection in subprocesses so PATH/daemon settings cannot leak between tests.
use super::DockerToolchain;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command};
use tempfile::TempDir;

struct DockerFixture {
    directory: TempDir,
}

impl DockerFixture {
    fn new(scenario: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("docker");
        fs::write(&executable, include_str!("testdata/docker-builder.py")).unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o755)).unwrap();
        let builders = if scenario == "provider" {
            vec!["provider-owned"]
        } else {
            vec![]
        };
        fs::write(
            directory.path().join("state.json"),
            serde_json::to_vec(&json!({
                "scenario": scenario,
                "builders": builders,
                "calls": [],
                "creates": [],
                "initialMisses": 0,
                "bootstraps": 0
            }))
            .unwrap(),
        )
        .unwrap();
        Self { directory }
    }

    fn select(&self) -> Child {
        let mut paths = vec![self.directory.path().to_path_buf()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "toolchain::docker::builder_tests::selection_subprocess",
                "--ignored",
                "--nocapture",
            ])
            .env("PATH", env::join_paths(paths).unwrap())
            .env("ALIEN_BUILDER_SELECTION_TEST", "1")
            .env("DOCKER_HOST", "unix:///invalid-ambient-host.sock")
            .env("DOCKER_CONTEXT", "ambient-context")
            .env_remove("BUILDX_BUILDER")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.directory.path().join("state.json")).unwrap())
            .unwrap()
    }
}

fn selected(child: Child) -> String {
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "selection failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("SELECTED_BUILDER="))
        .expect("subprocess must report its selected builder")
        .to_owned()
}

#[tokio::test]
#[ignore = "Subprocess entry point invoked by the builder lifecycle tests"]
async fn selection_subprocess() {
    assert_eq!(env::var("ALIEN_BUILDER_SELECTION_TEST").unwrap(), "1");
    let builder = DockerToolchain::select_builder().await.unwrap();
    println!("SELECTED_BUILDER={}", builder.name);
}

#[test]
fn concurrent_first_users_share_the_created_builder() {
    let fixture = DockerFixture::new("concurrent");
    let first = fixture.select();
    let second = fixture.select();
    let first = selected(first);
    assert_eq!(first, selected(second));
    assert!(first.starts_with("alien-cache-v1-"));
    let state = fixture.state();
    assert_eq!(state["initialMisses"], 2);
    assert_eq!(state["creates"].as_array().unwrap().len(), 2);
    assert_eq!(
        state["creates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|create| create["success"] == true)
            .count(),
        1
    );
    assert_eq!(state["builders"], json!([first]));
    assert!(fixture.directory.path().join("worker").exists());
}

#[test]
fn selected_context_wins_over_ambient_docker_host() {
    let fixture = DockerFixture::new("context");
    let builder = selected(fixture.select());
    let state = fixture.state();
    assert_eq!(state["builders"], json!([builder]));
    assert_eq!(state["creates"][0]["endpoint"], "selected-context");
    assert!(state["calls"].as_array().unwrap().iter().any(|call| {
        call == &json!([
            "--context",
            "selected-context",
            "info",
            "--format",
            "{{.ID}}"
        ])
    }));
}

#[test]
fn missing_worker_is_recreated_without_replacing_builder() {
    let fixture = DockerFixture::new("default");
    let builder = selected(fixture.select());
    fs::remove_file(fixture.directory.path().join("worker")).unwrap();
    let before = fixture.state()["bootstraps"].as_u64().unwrap();
    assert_eq!(builder, selected(fixture.select()));
    let state = fixture.state();
    assert_eq!(state["creates"].as_array().unwrap().len(), 1);
    assert_eq!(state["builders"], json!([builder]));
    assert_eq!(state["bootstraps"].as_u64().unwrap(), before + 1);
    assert!(fixture.directory.path().join("worker").exists());
}

#[test]
fn selected_provider_builder_is_not_replaced() {
    let fixture = DockerFixture::new("provider");
    assert_eq!(selected(fixture.select()), "provider-owned");
    let state = fixture.state();
    assert_eq!(state["builders"], json!(["provider-owned"]));
    assert!(state["creates"].as_array().unwrap().is_empty());
    assert_eq!(state["bootstraps"], 1);
    assert_eq!(
        state["calls"],
        json!([
            ["buildx", "inspect"],
            ["buildx", "inspect", "--bootstrap", "provider-owned"]
        ])
    );
}

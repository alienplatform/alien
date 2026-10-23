#![cfg(all(feature = "platform", unix))]

//! A directory linked with `alien link --workspace <name>` keeps using that
//! workspace, even when the saved default workspace is a different one.

use httpmock::{Method::GET, MockServer};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const PROJECT_ID: &str = "prj_0000000000000000000000000001";
// Unsigned JWT that expires in 2100, so the CLI uses it without refreshing.
const ACCESS_TOKEN: &str = "eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9.sig";

/// A signed-in user whose saved default workspace is `personal`.
struct SignedInUser {
    home: TempDir,
}

impl SignedInUser {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let user = Self { home };
        let alien = user.config_dir().join("alien");
        fs::create_dir_all(&alien).unwrap();
        fs::write(
            alien.join("credentials.json"),
            json!({ "accessToken": ACCESS_TOKEN, "refreshToken": null }).to_string(),
        )
        .unwrap();
        fs::write(
            alien.join("profile.json"),
            json!({ "default_workspace": "personal" }).to_string(),
        )
        .unwrap();
        user
    }

    fn config_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home.path().join("Library/Application Support")
        } else {
            self.home.path().join(".config")
        }
    }

    fn run(&self, api: &MockServer, dir: &Path, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
        command
            .current_dir(dir)
            .args(["--base-url", &api.base_url()])
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_dir());
        for name in [
            "ALIEN_API_KEY",
            "ALIEN_MANAGER_URL",
            "ALIEN_PROJECT",
            "ALIEN_BASE_URL",
            "ALIEN_WORKSPACE",
            "ALIEN_LOG",
            "ALIEN_LOG_FILE",
            "RUST_LOG",
        ] {
            command.env_remove(name);
        }
        command.output().unwrap()
    }
}

fn project_body() -> Value {
    json!({
        "id": PROJECT_ID,
        "name": "my-worker",
        "createdAt": "2026-01-01T00:00:00Z",
        "workspaceId": "ws_000000000000000000000001"
    })
}

fn assert_success(output: &Output, step: &str) {
    assert!(
        output.status.success(),
        "{step} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn commands_in_a_directory_linked_to_another_workspace_use_the_linked_workspace() {
    let api = MockServer::start();
    let auth = format!("Bearer {ACCESS_TOKEN}");
    let project_by_name = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/projects/my-worker")
            .query_param("workspace", "acme")
            .header("authorization", &auth);
        then.status(200).json_body(project_body());
    });
    let project_by_id = api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/projects/{PROJECT_ID}"))
            .query_param("workspace", "acme")
            .header("authorization", &auth);
        then.status(200).json_body(project_body());
    });
    let releases = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/releases")
            .query_param("project", PROJECT_ID)
            .query_param("workspace", "acme")
            .header("authorization", &auth);
        then.status(200)
            .json_body(json!({ "items": [], "nextCursor": null }));
    });
    let default_workspace = api.mock(|when, then| {
        when.query_param("workspace", "personal");
        then.status(404).json_body(json!({
            "code": "NOT_FOUND", "message": "Not found", "retryable": false,
            "internal": false, "httpStatusCode": 404
        }));
    });

    let user = SignedInUser::new();
    let checkout = TempDir::new().unwrap();

    let link = user.run(
        &api,
        checkout.path(),
        &[
            "link",
            "--workspace",
            "acme",
            "--project",
            "my-worker",
            "--json",
        ],
    );
    assert_success(&link, "alien link");
    let link: Value = serde_json::from_slice(&link.stdout).unwrap();
    assert_eq!(link["workspace"], "acme");
    assert_eq!(link["projectId"], PROJECT_ID);

    let list = user.run(&api, checkout.path(), &["releases", "ls", "--json"]);
    assert_success(&list, "alien releases ls");
    let list: Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(list, json!([]));

    project_by_name.assert_hits(1);
    project_by_id.assert_hits(1);
    releases.assert_hits(1);
    default_workspace.assert_hits(0);
}

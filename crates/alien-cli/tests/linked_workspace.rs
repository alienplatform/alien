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

#[test]
fn workspace_commands_reject_invalid_links_before_requesting_the_saved_workspace() {
    let api = MockServer::start();
    let requests = api.mock(|_, then| {
        then.status(500);
    });
    let user = SignedInUser::new();
    let checkout = TempDir::new().unwrap();
    let link = checkout.path().join(".alien/project.json");
    fs::create_dir_all(link.parent().unwrap()).unwrap();

    for unreadable in [false, true] {
        if unreadable {
            fs::remove_file(&link).unwrap();
            fs::create_dir(&link).unwrap();
        } else {
            fs::write(&link, "{invalid json").unwrap();
        }
        for args in [vec!["whoami", "--json"], vec!["usage", "ai", "--json"]] {
            let command = args[0];
            let output = user.run(&api, checkout.path(), &args);
            assert!(!output.status.success(), "{command} should reject the link");
            let error: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(error["code"], "PROJECT_LINK_INVALID", "{error}");
        }
    }
    requests.assert_hits(0);
}

#[test]
fn whoami_uses_the_saved_default_until_a_link_or_flag_overrides_it() {
    let api = MockServer::start();
    let user = SignedInUser::new();
    let checkout = TempDir::new().unwrap();
    let mut requests = Vec::new();
    for workspace in ["personal", "acme", "explicit"] {
        requests.push(api.mock(|when, then| {
            when.method(GET)
                .path("/v1/whoami")
                .query_param("workspace", workspace);
            then.status(200).json_body(json!({
                "kind": "user",
                "id": "usr_000000000000000000000001",
                "email": "user@example.com",
                "role": "workspace.admin",
                "workspaceId": "ws_000000000000000000000001",
                "workspaceName": workspace
            }));
        }));
    }
    let relink_project = api.mock(|when, then| {
        when.method(GET)
            .path("/v1/projects/my-worker")
            .query_param("workspace", "explicit");
        then.status(200).json_body(project_body());
    });

    let output = user.run(&api, checkout.path(), &["whoami", "--json"]);
    assert_success(&output, "whoami with saved default");
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(identity["workspaceName"], "personal");

    let link = checkout.path().join(".alien/project.json");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    fs::write(
        &link,
        json!({
            "workspace": "acme", "projectId": PROJECT_ID, "projectName": "my-worker"
        })
        .to_string(),
    )
    .unwrap();
    let output = user.run(&api, checkout.path(), &["whoami", "--json"]);
    assert_success(&output, "whoami with linked workspace");
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(identity["workspaceName"], "acme");

    fs::write(&link, "{invalid json").unwrap();
    let output = user.run(
        &api,
        checkout.path(),
        &["whoami", "--workspace", "explicit", "--json"],
    );
    assert_success(&output, "whoami with explicit workspace and invalid link");
    let identity: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(identity["workspaceName"], "explicit");

    let output = user.run(
        &api,
        checkout.path(),
        &[
            "link",
            "--workspace",
            "explicit",
            "--project",
            "my-worker",
            "--json",
        ],
    );
    assert_success(
        &output,
        "recreate malformed link with an explicit workspace",
    );
    let repaired_link: Value = serde_json::from_slice(&fs::read(&link).unwrap()).unwrap();
    assert_eq!(repaired_link["workspace"], "explicit");
    assert_eq!(repaired_link["projectId"], PROJECT_ID);
    relink_project.assert_hits(1);
    for request in requests {
        request.assert_hits(1);
    }
}

#![cfg(feature = "platform")]

use httpmock::{
    Method::{GET, POST},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::process::{Command, Output};

const PROJECT: &str = "prj_0000000000000000000000000000";
const GROUP: &str = "dg_0000000000000000000000000000";
const LINK: &str = "https://portal.example.com/deploy#sl_test";

fn run_onboard(server: &MockServer, args: &[&str]) -> Output {
    let config = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_alien"))
        .args([
            "--base-url",
            &server.base_url(),
            "--workspace",
            "acme",
            "--project",
            "demo",
            "onboard",
            "jane-corp",
            "--setup-items",
            "models",
        ])
        .args(args)
        .current_dir(config.path())
        .env("HOME", config.path())
        .env("XDG_CONFIG_HOME", config.path())
        .env("ALIEN_API_KEY", "test-key")
        .env_remove("ALIEN_PROJECT")
        .env_remove("ALIEN_WORKSPACE")
        .env_remove("ALIEN_BASE_URL")
        .env_remove("RUST_LOG")
        .output()
        .unwrap()
}

/// The project, its available setup, and a setup link that only accepts the
/// given `allowed` list.
fn mock_platform(server: &MockServer, allowed: Value) -> Mock<'_> {
    server.mock(|when, then| {
        when.method(GET).path("/v1/projects/demo");
        then.json_body(json!({"id":PROJECT, "name":"demo",
            "workspaceId":"ws_000000000000000000000000", "createdAt":"2026-01-01T00:00:00Z"}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/projects/{PROJECT}/deployment-link-setup"));
        then.json_body(json!({"activeRelease":null, "supportedPlatforms":["aws"],
            "setupItems":["models"], "visiblePackageTypes":[], "visibleSetupMethods":[],
            "setupPackagesStatus":"ready"}));
    });
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/deployment-groups/setup-links")
            .json_body_partial(json!({"allowed": allowed}).to_string());
        then.json_body(json!({
            "token":"ax_test", "deploymentLink":LINK, "expiresAt":"2026-11-05T10:00:00Z",
            "deploymentGroup":{"id":GROUP, "name":"jane-corp", "externalId":"jane-corp", "accessApprover":"customer",
                "projectId":PROJECT, "workspaceId":"ws_000000000000000000000000",
                "createdAt":"2026-01-01T00:00:00Z"}
        }));
    })
}

#[test]
fn onboard_sends_allowed_entries_and_prints_who_can_open_the_link() {
    let server = MockServer::start();
    let link = mock_platform(&server, json!(["jane@acme.com", "@acme.com"]));

    let output = run_onboard(
        &server,
        &["--allow", "Jane@Acme.com", "--allow", "@acme.com", "--json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["deploymentGroupId"], GROUP);
    assert_eq!(value["deploymentLink"], LINK);
    assert_eq!(value["allowed"], json!(["jane@acme.com", "@acme.com"]));
    assert_eq!(value["expiresAt"], "2026-11-05T10:00:00Z");
    assert!(value["nextAction"]
        .as_str()
        .unwrap()
        .contains("a code sent to that address"));

    let output = run_onboard(
        &server,
        &["--allow", "Jane@Acme.com", "--allow", "@acme.com"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(LINK), "{stdout}");
    assert!(
        stdout.contains("It asks for their email and a code sent to it"),
        "{stdout}"
    );
    assert!(stdout.contains("jane@acme.com, @acme.com"), "{stdout}");
    assert!(stdout.contains("2026-11-05 10:00 UTC"), "{stdout}");
    link.assert_hits(2);
}

#[test]
fn onboard_without_allow_is_refused_before_creating_a_link() {
    let server = MockServer::start();
    let link = mock_platform(&server, json!([]));

    // With --json, and outside a terminal (the test's stdin is not one), nothing asks for it.
    for args in [&["--json"][..], &[][..]] {
        let output = run_onboard(&server, args);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Required with --json or outside a terminal: pass --allow"),
            "{stderr}"
        );
    }
    link.assert_hits(0);
}

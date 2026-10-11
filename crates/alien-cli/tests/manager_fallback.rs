#![cfg(feature = "platform")]

use httpmock::{Method::GET, MockServer};
use serde_json::json;
use std::{fs, process::Command};
use tempfile::TempDir;

#[test]
fn manager_discovery_warns_on_system_fallback_without_changing_json_or_routing() {
    for is_system in [true, false] {
        let discovery = MockServer::start();
        let manager = MockServer::start();
        let resolve = discovery.mock(|when, then| {
            when.method(GET)
                .path("/v1/resolve")
                .query_param("project", "prj_test")
                .query_param("platform", "aws")
                .query_param("workspace", "demo")
                .header("authorization", "Bearer test-key");
            then.status(200).json_body(json!({
                "managerUrl": manager.base_url(),
                "managerName": "Selected manager",
                "managerIsSystem": is_system,
                "managerCloud": "aws",
                "projectId": "prj_test"
            }));
        });
        let list = manager.mock(|when, then| {
            when.method(GET)
                .path("/v1/deployments")
                .header("authorization", "Bearer test-key");
            then.status(200)
                .json_body(json!({"items": [], "nextCursor": null}));
        });
        let checkout = TempDir::new().unwrap();
        fs::create_dir(checkout.path().join(".alien")).unwrap();
        fs::write(
            checkout.path().join(".alien/project.json"),
            json!({
                "workspace": "demo", "projectId": "prj_test", "projectName": "demo-project",
                "rootDirectory": null
            })
            .to_string(),
        )
        .unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_alien"))
            .current_dir(checkout.path())
            .args([
                "--base-url",
                &discovery.base_url(),
                "--workspace",
                "demo",
                "deployments",
                "list",
                "--json",
            ])
            .env("ALIEN_API_KEY", "test-key")
            .env_remove("ALIEN_MANAGER_URL")
            .env_remove("ALIEN_PROJECT")
            .env_remove("ALIEN_BASE_URL")
            .env_remove("ALIEN_LOG")
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        resolve.assert_hits(1);
        list.assert_hits(1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            json!([])
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        let warnings = stderr
            .lines()
            .filter(|line| line.contains("system manager"))
            .count();
        assert_eq!(warnings, usize::from(is_system), "stderr: {stderr}");
    }
}

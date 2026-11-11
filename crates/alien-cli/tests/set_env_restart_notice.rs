#![cfg(all(feature = "platform", target_os = "linux"))]

//! `alien deployments set-env` says which resources a change restarts before it asks for
//! confirmation, and never lists a container Alien adds itself for an untargeted variable.

use alien_core::{
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, Container, ContainerCode, Platform, Resource,
    ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    StackResourceState, StackState, Worker, WorkerCode,
};
use httpmock::{
    Method::{GET, PATCH, POST},
    Mock, MockServer,
};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const PROJECT: &str = "prj_0000000000000000000000000000";
const WORKSPACE_ID: &str = "ws_000000000000000000000000";
const DEPLOYMENT_ID: &str = "dep_setenvnotice0000000000000000";
const GROUP_ID: &str = "dg_customers0000000000000000000";
const RELEASE_ID: &str = "rel_0000000000000000000000000000";

fn container(id: &str) -> Resource {
    Resource::new(
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "example.invalid/app:1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .permissions("default".to_string())
            .build(),
    )
}

/// A stack with two application containers, a worker, an internet sandbox and the egress
/// gateway Alien installs for it.
fn stack_state() -> Value {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "example.invalid/base:1".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let worker = Worker::new("jobs".to_string())
        .code(WorkerCode::Image {
            image: "example.invalid/jobs:1".to_string(),
        })
        .permissions("default".to_string())
        .build();
    let mut state = StackState::new(Platform::Aws);
    for (id, resource_type, config) in [
        ("backend", "container", container("backend")),
        ("frontend", "container", container("frontend")),
        (
            SANDBOX_EGRESS_GATEWAY_ID,
            "container",
            container(SANDBOX_EGRESS_GATEWAY_ID),
        ),
        ("agents", "sandbox", Resource::new(sandbox)),
        ("jobs", "worker", Resource::new(worker)),
    ] {
        state.resources.insert(
            id.to_string(),
            StackResourceState::new_pending(
                resource_type.to_string(),
                config,
                Some(ResourceLifecycle::Live),
                vec![],
            ),
        );
    }
    serde_json::to_value(state).expect("stack state serializes")
}

fn deployment() -> Value {
    json!({
        "id": DEPLOYMENT_ID,
        "name": "acme",
        "status": "running",
        "projectId": PROJECT,
        "platform": "aws",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": GROUP_ID,
        "deploymentGroup": {"id": GROUP_ID, "name": "customers", "externalId": null},
        "purpose": "application",
        "stackSettings": {},
        "stackState": stack_state(),
        "releaseChannel": "production",
        "retryRequested": false,
        "updateBlocked": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": "mgr_0000000000000000000000000000",
        "workspaceId": WORKSPACE_ID,
        "desiredReleaseId": RELEASE_ID,
    })
}

/// Mocks the platform API the command calls. The returned mock is the save request.
fn mock_api(api: &MockServer) -> Mock<'_> {
    api.mock(|when, then| {
        when.method(GET)
            .path("/v1/whoami")
            .header("authorization", "Bearer test-key");
        then.status(200).json_body(json!({
            "kind": "serviceAccount", "id": "apikey_0000000000000000000000000000",
            "workspaceId": WORKSPACE_ID, "workspaceName": "demo",
            "scope": {"type": "project", "projectId": PROJECT}, "role": "project.admin"
        }));
    });
    api.mock(|when, then| {
        when.method(GET).path("/v1/projects/demo-project");
        then.status(200)
            .json_body(json!({"id": PROJECT, "name": "demo-project",
            "workspaceId": WORKSPACE_ID, "createdAt": "2026-01-01T00:00:00Z"}));
    });
    api.mock(|when, then| {
        when.method(GET)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}"));
        then.status(200).json_body(deployment());
    });
    api.mock(|when, then| {
        when.method(GET).path(format!(
            "/v1/deployments/{DEPLOYMENT_ID}/environment-variables"
        ));
        then.status(200).json_body(json!({"variables": []}));
    });
    // An older platform has no value-access route, so values go in the PATCH body.
    api.mock(|when, then| {
        when.method(POST)
            .path(format!("/v1/deployments/{DEPLOYMENT_ID}/value-access"));
        then.status(404).body("404 Not Found");
    });
    api.mock(|when, then| {
        when.method(PATCH).path(format!(
            "/v1/deployments/{DEPLOYMENT_ID}/environment-variables"
        ));
        then.status(202)
            .header("content-type", "application/json")
            .json_body(json!({"message": "Environment variables updated."}));
    })
}

fn run_set_env(server: &MockServer, args: &[&str]) -> Output {
    let home = TempDir::new().expect("home dir");
    let checkout = TempDir::new().expect("checkout dir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_alien"));
    command
        .current_dir(checkout.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("ALIEN_API_KEY", "test-key");
    for name in [
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
    command
        .args(["--base-url", &server.base_url()])
        .args(["--project", "demo-project"])
        .args(["deployments", "set-env", DEPLOYMENT_ID, "--yes"])
        .args(args);
    command.output().expect("alien runs")
}

fn stdout(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn untargeted_variable_lists_application_resources_and_not_the_gateway() {
    let api = MockServer::start();
    let save = mock_api(&api);

    let output = run_set_env(&api, &["--env", "FEATURE=on"]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    save.assert_hits(1);
    assert!(
        text.contains("restart when their environment changes"),
        "{text}"
    );
    for listed in [
        "backend (container)",
        "frontend (container)",
        "jobs (worker)",
    ] {
        assert!(text.contains(listed), "missing {listed}: {text}");
    }
    assert!(
        !text.contains(SANDBOX_EGRESS_GATEWAY_ID),
        "the gateway never receives an untargeted variable: {text}"
    );
    assert!(!text.contains("agents (sandbox)"), "{text}");
    assert!(text.contains("--env-targeted"), "{text}");
}

#[test]
fn targeted_variable_lists_only_matching_resources_without_the_hint() {
    let api = MockServer::start();
    let save = mock_api(&api);

    let output = run_set_env(&api, &["--env-targeted", "FEATURE=on:back*"]);
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    save.assert_hits(1);
    assert!(text.contains("backend (container)"), "{text}");
    assert!(!text.contains("frontend"), "{text}");
    assert!(!text.contains("jobs"), "{text}");
    assert!(!text.contains("--env-targeted"), "{text}");
}

#[test]
fn a_variable_that_names_the_gateway_does_not_list_it() {
    let api = MockServer::start();
    let save = mock_api(&api);

    let output = run_set_env(
        &api,
        &[
            "--env-targeted",
            &format!("PROXY_MODE=on:{SANDBOX_EGRESS_GATEWAY_ID}"),
        ],
    );
    let text = stdout(&output);

    assert!(output.status.success(), "{text}");
    save.assert_hits(1);
    // The gateway is Alien's own: its environment is never the application's, so a variable
    // that names it restarts nothing.
    assert!(!text.contains(SANDBOX_EGRESS_GATEWAY_ID), "{text}");
    assert!(!text.contains("backend"), "{text}");
}

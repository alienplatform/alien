use alien_platform_api::types::{
    CapabilityMaterialization, DeploymentLinkSetupResponse, ListProjectsResponse, Project,
    ProjectCapabilities,
};
use serde_json::{json, Value};

fn released_remote_sandbox() -> Value {
    json!({
        "enabled": true,
        "baseImage": "public.ecr.aws/docker/library/buildpack-deps@sha256:abc",
        "maxLifetimeSeconds": 3600,
        "azure": { "catalogImage": "ubuntu", "idleSuspendSeconds": 600 }
    })
}

// Fields a newer server may add to the strict remote sandbox objects: a top-level field, a new
// nested object, and a field inside an object the client already knows.
fn newer_remote_sandbox() -> Value {
    json!({
        "enabled": true,
        "futureField": "value",
        "baseImage": "ghcr.io/acme/img@sha256:abc",
        "maxLifetimeSeconds": 3600,
        "futureCloud": { "image": "registry.example.com/img@sha256:abc", "maxLifetimeSeconds": 3600 },
        "azure": {
            "catalogImage": "ubuntu",
            "futureImage": "ghcr.io/acme/img@sha256:def",
            "idleSuspendSeconds": 600
        }
    })
}

fn capabilities(remote_sandbox: Value) -> Value {
    json!({ "schemaVersion": 1, "capabilities": { "remoteSandbox": remote_sandbox } })
}

fn project(id: &str, remote_sandbox: Value) -> Value {
    json!({
        "id": id,
        "name": "my-app",
        "createdAt": "2026-09-29T00:00:00Z",
        "workspaceId": "ws_AAAAAAAAAAAAAAAAAAAAAAAA",
        "projectCapabilities": capabilities(remote_sandbox)
    })
}

#[test]
fn decodes_projects_carrying_fields_this_client_does_not_know() {
    let newer = project("prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaa", newer_remote_sandbox());
    let decoded: Project = serde_json::from_value(newer.clone()).expect("project decodes");
    let remote_sandbox = decoded
        .project_capabilities
        .expect("capabilities decode")
        .capabilities
        .remote_sandbox
        .expect("remote sandbox decodes");
    assert_eq!(
        remote_sandbox
            .base_image
            .as_ref()
            .map(|image| image.as_str()),
        Some("ghcr.io/acme/img@sha256:abc")
    );
    assert!(remote_sandbox.azure.is_some(), "azure decodes");

    serde_json::from_value::<ProjectCapabilities>(capabilities(newer_remote_sandbox()))
        .expect("capabilities decode");

    let page: ListProjectsResponse = serde_json::from_value(json!({
        "items": [project("prj_bbbbbbbbbbbbbbbbbbbbbbbbbbbb", released_remote_sandbox()), newer],
        "nextCursor": null
    }))
    .expect("one newer project does not fail the page");
    assert_eq!(page.items.len(), 2);
}

#[test]
fn decodes_package_types_this_client_does_not_know() {
    let materialization: CapabilityMaterialization = serde_json::from_value(json!({
        "projectCapabilities": capabilities(released_remote_sandbox()),
        "source": {
            "definitionId": "customer-sandbox",
            "definitionVersion": "1",
            "releaseId": "rel_x"
        },
        "packages": [
            { "type": "sandbox-bundle", "status": "ready" },
            { "type": "future-package-type", "status": "ready" }
        ]
    }))
    .expect("materialization decodes");
    assert_eq!(materialization.packages[1].type_, "future-package-type");

    let setup: DeploymentLinkSetupResponse = serde_json::from_value(json!({
        "activeRelease": null,
        "supportedPlatforms": ["aws", "gcp"],
        "setupItems": ["sandbox"],
        "visiblePackageTypes": ["cloudformation", "future-package-type"],
        "visibleSetupMethods": [],
        "setupPackagesStatus": "ready"
    }))
    .expect("deployment link setup decodes");
    assert_eq!(
        setup.visible_package_types,
        ["cloudformation", "future-package-type"]
    );
}

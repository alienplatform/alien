use alien_platform_api::types::Project;
use serde_json::json;

// A public custom image with a linux/amd64 variant runs on Azure from its registry digest, so the
// saved Azure half names a `registryImage` and no `catalogImage`.
fn project_with_a_custom_image_on_azure() -> serde_json::Value {
    json!({
        "id": "prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "name": "my-app",
        "createdAt": "2026-09-30T00:00:00Z",
        "workspaceId": "ws_AAAAAAAAAAAAAAAAAAAAAAAA",
        "projectCapabilities": {
            "schemaVersion": 1,
            "capabilities": {
                "remoteSandbox": {
                    "enabled": true,
                    "customImage": "docker.io/library/python:3.12",
                    "baseImage": "docker.io/library/python@sha256:aaa",
                    "maxLifetimeSeconds": 3600,
                    "azure": {
                        "registryImage": "docker.io/library/python@sha256:bbb",
                        "idleSuspendSeconds": 900
                    }
                }
            }
        }
    })
}

#[test]
fn decodes_an_azure_half_with_a_registry_image_and_no_catalog_image() {
    let project: Project = serde_json::from_value(project_with_a_custom_image_on_azure())
        .expect("project decodes");
    let azure = project
        .project_capabilities
        .expect("capabilities decode")
        .capabilities
        .remote_sandbox
        .expect("remote sandbox decodes")
        .azure
        .expect("azure decodes");

    assert_eq!(
        azure.registry_image.as_ref().map(|image| image.as_str()),
        Some("docker.io/library/python@sha256:bbb")
    );
    assert!(azure.catalog_image.is_none());
    assert_eq!(azure.idle_suspend_seconds.get(), 900);
}

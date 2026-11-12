//! The values the manager serves for a registered deployment name no infrastructure, so they
//! render for a stack whose frozen custom type lists Helm without its emitter.

use alien_core::{Platform, Stack, StackSettings, StackState};
use alien_helm::{render_manager_fetch_values, ManagerFetchHelmValuesOptions};

fn stack_with_extension(helm: &[&str]) -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "acme-chart",
        "resources": {
            "zone": {
                "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": "frozen",
                "dependencies": []
            }
        },
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": "sha256:0",
            "resourceTypes": {
                "acme/dns-zone": {
                    "lifecycles": ["frozen"],
                    "holdsData": false,
                    "setup": { "cloudformation": ["aws"], "terraform": ["aws"], "helm": helm, "importers": ["aws"] },
                    "controllerPlatforms": [],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }]
    }))
    .unwrap()
}

#[test]
fn fetch_values_render_for_a_custom_type_that_lists_helm() {
    let stack_state = StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());
    let result = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: &"a".repeat(64),
        stack: &stack_with_extension(&["kubernetes"]),
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Aws),
        region: Some("us-east-1"),
        gcp_project_id: None,
        azure_location: None,
    });
    let values = result.expect("manager-fetch values render");
    assert!(values.contains("infrastructure: null"), "{values}");
}

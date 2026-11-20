//! What an extension's importer gets. What it returns reaches the extension's controller, which
//! may run outside the customer's cloud, so it gets no more than the controller would.

use alien_core::import::ImportContext;
use alien_core::{
    ExternalBinding, ExternalBindings, Platform, ResourceType, Stack, StackSettings, StorageBinding,
};
use alien_infra::{DeploymentRegistries, HostRegistrations, LoadedExtensions};
use serde_json::json;

/// An importer that returns its whole input as the controller's fields:
/// `{"status":"running","internalState":<input>}`, built at 4096.
const ECHO_IMPORTER: &str = r#"
    (i64.store (i32.const 4096) (i64.const 8319683848551211643))
    (i64.store (i32.const 4104) (i64.const 7597131072125680162))
    (i64.store (i32.const 4112) (i64.const 8389758751328069486))
    (i64.store (i32.const 4120) (i64.const 7022329443933647461))
    (i64.store (i32.const 4128) (i64.const 2314885531254809972))
    (memory.copy (i32.const 4136) (local.get 2) (local.get 3))
    (i32.store8 (i32.add (i32.const 4136) (local.get 3)) (i32.const 125))
    (i64.or
      (i64.const 4096)
      (i64.shl (i64.extend_i32_u (i32.add (local.get 3) (i32.const 41))) (i64.const 32)))"#;

#[test]
fn an_importer_gets_no_external_bindings() {
    let stack: Stack = serde_json::from_value(json!({
        "id": "app",
        "resources": { "zone": {
            "config": { "type": "acme/dns-zone", "id": "zone", "domain": "a.example" },
            "lifecycle": "frozen", "dependencies": []
        } },
        "extensions": [{
            "name": "acme", "version": "1.0.0", "digest": "sha256:0",
            "resourceTypes": { "acme/dns-zone": {
                "lifecycles": ["frozen"], "holdsData": false,
                "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": ["aws"] },
                "controllerPlatforms": [], "permissionSets": [], "link": { "permissionSets": [] },
                "schemas": { "config": {}, "outputs": {} }
            } }
        }]
    }))
    .unwrap();
    let extensions = LoadedExtensions::default().with_hooks(
        "acme",
        alien_deployment::testing::hooks_module(
            "acme",
            r#"[{"name":"import","kind":"importer","resourceType":"acme/dns-zone","platform":"aws"}]"#,
            ECHO_IMPORTER,
        ),
    );
    let registries = DeploymentRegistries::for_stack(&stack, &HostRegistrations::NONE, &extensions);
    let settings = StackSettings {
        external_bindings: Some(ExternalBindings(
            [(
                "uploads".to_string(),
                ExternalBinding::Storage(StorageBinding::s3("MARKER-EXTERNAL-BUCKET")),
            )]
            .into(),
        )),
        ..Default::default()
    };

    let imported = registries
        .importers
        .run(
            &ResourceType::from("acme/dns-zone"),
            Platform::Aws,
            json!({ "zoneId": "Z1" }),
            &ImportContext {
                resource_id: "zone",
                platform: Platform::Aws,
                region: "us-east-1",
                stack_settings: &settings,
                management_config: None,
                resource: &stack.resources["zone"],
            },
        )
        .unwrap();

    // The proxy sends these fields as the first step's `pendingImport`.
    let saved = serde_json::to_string(&imported.internal_state).unwrap();
    assert!(saved.contains("Z1"), "the importer ran: {saved}");
    assert!(!saved.contains("MARKER-EXTERNAL-BUCKET"), "{saved}");
}

//! Verifies the dual-path `values.schema.json` accepts both bootstrap
//! shapes — `registered setup` (default `values.yaml`) and
//! `external-bindings initialize path` (`examples/onprem.yaml`).

use super::{helpers::render, test_utils};
use alien_core::{
    ArtifactRegistry, ExternalBindings, Kv, Queue, ResourceLifecycle, Stack, StackSettings,
    Storage, Vault,
};

#[test]
fn schema_accepts_registered_setup_default_values() {
    let stack = Stack::new("boot-mgr".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files;
    // The manager-fetch path requires `management.{url,name,token,deploymentId}`
    // — the chart `required` guardrails reject installs missing them, so the
    // test must pass a minimal values overlay.
    let manager_fetch_values = r#"
management:
  url: "https://manager.example.com"
  name: "test-manager"
  token: "test-sync-token"
  deploymentId: "test-deployment-id"
"#;
    test_utils::helm_template_and_validate(&files, Some(manager_fetch_values))
        .assert_ok("manager-fetch path / registered setup");
}

#[test]
fn schema_accepts_external_bindings_initialize_onprem_values() {
    let stack = Stack::new("boot-local".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files;
    let local_values = files
        .get("examples/onprem.yaml")
        .expect("onprem example")
        .clone();
    test_utils::helm_template_and_validate(&files, Some(&local_values))
        .assert_ok("external-bindings initialize path");

    let values: serde_yaml::Value =
        serde_yaml::from_str(&local_values).expect("onprem values should parse");
    let infrastructure = values
        .get("infrastructure")
        .expect("onprem values should include infrastructure")
        .clone();
    let bindings: ExternalBindings =
        serde_yaml::from_value(infrastructure).expect("infrastructure should be ExternalBindings");
    assert!(bindings.has("data"));
    assert!(bindings.has("jobs"));
    assert!(bindings.has("metadata"));
    assert!(bindings.has("secrets"));
    assert!(bindings.has("registry"));
}

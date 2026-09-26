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
    test_utils::helm_template_and_validate(&files, None).assert_ok("registered setup");
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

#[test]
fn registered_setup_mounts_external_bindings() {
    let stack = Stack::new("registered-bindings".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let files = render(&stack, StackSettings::default()).files;
    let onprem: serde_yaml::Value =
        serde_yaml::from_str(&files["examples/onprem.yaml"]).expect("onprem values should parse");
    let bindings = serde_yaml::to_string(&onprem["infrastructure"])
        .expect("external bindings should serialize");
    let bindings = bindings
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let registered = files["values.yaml"].replacen(
        "infrastructure: null",
        &format!("infrastructure:\n{bindings}"),
        1,
    );

    test_utils::helm_template_and_validate(&files, Some(&registered))
        .assert_ok("registered setup with external bindings");
}

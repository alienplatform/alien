//! Verifies the dual-path `values.schema.json` accepts both bootstrap
//! shapes — a new deployment (default `values.yaml`) and
//! `external-bindings initialize path` (`examples/onprem.yaml`).

use super::{helpers::render, test_utils};
use alien_core::{
    ArtifactRegistry, ExternalBindings, Kv, Queue, ResourceLifecycle, Stack, StackSettings,
    Storage, Vault,
};

#[test]
fn schema_accepts_new_deployment_default_values() {
    let stack = Stack::new("boot-mgr".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let files = chart.files;
    test_utils::helm_template_and_validate(&files, None).assert_ok("new deployment");
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
    let registered = files["values.yaml"]
        .replacen("  deploymentId: null", "  deploymentId: dep_existing", 1)
        .replacen(
            "infrastructure: null",
            &format!("infrastructure:\n{bindings}"),
            1,
        );

    test_utils::helm_template_and_validate(&files, Some(&registered))
        .assert_ok("registered setup with external bindings");
}

#[test]
fn bootstrap_needs_only_credentials_and_keeps_registered_deployments_explicit() {
    let chart = render(
        &Stack::new("sample-agent".to_string()).build(),
        StackSettings::default(),
    );
    let values = "management:\n  token: ax_bootstrap\nruntime:\n  encryption:\n    key: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n";
    let bootstrap = test_utils::helm_template(&chart.files, Some(values));
    bootstrap.assert_ok("bootstrap with only credentials");
    let env = operator_env(&bootstrap.stdout);
    assert_eq!(
        env.get("SYNC_URL").map(String::as_str),
        Some("https://manager.alien.dev")
    );
    assert_eq!(
        env.get("OPERATOR_SETUP_ITEM").map(String::as_str),
        Some("deployment")
    );
    assert!(!env.contains_key("DEPLOYMENT_ID"));

    let registered_values = values.replace("  token: ax_bootstrap", "  token: ax_existing\n  deploymentId: dep_existing\n  url: https://management.example.test\n  setupItem: custom-item");
    let registered = test_utils::helm_template(&chart.files, Some(&registered_values));
    registered.assert_ok("registered deployment with custom endpoint and setup item");
    let env = operator_env(&registered.stdout);
    assert_eq!(
        env.get("DEPLOYMENT_ID").map(String::as_str),
        Some("dep_existing")
    );
    assert_eq!(
        env.get("SYNC_URL").map(String::as_str),
        Some("https://management.example.test")
    );
    assert_eq!(
        env.get("OPERATOR_SETUP_ITEM").map(String::as_str),
        Some("custom-item")
    );
}

fn operator_env(manifest: &str) -> std::collections::BTreeMap<String, String> {
    let deployment = serde_yaml::Deserializer::from_str(manifest)
        .map(|doc| {
            <serde_yaml::Value as serde::Deserialize>::deserialize(doc).expect("Kubernetes YAML")
        })
        .find(|doc| doc["kind"] == "Deployment")
        .expect("Operator Deployment");
    deployment["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .expect("Operator environment")
        .iter()
        .filter_map(|entry| {
            Some((
                entry["name"].as_str()?.to_string(),
                entry["value"].as_str()?.to_string(),
            ))
        })
        .collect()
}

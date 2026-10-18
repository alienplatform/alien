//! Per-resource layer scenarios — storage / kv / queue / vault /
//! artifact-registry contributions land under
//! `infrastructure.<resource_id>` in the chart's `values.yaml`.

use super::helpers::{assert_helm_valid, render, snapshot_chart};
use alien_core::{
    ArtifactRegistry, Kv, Queue, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings, Storage, Vault,
};

#[test]
fn data_layer_emits_infrastructure_bindings() {
    let stack = Stack::new("data-chart".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
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
    snapshot_chart("data_layer", &chart);
    assert_helm_valid(&chart, "data_layer");
}

/// The Kubernetes Frozen parent owns two things the operator does not: the NetworkPolicy that
/// makes the declared egress real, and the cluster-scoped RBAC the broker's `TokenReview` needs.
/// Rendering is not enough on its own — `assert_helm_valid` runs `helm lint`, `helm template` and
/// `kubeconform`, so a policy the API server would reject fails here rather than at install.
#[test]
fn a_sandbox_emits_its_network_policy_and_the_brokers_rbac() {
    let stack = Stack::new("sandbox-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(3600),
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());

    let policy = chart
        .files
        .get("templates/sandbox-agent-networkpolicy.yaml")
        .unwrap_or_else(|| {
            panic!(
                "the sandbox NetworkPolicy must render: {:?}",
                chart.files.keys().collect::<Vec<_>>()
            )
        });
    assert!(
        policy.contains("alien.dev/sandbox: agent"),
        "the policy must select the operator's own pod label:\n{policy}"
    );
    assert!(
        !policy.contains("egress:"),
        "deny is an Egress policy type with no rule; a rule here would permit something:\n{policy}"
    );

    let rbac = chart
        .files
        .get("templates/sandbox-broker-rbac.yaml")
        .expect("the broker's RBAC must render");
    assert!(
        rbac.contains("tokenreviews"),
        "a TokenReview is cluster-scoped, so a namespaced Role cannot grant it:\n{rbac}"
    );

    assert_helm_valid(&chart, "sandbox_layer");
}

/// Internet sessions can only reach this release's gateway, which resolves names on
/// their behalf. Helm/kubeconform validate the complete rendered resource set.
#[test]
fn a_sandbox_allowing_egress_only_reaches_its_gateway() {
    let stack = Stack::new("sandbox-allow-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());

    let policy = chart
        .files
        .get("templates/sandbox-agent-networkpolicy.yaml")
        .expect("the sandbox NetworkPolicy must render");
    assert!(policy.contains("sandbox-egress-gateway"));
    assert!(policy.contains("port: 3128"));
    assert!(!policy.contains("ipBlock:"));
    assert!(!policy.contains("port: 53"));
    // Inbound is the agent port from this release's own pods. Denying it outright drops every
    // exec and file call on an enforcing CNI, which is what the application drives a session with.
    assert!(
        policy.contains("- Ingress"),
        "ingress stays governed by the policy:\n{policy}"
    );
    assert!(
        policy.contains("port: 8971"),
        "the application must reach the agent port:\n{policy}"
    );
    assert!(
        policy.contains(r#"app.kubernetes.io/instance: {{ .Release.Name }}"#),
        "inbound is scoped to this release's pods, not the whole cluster:\n{policy}"
    );
    assert!(
        !policy.contains("port: 8080"),
        "a preview port stays unreachable: that needs a gateway validating a session-and-port \
         capability, and none exists:\n{policy}"
    );

    assert_helm_valid(&chart, "sandbox_layer_allow");
}

#[test]
fn sandbox_management_gets_only_its_signing_secret() {
    use alien_core::{permissions::PermissionSetReference, PermissionProfile};
    use serde::Deserialize;
    let sandbox = Sandbox::new("subject".into())
        .code(SandboxCode::Image {
            image: "example.invalid/agent:1".into(),
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(300),
            idle_pause_seconds: None,
        })
        .build();
    let mut stack = Stack::new("gateway-access".into())
        .add(sandbox, ResourceLifecycle::Frozen)
        .build();
    let mut manager = PermissionProfile::new();
    manager.0.insert(
        "subject".into(),
        vec![PermissionSetReference::Name("sandbox/management".into())],
    );
    stack.permissions.profiles.insert("manager".into(), manager);
    let mut inline_manager = PermissionProfile::new();
    inline_manager.0.insert(
        "*".into(),
        vec![PermissionSetReference::Inline(
            alien_core::permissions::PermissionSet {
                id: "sandbox/management".into(),
                description: "Manage sandbox sessions".into(),
                platforms: alien_core::permissions::PlatformPermissions {
                    aws: None,
                    gcp: None,
                    azure: None,
                },
            },
        )],
    );
    stack
        .permissions
        .profiles
        .insert("inline-manager".into(), inline_manager);
    stack
        .permissions
        .profiles
        .insert("unrelated".into(), PermissionProfile::new());
    let chart = render(&stack, StackSettings::default());
    assert_helm_valid(&chart, "sandbox gateway exact secret grant");
    let rendered =
        super::test_utils::helm_template(&chart.files, Some("serviceAccountPrefix: test-prefix\n"));
    rendered.assert_ok("sandbox gateway RBAC");
    let documents: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&rendered.stdout)
        .map(|document| serde_yaml::Value::deserialize(document).unwrap())
        .collect();
    let roles: Vec<_> = documents
        .iter()
        .filter(|doc| doc["kind"] == "Role")
        .filter(|doc| {
            doc["metadata"]["name"]
                .as_str()
                .is_some_and(|name| name.ends_with("-egress"))
        })
        .collect();
    assert_eq!(roles.len(), 2);
    let mut accounts = std::collections::BTreeSet::new();
    for role in roles {
        assert_eq!(
            role["rules"],
            serde_yaml::from_str::<serde_yaml::Value>(
                r#"
- apiGroups: [""]
  resources: [secrets]
  resourceNames: [test-prefix-secrets-sandbox-egress-signing-key]
  verbs: [get]
"#
            )
            .unwrap()
        );
        let binding = documents
            .iter()
            .find(|doc| {
                doc["kind"] == "RoleBinding" && doc["roleRef"]["name"] == role["metadata"]["name"]
            })
            .unwrap();
        accounts.insert(binding["subjects"][0]["name"].as_str().unwrap());
        assert_eq!(binding["subjects"].as_sequence().unwrap().len(), 1);
    }
    assert_eq!(
        accounts,
        std::collections::BTreeSet::from([
            "test-prefix-manager-sa",
            "test-prefix-inline-manager-sa"
        ])
    );

    stack
        .permissions
        .profiles
        .get_mut("unrelated")
        .unwrap()
        .0
        .insert(
            "app-vault".into(),
            vec![PermissionSetReference::Name("vault/data-read".into())],
        );
    let error = super::helpers::try_render(&stack, StackSettings::default()).unwrap_err();
    assert!(error.to_string().contains("namespace-wide Secret access"));
    assert!(error.to_string().contains("unrelated"));
    // Existing deployments without an internet sandbox retain their permissions.
    stack
        .resources
        .get_mut("subject")
        .unwrap()
        .config
        .downcast_mut::<Sandbox>()
        .unwrap()
        .egress = SandboxEgress::Allow;
    super::helpers::try_render(&stack, StackSettings::default()).unwrap();
}

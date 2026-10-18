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
    assert!(policy.contains("alien.dev/sandbox-egress-gateway: \"true\""));
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

//! Per-resource layer scenarios — storage / kv / queue / vault /
//! artifact-registry contributions land under
//! `infrastructure.<resource_id>` in the chart's `values.yaml`.

use super::helpers::{assert_helm_valid, render, snapshot_chart, try_render};
use alien_core::{
    ArtifactRegistry, Kv, Queue, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxSessionPolicy, Stack, StackSettings, Storage, Vault,
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

/// The Kubernetes Frozen parent, which nothing emitted before this.
///
/// Two things the chart owns and the operator does not: the NetworkPolicy that makes the declared
/// egress real, and the cluster-scoped RBAC the broker's `TokenReview` needs. Rendering is not
/// enough on its own — `assert_helm_valid` runs `helm lint`, `helm template` and `kubeconform`, so
/// a policy the API server would reject fails here rather than at install.
#[test]
fn a_sandbox_emits_its_network_policy_and_the_brokers_rbac() {
    let stack = Stack::new("sandbox-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                })
                .egress(SandboxEgress::Deny)
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: Some(3600),
                    idle_suspend_seconds: None,
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

/// Under `allow` the policy must still close the metadata endpoint and the deployment's own
/// private ranges — the sandbox reaching either is a lateral move, not egress.
///
/// The metadata server is excluded as a **`/32`**. Excluding the enclosing `169.254.0.0/16`
/// agreed with nothing else: GKE's NodeLocal DNSCache sits at `169.254.20.10`, so the wide range
/// takes DNS down and `allow` stops reaching anything at all. The GCE metadata server was
/// observed answering from inside a gVisor pod on GKE, which is why it is denied in both modes.
#[test]
fn a_sandbox_allowing_egress_still_denies_the_metadata_endpoint() {
    let stack = Stack::new("sandbox-allow-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                })
                .egress(SandboxEgress::Allow)
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: None,
                    idle_suspend_seconds: None,
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
    for denied in [
        "169.254.169.254/32",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
    ] {
        assert!(
            policy.contains(denied),
            "{denied} must stay closed even under allow:\n{policy}"
        );
    }
    assert!(
        !policy.contains("169.254.0.0/16"),
        "denying all of link-local takes out NodeLocal DNS at 169.254.20.10:\n{policy}"
    );
    // The excepts above cover the ranges a cluster's DNS service lives in, so without a rule
    // that names the resolver by selector, allow mode resolves no names off GKE.
    assert!(
        policy.contains("k8s-app: kube-dns") && policy.contains("port: 53"),
        "allow must reach the cluster resolver, which the denied ranges otherwise cover:\n{policy}"
    );
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

/// NetworkPolicy matches addresses, not names, so a hostname allowlist has nothing to render
/// into. It is refused: rendering it as `allow` would open every address the list excluded, and
/// the chart would look like the policy applied.
#[test]
fn a_hostname_allowlist_is_refused_rather_than_widened() {
    let stack = Stack::new("sandbox-domains-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                })
                .egress(SandboxEgress::AllowDomains {
                    domains: vec!["example.com".to_string()],
                })
                .session(SandboxSessionPolicy {
                    max_lifetime_seconds: None,
                    idle_suspend_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let error = try_render(&stack, StackSettings::default())
        .expect_err("a hostname list must be refused rather than approximated");

    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(
        error.to_string().contains("agent"),
        "the refusal must name the sandbox it is about: {error}"
    );
}

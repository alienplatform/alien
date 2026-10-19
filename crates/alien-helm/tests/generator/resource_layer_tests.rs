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

/// Kubernetes hosts the sandbox gateway itself. The same preparation `alien render` runs adds a
/// replicated gateway Container with its own service account, sessions can reach only that
/// gateway's pods, and installation first proves that the cluster enforces NetworkPolicy.
#[tokio::test]
async fn kubernetes_internet_sandbox_installs_a_hosted_gateway() {
    use alien_core::{
        sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, Container, ContainerEgress, DeploymentConfig,
        EnvironmentVariablesSnapshot, ExternalBindings, Platform, StackState,
    };
    use serde::Deserialize;

    let stack = Stack::new("kubernetes-internet".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "example.invalid/agent:1".to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(300),
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .expect("Kubernetes hosts the gateway for internet sandboxes");
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let prepared = runner
        .apply_mutations(stack, &StackState::new(Platform::Kubernetes), &config)
        .await
        .expect("preparation adds the gateway");

    let gateway = prepared.resources[SANDBOX_EGRESS_GATEWAY_ID]
        .config
        .downcast_ref::<Container>()
        .expect("the gateway is a Container");
    assert!(gateway.replicas.is_some_and(|replicas| replicas >= 2));
    assert_eq!(gateway.egress, Some(ContainerEgress::Internet));
    // The controller injects the gateway's configuration; the pod never reads the vault.
    assert!(gateway.links.is_empty(), "{:?}", gateway.links);
    // Replicas receive traffic only after copying the stored policies.
    assert_eq!(
        gateway.kubernetes_readiness_probe,
        Some(alien_core::KubernetesHttpProbe {
            path: "/health".into(),
            port: 3129,
        })
    );

    let chart = render(&prepared, StackSettings::default());
    assert_helm_valid(&chart, "kubernetes internet sandbox");
    // The operator labels runtime pods with its resource prefix, which an installer may
    // set independently of the release name.
    let rendered = super::test_utils::helm_template(
        &chart.files,
        // Node heartbeats off: the operator still lists nodes to exclude their addresses.
        Some("serviceAccountPrefix: custom-prefix\nheartbeat:\n  collection:\n    nodes:\n      enabled: false\n"),
    );
    rendered.assert_ok("kubernetes internet sandbox");
    let documents: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&rendered.stdout)
        .map(|document| serde_yaml::Value::deserialize(document).unwrap())
        .filter(|document| !document.is_null())
        .collect();

    let accounts: Vec<_> = documents
        .iter()
        .filter(|doc| doc["kind"] == "ServiceAccount")
        .filter_map(|doc| doc["metadata"]["name"].as_str())
        .collect();
    assert!(
        accounts
            .iter()
            .any(|name| name.ends_with(&format!("{SANDBOX_EGRESS_GATEWAY_ID}-sa"))),
        "the gateway Deployment needs its service account: {accounts:?}"
    );

    let manager = documents
        .iter()
        .find(|doc| doc["kind"] == "Deployment")
        .and_then(|doc| doc["spec"]["template"]["spec"]["serviceAccountName"].as_str())
        .expect("operator service account");
    let node_readers: Vec<_> = documents
        .iter()
        .filter(|doc| doc["kind"] == "ClusterRole")
        .filter(|role| {
            role["rules"].as_sequence().is_some_and(|rules| {
                rules.iter().any(|rule| {
                    rule["resources"] == serde_yaml::Value::from(vec!["nodes"])
                        && rule["apiGroups"] == serde_yaml::Value::from(vec![""])
                        && rule["verbs"]
                            .as_sequence()
                            .is_some_and(|verbs| verbs.contains(&"list".into()))
                })
            })
        })
        .filter_map(|role| role["metadata"]["name"].as_str())
        .collect();
    assert!(
        documents.iter().any(|doc| doc["kind"] == "ClusterRoleBinding"
            && doc["roleRef"]["name"]
                .as_str()
                .is_some_and(|name| node_readers.contains(&name))
            && doc["subjects"][0]["name"] == manager),
        "the operator must list nodes to exclude node and pod ranges: {node_readers:?}"
    );
    assert!(
        documents.iter().any(|doc| doc["kind"] == "ClusterRole"
            && doc["metadata"]["name"]
                .as_str()
                .is_some_and(|name| node_readers.contains(&name))
            && doc["rules"].as_sequence().is_some_and(|rules| rules.iter().any(|rule| {
                rule["apiGroups"] == serde_yaml::Value::from(vec!["networking.k8s.io"])
                    && rule["resources"] == serde_yaml::Value::from(vec!["servicecidrs"])
            }))),
        "the operator must list Service ranges to exclude them"
    );

    // Two releases in one namespace must not claim the same node-read role names.
    let cluster_scoped = |release: &str| -> Vec<String> {
        let rendered = super::test_utils::helm_template_for_release(&chart.files, None, release);
        rendered.assert_ok(release);
        serde_yaml::Deserializer::from_str(&rendered.stdout)
            .map(|document| serde_yaml::Value::deserialize(document).unwrap())
            .filter(|doc| matches!(doc["kind"].as_str(), Some("ClusterRole" | "ClusterRoleBinding")))
            .filter_map(|doc| doc["metadata"]["name"].as_str().map(str::to_string))
            .filter(|name| name.contains("-egress-nodes-"))
            .collect()
    };
    let shared = "release-names-sharing-the-first-forty-chars";
    let first = cluster_scoped(&format!("{shared}-one"));
    let second = cluster_scoped(&format!("{shared}-two"));
    assert!(!first.is_empty());
    assert!(
        first.iter().all(|name| !second.contains(name)),
        "{first:?} {second:?}"
    );

    let probe = documents
        .iter()
        .find(|doc| {
            doc["kind"] == "Job"
                && doc["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-sandbox-cni"))
        })
        .expect("installation verifies NetworkPolicy enforcement");
    assert_eq!(
        probe["metadata"]["annotations"]["helm.sh/hook"],
        "pre-install,pre-upgrade"
    );
    // The probe image is pinned by digest.
    let image = probe["spec"]["template"]["spec"]["containers"][0]["image"]
        .as_str()
        .unwrap();
    assert!(image.contains("@sha256:"), "{image}");
    // Reads and deletions are limited to the probe's own objects; creation, which RBAC
    // cannot scope, is confined by an admission policy bound to the probe account.
    let probe_name = probe["metadata"]["name"].as_str().unwrap();
    let role = documents
        .iter()
        .find(|doc| doc["kind"] == "Role" && doc["metadata"]["name"] == probe_name)
        .expect("probe Role");
    for rule in role["rules"].as_sequence().unwrap() {
        let verbs = rule["verbs"].as_sequence().unwrap();
        if verbs.iter().any(|verb| verb == "delete" || verb == "get") {
            let names = rule["resourceNames"].as_sequence().expect("named objects only");
            assert!(names
                .iter()
                .all(|name| name.as_str().is_some_and(|name| name.starts_with(probe_name))));
        } else {
            assert_eq!(verbs, &vec![serde_yaml::Value::from("create")], "{rule:?}");
        }
    }
    let admission = documents
        .iter()
        .find(|doc| doc["kind"] == "ValidatingAdmissionPolicy")
        .expect("probe admission policy");
    let condition = admission["spec"]["matchConditions"][0]["expression"]
        .as_str()
        .unwrap();
    assert!(condition.contains(&format!(":{probe_name}'")), "{condition}");
    let rules = serde_yaml::to_string(&admission["spec"]["validations"]).unwrap();
    assert!(rules.contains("automountServiceAccountToken == false") && rules.contains(image));
    // Created pods cannot ask for host powers the probe itself does not have.
    for required in [
        "'ALL' in c.securityContext.capabilities.drop",
        "!has(c.securityContext.capabilities.add)",
        "allowPrivilegeEscalation == false",
        "seccompProfile.type == 'RuntimeDefault'",
        "runAsUser == 65534",
    ] {
        assert!(rules.contains(required), "{required}: {rules}");
    }
    assert!(documents
        .iter()
        .any(|doc| doc["kind"] == "ValidatingAdmissionPolicyBinding"
            && doc["spec"]["policyName"] == admission["metadata"]["name"]
            && doc["spec"]["validationActions"] == serde_yaml::Value::from(vec!["Deny"])));

    let policy = documents
        .iter()
        .find(|doc| doc["kind"] == "NetworkPolicy" && doc["metadata"]["name"] == "alien-sbx-agent")
        .expect("the session NetworkPolicy");
    let egress = policy["spec"]["egress"]
        .as_sequence()
        .expect("egress rules");
    assert_eq!(egress.len(), 1, "{egress:?}");
    let peers = egress[0]["to"].as_sequence().unwrap();
    assert_eq!(peers.len(), 1);
    assert!(peers[0]["ipBlock"].is_null() && peers[0]["namespaceSelector"].is_null());
    let labels = peers[0]["podSelector"]["matchLabels"].as_mapping().unwrap();
    assert_eq!(
        labels
            .get("alien.dev/resource")
            .and_then(serde_yaml::Value::as_str),
        Some(SANDBOX_EGRESS_GATEWAY_ID)
    );
    assert_eq!(
        labels
            .get("alien.dev/deployment")
            .and_then(serde_yaml::Value::as_str),
        Some("custom-prefix"),
        "the session policy must select the pods the operator labels"
    );
    assert_eq!(
        egress[0]["ports"],
        serde_yaml::from_str::<serde_yaml::Value>("[{protocol: TCP, port: 3128}]").unwrap()
    );
}

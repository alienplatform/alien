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
                    bundled_from: None,
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

/// A live sandbox gets the same chart-owned rules as a frozen one: the deployment template
/// refers to the RuntimeClass helper whenever the stack has a sandbox, so skipping the emitter for
/// a live one leaves a chart that fails at install with a missing template.
#[test]
fn a_live_sandbox_emits_the_same_chart_resources_as_a_frozen_one() {
    let stack = Stack::new("sandbox-live-chart".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(3600),
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let chart = render(&stack, StackSettings::default());

    for file in [
        "templates/sandbox-agent-networkpolicy.yaml",
        "templates/sandbox-broker-rbac.yaml",
        "templates/_sandbox.tpl",
    ] {
        assert!(
            chart.files.contains_key(file),
            "{file} must render for a live sandbox: {:?}",
            chart.files.keys().collect::<Vec<_>>()
        );
    }
    assert_helm_valid(&chart, "sandbox_layer_live");
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
                    bundled_from: None,
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
            bundled_from: None,
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
                    bundled_from: None,
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
        documents
            .iter()
            .any(|doc| doc["kind"] == "ClusterRoleBinding"
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
            && doc["rules"]
                .as_sequence()
                .is_some_and(|rules| rules.iter().any(|rule| {
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
            .filter(|doc| {
                matches!(
                    doc["kind"].as_str(),
                    Some("ClusterRole" | "ClusterRoleBinding")
                )
            })
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
            let names = rule["resourceNames"]
                .as_sequence()
                .expect("named objects only");
            assert!(names.iter().all(|name| name
                .as_str()
                .is_some_and(|name| name.starts_with(probe_name))));
        } else {
            assert_eq!(verbs, &vec![serde_yaml::Value::from("create")], "{rule:?}");
        }
    }
    let admission = documents
        .iter()
        .find(|doc| {
            doc["kind"] == "ValidatingAdmissionPolicy"
                && doc["spec"]["matchConstraints"]["resourceRules"][0]["resources"][0] == "pods"
        })
        .expect("probe pod admission policy");
    let condition = admission["spec"]["matchConditions"][0]["expression"]
        .as_str()
        .unwrap();
    assert!(
        condition.contains(&format!(":{probe_name}'")),
        "{condition}"
    );
    let rules = serde_yaml::to_string(&admission["spec"]["validations"]).unwrap();
    assert!(rules.contains("automountServiceAccountToken == false") && rules.contains(image));

    // One chart value moves every image to the site's registry, and the admission policy
    // that only admits the probe's exact image follows it.
    let mirrored = super::test_utils::helm_template(
        &chart.files,
        Some("serviceAccountPrefix: custom-prefix\nglobal:\n  imageRegistry: mirror.site.internal:5000/alien\nheartbeat:\n  collection:\n    nodes:\n      enabled: false\n"),
    );
    mirrored.assert_ok("mirrored sandbox chart");
    let mirrored_documents = rendered_documents(&mirrored);
    // An air-gapped install passes the exact reference it bundled (a single-architecture
    // export pins another digest than the default), and it follows the mirror too.
    let bundled = super::test_utils::helm_template(
        &chart.files,
        Some("global:\n  imageRegistry: mirror.site.internal:5000/alien\nsandbox:\n  cniProbeImage: docker.io/library/python:3.13-slim@sha256:aaaa\n"),
    );
    bundled.assert_ok("bundled probe image");
    assert!(
        bundled
            .stdout
            .contains("mirror.site.internal:5000/alien/library/python@sha256:aaaa"),
        "the probe runs the bundled digest from the mirror"
    );
    assert!(!bundled.stdout.contains("70729b46c69b4f1e97c4822c1af3df53a1476cf5ddc6c087c0c10bc3a5678c2f"));
    // A pinned Operator digest is the install's choice; self-update to the manager's tag would undo it.
    let self_update_env = |values: &str| -> bool {
        let rendered = super::test_utils::helm_template(&chart.files, Some(values));
        rendered.assert_ok("self-update chart");
        rendered_documents(&rendered).iter().any(|doc| {
            doc["spec"]["template"]["spec"]["containers"]
                .as_sequence()
                .into_iter()
                .flatten()
                .any(|c| {
                    c["env"]
                        .as_sequence()
                        .into_iter()
                        .flatten()
                        .any(|e| e["name"] == "OPERATOR_SELF_UPDATE_DEPLOYMENT")
                })
        })
    };
    assert!(self_update_env("serviceAccountPrefix: custom-prefix\n"));
    assert!(!self_update_env(&format!(
        "serviceAccountPrefix: custom-prefix\nruntime:\n  image:\n    digest: sha256:{}\n",
        "a".repeat(64)
    )));
    let mut images = Vec::new();
    for doc in &mirrored_documents {
        let pod = match doc["kind"].as_str() {
            Some("Pod") => &doc["spec"],
            Some(_) => &doc["spec"]["template"]["spec"],
            None => continue,
        };
        for key in ["containers", "initContainers"] {
            for container in pod[key].as_sequence().into_iter().flatten() {
                images.push(container["image"].as_str().unwrap().to_string());
            }
        }
    }
    assert!(!images.is_empty());
    for image in &images {
        assert!(
            image.starts_with("mirror.site.internal:5000/alien/"),
            "{image} does not come from the mirror"
        );
    }
    let mirrored_probe = mirrored_documents
        .iter()
        .find(|doc| {
            doc["kind"] == "Job"
                && doc["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-sandbox-cni"))
        })
        .expect("probe job");
    let mirrored_image = mirrored_probe["spec"]["template"]["spec"]["containers"][0]["image"]
        .as_str()
        .unwrap();
    assert_eq!(
        mirrored_image,
        image.replace("docker.io/", "mirror.site.internal:5000/alien/"),
    );
    let mirrored_rules = serde_yaml::to_string(
        &mirrored_documents
            .iter()
            .find(|doc| {
                doc["kind"] == "ValidatingAdmissionPolicy"
                    && doc["spec"]["matchConstraints"]["resourceRules"][0]["resources"][0] == "pods"
            })
            .unwrap()["spec"]["validations"],
    )
    .unwrap();
    assert!(
        mirrored_rules.contains(&format!("c.image == '{mirrored_image}'")),
        "{mirrored_rules}"
    );
    let operator_env = mirrored_documents
        .iter()
        .find(|doc| doc["kind"] == "Deployment")
        .map(|doc| {
            serde_yaml::to_string(&doc["spec"]["template"]["spec"]["containers"][0]["env"]).unwrap()
        })
        .unwrap();
    assert!(
        operator_env.contains("IMAGE_REGISTRY")
            && operator_env.contains("mirror.site.internal:5000/alien"),
        "{operator_env}"
    );
    // Created pods cannot ask for host powers the probe itself does not have.
    for required in [
        "'ALL' in c.securityContext.capabilities.drop",
        "!has(c.securityContext.capabilities.add)",
        "allowPrivilegeEscalation == false",
        "seccompProfile.type == 'RuntimeDefault'",
        "runAsUser == 65534",
        // Gateway or sandbox labels would join a Service or a policy selector.
        "size(object.metadata.labels) == 2",
        "p.hostPort == 0",
        "!has(object.spec.securityContext.seLinuxOptions)",
        "!has(c.securityContext.appArmorProfile)",
        "runAsGroup != 0",
        "object.spec.nodeName == ''",
    ] {
        assert!(rules.contains(required), "{required}: {rules}");
    }
    // NetworkPolicy creation is confined too: only the probe's two names, a selector of
    // exactly the probe labels, and no allow rules.
    let policies = documents
        .iter()
        .find(|doc| {
            doc["kind"] == "ValidatingAdmissionPolicy"
                && doc["spec"]["matchConstraints"]["resourceRules"][0]["resources"][0]
                    == "networkpolicies"
        })
        .expect("probe NetworkPolicy admission policy");
    assert_eq!(
        policies["spec"]["matchConditions"][0]["expression"],
        admission["spec"]["matchConditions"][0]["expression"]
    );
    let policy_rules = policies["spec"]["validations"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(|rule| rule["expression"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for required in [
        format!("object.metadata.name == '{probe_name}'"),
        "size(object.spec.podSelector.matchLabels) == 2".to_string(),
        format!("matchLabels['alien.dev/cni-probe'] == '{probe_name}'"),
        "size(object.spec.ingress) == 0".to_string(),
        "size(object.spec.egress) == 0".to_string(),
    ] {
        assert!(
            policy_rules.contains(&required),
            "{required}: {policy_rules}"
        );
    }
    // Under default-deny pod networking (EKS Auto Mode strict) a pod with no policy has no
    // network: the runner needs its API server egress, and the probe's own pods need allow
    // rules that never select the `denied` pod (policies are additive).
    let allow = |suffix: &str| {
        documents
            .iter()
            .find(|doc| {
                doc["kind"] == "NetworkPolicy"
                    && doc["metadata"]["name"] == format!("{probe_name}-{suffix}").as_str()
            })
            .unwrap_or_else(|| panic!("the probe installs its {suffix} allow policy"))
    };
    let runner = allow("runner");
    assert_eq!(
        runner["spec"]["podSelector"]["matchLabels"]["alien.dev/cni-probe-runner"],
        probe_name
    );
    // The runner's egress keeps its port list: without it the EKS network policy agent stopped
    // refusing the `denied` probe pod.
    let runner_ports: Vec<_> = runner["spec"]["egress"][0]["ports"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|port| port["port"].as_u64().unwrap())
        .collect();
    assert_eq!(runner_ports, vec![443, 6443]);
    // No `to` block: on Cilium a CIDR rule does not match node addresses, and the API server
    // often runs on a node. The rule allows any destination on those ports.
    assert!(
        runner["spec"]["egress"][0]["to"].is_null(),
        "{}",
        serde_yaml::to_string(runner).unwrap()
    );
    // A failed install must not leave any probe policy behind.
    for suffix in ["runner", "egress", "listener"] {
        let policy = allow(suffix);
        let delete_policy = policy["metadata"]["annotations"]["helm.sh/hook-delete-policy"]
            .as_str()
            .unwrap();
        assert!(
            delete_policy.contains("hook-failed"),
            "{suffix}: {delete_policy}"
        );
    }
    let egress = serde_yaml::to_string(allow("egress")).unwrap();
    assert!(
        egress.contains("denied") && egress.contains("NotIn"),
        "{egress}"
    );
    let listener = serde_yaml::to_string(allow("listener")).unwrap();
    // The denied pod must reach the listener's ingress, so only the egress bound can refuse it;
    // the ingress-denied pod must not, so only the ingress bound can refuse it.
    let listener_roles: Vec<&str> = allow("listener")["spec"]["ingress"][0]["from"][0]
        ["podSelector"]["matchExpressions"][0]["values"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|role| role.as_str().unwrap())
        .collect();
    assert!(listener_roles.contains(&"denied"), "{listener}");
    assert!(!listener_roles.contains(&"ingress-denied"), "{listener}");
    assert!(documents
        .iter()
        .any(|doc| doc["kind"] == "ValidatingAdmissionPolicyBinding"
            && doc["spec"]["policyName"] == policies["metadata"]["name"]
            && doc["spec"]["validationActions"] == serde_yaml::Value::from(vec!["Deny"])));
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

fn rendered_documents(rendered: &super::test_utils::LinterRun) -> Vec<serde_yaml::Value> {
    use serde::Deserialize;
    serde_yaml::Deserializer::from_str(&rendered.stdout)
        .map(|document| serde_yaml::Value::deserialize(document).expect("rendered YAML"))
        .filter(|document| !document.is_null())
        .collect()
}

fn sandbox_with_linked_app() -> Stack {
    use alien_core::{Container, ContainerCode, ResourceSpec};
    let sandbox = Sandbox::new("agent".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu:24.04".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::None)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: Some(3600),
            idle_pause_seconds: None,
        })
        .build();
    let container = |id: &str| {
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "example.invalid/app:1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.1".to_string(),
                desired: "0.1".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "128Mi".to_string(),
            })
            .permissions("app".to_string())
    };
    let app = container("app").link(&sandbox).build();
    let other = container("other").build();
    // A valid id that YAML would read as a boolean if it were not quoted.
    let boolean_named = container("true").link(&sandbox).build();
    Stack::new("sandbox-broker-chart".to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
        .add(app, ResourceLifecycle::Live)
        .add(other, ResourceLifecycle::Live)
        .add(boolean_named, ResourceLifecycle::Live)
        .build()
}

/// The controller publishes the operator's `ALIEN_SANDBOX_BROKER_URL` as the binding's broker
/// address, and a linked application calls it to claim every session. So the address has to name
/// a Service this chart renders, on a port that reaches the operator's listener. The release name
/// and the deployment name differ here on purpose: the operator's identity is not its Service.
#[test]
fn the_operator_is_given_the_address_of_its_own_broker_service() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    assert_helm_valid(&chart, "sandbox broker service");
    let rendered = super::test_utils::helm_template_for_release(
        &chart.files,
        Some("management:\n  name: customer-1\n"),
        "acme",
    );
    rendered.assert_ok("sandbox broker service");
    let documents = rendered_documents(&rendered);

    let operator = documents
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "acme")
        .expect("the operator Deployment");
    let template = &operator["spec"]["template"];
    let container = &template["spec"]["containers"][0];
    let env = |name: &str| {
        container["env"]
            .as_sequence()
            .expect("operator env")
            .iter()
            .find(|variable| variable["name"] == name)
            .and_then(|variable| variable["value"].as_str())
            .map(str::to_string)
    };
    let broker = env(alien_core::ENV_ALIEN_SANDBOX_BROKER_URL)
        .expect("the operator is told where its broker is reachable");
    let listener: u64 = env("OTLP_PORT")
        .expect("the operator's listener port")
        .parse()
        .expect("numeric port");

    let address = broker
        .strip_prefix("http://")
        .expect("in-cluster plain HTTP, like the broker listener");
    let (host, port) = address.split_once(':').expect("host and port");
    let port: u64 = port.parse().expect("numeric port");
    let (service_name, rest) = host.split_once('.').expect("service.namespace.svc");
    assert_eq!(rest, "default.svc", "the release namespace: {broker}");

    let service = documents
        .iter()
        .find(|document| {
            document["kind"] == "Service" && document["metadata"]["name"] == service_name
        })
        .unwrap_or_else(|| panic!("{broker} names no Service this chart renders"));
    let service_port = service["spec"]["ports"]
        .as_sequence()
        .expect("Service ports")
        .iter()
        .find(|entry| entry["port"].as_u64() == Some(port))
        .unwrap_or_else(|| panic!("Service {service_name} does not expose port {port}"));
    let target = service_port["targetPort"]
        .as_str()
        .expect("named target port");
    let container_port = container["ports"]
        .as_sequence()
        .expect("operator ports")
        .iter()
        .find(|entry| entry["name"] == target)
        .unwrap_or_else(|| panic!("the operator declares no port named {target}"));
    assert_eq!(
        container_port["containerPort"].as_u64(),
        Some(listener),
        "the Service reaches the port the broker listens on"
    );
    let pod_labels = template["metadata"]["labels"]
        .as_mapping()
        .expect("pod labels");
    for (key, value) in service["spec"]["selector"]
        .as_mapping()
        .expect("Service selector")
    {
        assert_eq!(
            pod_labels.get(key),
            Some(value),
            "the Service selects the operator pod"
        );
    }
}

/// An application drives its sessions over the agent port, so the sandbox policy must admit the
/// workloads that link the sandbox, and only them, alongside the operator.
#[test]
fn the_sandbox_policy_admits_its_linkers_on_the_agent_port() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_release(
        &chart.files,
        Some("serviceAccountPrefix: release-a\n"),
        "acme",
    );
    rendered.assert_ok("sandbox policy");
    let documents = rendered_documents(&rendered);
    let policy = documents
        .iter()
        .find(|document| {
            document["kind"] == "NetworkPolicy" && document["metadata"]["name"] == "alien-sbx-agent"
        })
        .expect("the sandbox NetworkPolicy");
    let ingress = policy["spec"]["ingress"].as_sequence().expect("ingress");
    assert_eq!(
        ingress.len(),
        1,
        "one rule, for the agent port: {ingress:?}"
    );
    assert_eq!(
        ingress[0]["ports"][0]["port"].as_u64(),
        Some(u64::from(alien_core::sandbox_image::AGENT_PORT))
    );
    let peers: Vec<serde_yaml::Value> = ingress[0]["from"]
        .as_sequence()
        .expect("peers")
        .iter()
        .map(|peer| peer["podSelector"]["matchLabels"].clone())
        .collect();
    let app: serde_yaml::Value = serde_yaml::from_str(
        "alien.dev/deployment: release-a\nalien.dev/resource: app\nmanaged-by: runtime\n",
    )
    .unwrap();
    let boolean_named: serde_yaml::Value = serde_yaml::from_str(
        "alien.dev/deployment: release-a\nalien.dev/resource: \"true\"\nmanaged-by: runtime\n",
    )
    .unwrap();
    let operator: serde_yaml::Value = serde_yaml::from_str(
        "app.kubernetes.io/name: sandbox-broker-chart\napp.kubernetes.io/instance: acme\n",
    )
    .unwrap();
    assert_eq!(
        peers,
        vec![operator, app, boolean_named],
        "only the operator and the linkers, every label value a string"
    );
}

#[test]
fn platform_scoped_storage_is_absent_from_kubernetes_chart() {
    let mut input = alien_core::StackInputDefinition::deployer_boolean(
        "managedBucket",
        "Managed bucket",
        "Create a bucket",
        Some(true),
    );
    input.platforms = Some(vec![alien_core::Platform::Aws]);
    let stack = Stack::new("scoped-gate".into())
        .inputs(vec![input])
        .add_enabled_when(
            Storage::new("data".into()).build(),
            ResourceLifecycle::Frozen,
            "managedBucket",
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    assert_helm_valid(&chart, "off-platform storage");
    let values: serde_yaml::Value = serde_yaml::from_str(&chart.files["values.yaml"]).unwrap();
    assert!(values["infrastructure"]["data"].is_null());
}

/// The operator's `ALIEN_KUBERNETES_PRIORITY_CLASSES` and the PriorityClass documents the chart
/// renders for the given values, for a release named `acme`.
fn priority_render(values: &str) -> (Option<String>, Vec<serde_yaml::Value>) {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_release(
        &chart.files,
        Some(&format!("management:\n  name: customer-1\n{values}")),
        "acme",
    );
    rendered.assert_ok("workload priority values");
    let documents = rendered_documents(&rendered);
    let operator = documents
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "acme")
        .expect("the operator Deployment");
    let map = operator["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .expect("operator env")
        .iter()
        .find(|variable| variable["name"] == alien_core::ENV_ALIEN_KUBERNETES_PRIORITY_CLASSES)
        .and_then(|variable| variable["value"].as_str())
        .map(str::to_string);
    let classes = documents
        .into_iter()
        .filter(|document| document["kind"] == "PriorityClass")
        .collect();
    (map, classes)
}

/// An existing install upgraded to this chart must not restart: with nothing mapped the
/// operator's pod template gains no variable and no PriorityClass appears.
#[test]
fn workload_priority_renders_nothing_until_a_class_is_mapped() {
    let (map, classes) = priority_render("");
    assert_eq!(map, None);
    assert!(classes.is_empty());
}

#[test]
fn workload_priority_maps_each_level_to_the_class_the_admin_names() {
    let (map, classes) = priority_render(
        "runtime:\n  workloadPriority:\n    classes:\n      critical: vendor-core\n      low: vendor-batch\n",
    );
    let map: serde_json::Value = serde_json::from_str(&map.expect("env is set")).unwrap();
    assert_eq!(
        map,
        serde_json::json!({"critical": "vendor-core", "low": "vendor-batch"})
    );
    assert!(classes.is_empty(), "the chart creates nothing unless asked");
}

#[test]
fn workload_priority_create_renders_four_release_named_classes_below_system_ones() {
    let (map, classes) = priority_render("runtime:\n  workloadPriority:\n    create: true\n");
    let map: serde_json::Value = serde_json::from_str(&map.expect("env is set")).unwrap();
    let rendered: Vec<(String, i64, bool)> = classes
        .iter()
        .map(|class| {
            (
                class["metadata"]["name"].as_str().unwrap().to_string(),
                class["value"].as_i64().unwrap(),
                class["globalDefault"].as_bool().unwrap(),
            )
        })
        .collect();
    // Cluster-scoped names carry a namespace/release hash so two releases that share a
    // name in different namespaces never collide.
    let name = |level: &str| map[level].as_str().unwrap().to_string();
    for level in ["critical", "high", "normal", "low"] {
        let n = name(level);
        assert!(
            n.starts_with("acme-") && n.ends_with(&format!("-{level}")),
            "{n}"
        );
        assert_ne!(n, format!("acme-{level}"), "name must be namespace-scoped");
    }
    assert_eq!(
        rendered,
        vec![
            (name("critical"), 1_000_000, false),
            (name("high"), 100_000, false),
            (name("normal"), 0, false),
            (name("low"), -100, false),
        ]
    );
    assert!(classes[0]["description"].as_str().unwrap().contains("acme"));
}

/// With `create` on, an admin override points one level at their own class and the chart
/// creates only the remaining levels.
#[test]
fn workload_priority_create_skips_levels_the_admin_mapped_elsewhere() {
    let (map, classes) = priority_render(
        "runtime:\n  workloadPriority:\n    create: true\n    classes:\n      critical: vendor-core\n",
    );
    let map: serde_json::Value = serde_json::from_str(&map.unwrap()).unwrap();
    assert_eq!(map["critical"], "vendor-core");
    let names: Vec<_> = classes
        .iter()
        .map(|class| class["metadata"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            map["high"].as_str().unwrap(),
            map["normal"].as_str().unwrap(),
            map["low"].as_str().unwrap()
        ]
    );
}

/// An explicit mapping whose name equals a generated name is still the admin's class:
/// the chart must not try to create it.
#[test]
fn workload_priority_create_never_creates_a_class_the_admin_named() {
    let (map, classes) = priority_render(
        "runtime:\n  workloadPriority:\n    create: true\n    classes:\n      critical: acme-critical\n",
    );
    let map: serde_json::Value = serde_json::from_str(&map.unwrap()).unwrap();
    assert_eq!(map["critical"], "acme-critical");
    assert_eq!(classes.len(), 3);
    assert!(classes
        .iter()
        .all(|c| c["metadata"]["name"] != "acme-critical"));
}

/// Mapping a level to the name this release would generate anyway (for example by copying
/// the rendered map into values) must keep creating that class; otherwise the next upgrade
/// would delete a class running workloads still use.
#[test]
fn workload_priority_create_keeps_the_releases_own_class_when_mapped_to_its_name() {
    let (map, _) = priority_render("runtime:\n  workloadPriority:\n    create: true\n");
    let map: serde_json::Value = serde_json::from_str(&map.unwrap()).unwrap();
    let own = map["critical"].as_str().unwrap().to_string();
    let (mapped, classes) = priority_render(&format!(
        "runtime:\n  workloadPriority:\n    create: true\n    classes:\n      critical: {own}\n"
    ));
    let mapped: serde_json::Value = serde_json::from_str(&mapped.unwrap()).unwrap();
    assert_eq!(mapped["critical"], own.as_str());
    assert_eq!(classes.len(), 4);
    assert!(classes
        .iter()
        .any(|c| c["metadata"]["name"] == own.as_str()));
}

#[test]
fn workload_priority_schema_rejects_unknown_levels() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_release(
        &chart.files,
        Some("management:\n  name: customer-1\nruntime:\n  workloadPriority:\n    classes:\n      urgent: x\n"),
        "acme",
    );
    assert!(
        !rendered.is_ok() && rendered.stderr.contains("urgent"),
        "{}",
        rendered.stderr
    );
}

/// Legacy `allow` has no gateway: the rendered NetworkPolicy alone keeps a session off node
/// metadata, node credentials and private ranges. Evaluate the rendered policy the way the API
/// server's peers do — a destination is reachable when some rule's ports match and some peer's
/// `ipBlock` contains it outside every `except` — rather than looking for strings.
#[test]
fn an_allow_sandbox_policy_admits_only_public_ipv4_and_dns() {
    use std::net::IpAddr;

    let stack = Stack::new("sandbox-legacy-allow".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let rendered = super::test_utils::helm_template(
        &chart.files,
        Some("sandbox:\n  clusterDNS:\n    - 10.100.0.10\n"),
    );
    rendered.assert_ok("legacy allow sandbox");
    let documents = rendered_documents(&rendered);
    let policy = documents
        .iter()
        .find(|doc| doc["kind"] == "NetworkPolicy" && doc["metadata"]["name"] == "alien-sbx-agent")
        .expect("the session NetworkPolicy");
    let rules = policy["spec"]["egress"]
        .as_sequence()
        .expect("allow lists egress rules");

    let contains = |cidr: &str, ip: IpAddr| -> bool {
        let (network, prefix) = cidr.split_once('/').expect("CIDR");
        let prefix: u32 = prefix.parse().expect("prefix length");
        match (network.parse::<IpAddr>().expect("network address"), ip) {
            (IpAddr::V4(network), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
                u32::from(network) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
                u128::from(network) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    };
    let reachable = |address: &str, protocol: &str, port: u64| -> bool {
        let ip: IpAddr = address.parse().expect("test address");
        rules.iter().any(|rule| {
            let ports_match = match rule["ports"].as_sequence() {
                None => true,
                Some(ports) => ports.iter().any(|entry| {
                    entry["protocol"].as_str().unwrap_or("TCP") == protocol
                        && entry["port"].as_u64() == Some(port)
                }),
            };
            ports_match
                && rule["to"].as_sequence().is_some_and(|peers| {
                    peers.iter().any(|peer| {
                        let block = &peer["ipBlock"];
                        block["cidr"]
                            .as_str()
                            .is_some_and(|cidr| contains(cidr, ip))
                            && !block["except"].as_sequence().is_some_and(|excepts| {
                                excepts.iter().any(|except| {
                                    contains(except.as_str().expect("except CIDR"), ip)
                                })
                            })
                    })
                })
        })
    };

    for address in [
        "169.254.169.254", // instance metadata (AWS, GCP, Azure)
        "169.254.170.2",   // ECS task credentials
        "169.254.170.23",  // EKS Pod Identity agent
        "169.254.20.10",   // a node-local resolver that is not the configured one
        "169.254.169.253", // the VPC resolver: only the configured cluster DNS is admitted
        "10.100.0.11",     // another private host on port 53, next to the configured resolver
        "10.100.0.0",
        "168.63.129.16", // Azure wire server
        "100.64.0.10",   // shared address space: CGNAT, EKS custom pod networking
        "100.127.255.254",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "127.0.0.1",
        "0.0.0.1",
        "192.0.0.170",
        "198.18.0.1",
        "198.19.255.254",
        "224.0.0.251",
        "240.0.0.1",
        "255.255.255.255",
        // No IPv6 is admitted: the AWS IPv6 metadata endpoint and public IPv6 alike.
        "fd00:ec2::254",
        "::1",
        "fe80::1",
        "2606:4700:4700::1111",
    ] {
        for (protocol, port) in [
            ("TCP", 80),
            ("TCP", 443),
            ("UDP", 54),
            ("UDP", 53),
            ("TCP", 53),
        ] {
            assert!(
                !reachable(address, protocol, port),
                "{address} {protocol}/{port} must be denied: {rules:#?}"
            );
        }
    }
    for (address, protocol, port) in [
        ("8.8.8.8", "TCP", 443),
        ("1.1.1.1", "UDP", 53),
        ("203.0.114.1", "TCP", 22),
    ] {
        assert!(
            reachable(address, protocol, port),
            "{address} {protocol}/{port} must stay reachable: {rules:#?}"
        );
    }
    // A CNI that checks after the Service address is translated sees a DNS pod address, so the
    // kube-dns pods are admitted by selector, on port 53 alone.
    let kube_dns = rules
        .iter()
        .find(|rule| {
            rule["to"].as_sequence().is_some_and(|peers| {
                peers.iter().any(|peer| {
                    peer["namespaceSelector"]["matchLabels"]["kubernetes.io/metadata.name"]
                        == "kube-system"
                        && peer["podSelector"]["matchLabels"]["k8s-app"] == "kube-dns"
                })
            })
        })
        .expect("a rule admitting the cluster's DNS pods");
    assert_eq!(
        kube_dns["ports"],
        serde_yaml::from_str::<serde_yaml::Value>(
            "[{protocol: UDP, port: 53}, {protocol: TCP, port: 53}]"
        )
        .unwrap()
    );
    // The configured resolver lives in a denied private range, so it is admitted by address, on
    // port 53 alone: any other port on it stays refused.
    for (protocol, port) in [("UDP", 53), ("TCP", 53)] {
        assert!(
            reachable("10.100.0.10", protocol, port),
            "the configured cluster DNS must answer on {protocol}/{port}: {rules:#?}"
        );
    }
    for (protocol, port) in [("TCP", 80), ("TCP", 443), ("TCP", 10250), ("UDP", 54)] {
        assert!(
            !reachable("10.100.0.10", protocol, port),
            "the cluster DNS address is admitted on port 53 only, not {protocol}/{port}: {rules:#?}"
        );
    }

    // These denials are real only on a CNI that enforces NetworkPolicy, so installing and
    // upgrading the chart first proves enforcement.
    let probe = documents
        .iter()
        .find(|doc| {
            doc["kind"] == "Job"
                && doc["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-sandbox-cni"))
        })
        .expect("an allow sandbox's install verifies NetworkPolicy enforcement");
    assert_eq!(
        probe["metadata"]["annotations"]["helm.sh/hook"],
        "pre-install,pre-upgrade"
    );
}

/// The probe's client pods run where sessions run: they get `sandbox.nodeSelector` and
/// `sandbox.tolerations`, or the probe would test whichever node the scheduler preferred.
#[test]
fn the_cni_probe_runs_its_clients_on_the_nodes_sessions_use() {
    let stack = Stack::new("sandbox-probe-placement".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let rendered = super::test_utils::helm_template(
        &chart.files,
        Some("sandbox:\n  runtimeClassName: gvisor\n  clusterDNS:\n    - 10.100.0.10\n  nodeSelector:\n    pool: sandboxes\n  tolerations:\n    - key: sandbox\n      operator: Exists\n"),
    );
    rendered.assert_ok("probe placement");
    let probe = rendered_documents(&rendered)
        .into_iter()
        .find(|doc| {
            doc["kind"] == "Job"
                && doc["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-sandbox-cni"))
        })
        .expect("the NetworkPolicy probe");
    let env = |name: &str| {
        probe["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|variable| variable["name"] == name)
            .and_then(|variable| variable["value"].as_str().map(str::to_string))
            .unwrap_or_else(|| panic!("{name} missing from the probe"))
    };
    assert_eq!(env("PROBE_NODE_SELECTOR"), r#"{"pool":"sandboxes"}"#);
    assert!(env("PROBE_TOLERATIONS").contains("\"key\":\"sandbox\""));
    // Node consolidation must not evict the probe's pods mid-run (EKS Auto Mode did).
    assert_eq!(
        probe["spec"]["template"]["metadata"]["annotations"]["karpenter.sh/do-not-disrupt"],
        "true"
    );
    assert!(
        serde_yaml::to_string(&probe)
            .unwrap()
            .contains("\"annotations\": {\"karpenter.sh/do-not-disrupt\": \"true\"}"),
        "the probe's own pods carry the annotation"
    );
}

fn sandbox_runtime_class_env(rendered: &super::test_utils::LinterRun) -> Option<String> {
    rendered_documents(rendered)
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "acme")
        .and_then(|operator| {
            operator["spec"]["template"]["spec"]["containers"][0]["env"]
                .as_sequence()
                .and_then(|env| {
                    env.iter().find(|variable| {
                        variable["name"] == alien_core::ENV_ALIEN_SANDBOX_RUNTIME_CLASS
                    })
                })
                .and_then(|variable| variable["value"].as_str().map(str::to_string))
        })
}

/// The Operator learns its own pod address from the downward API, so a sandbox pod can prove its
/// NetworkPolicy refuses it before the pod takes a session.
#[test]
fn the_operator_gets_its_own_pod_address_for_the_policy_proof() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_kube_version(
        &chart.files,
        Some("management:\n  name: customer-1\nsandbox:\n  runtimeClassName: gvisor\n"),
        "acme",
        None,
    );
    let operator = rendered_documents(&rendered)
        .into_iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "acme")
        .expect("the operator Deployment");
    let env = operator["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .unwrap();
    let find = |name: &str| env.iter().find(|variable| variable["name"] == name).cloned();
    let pod_ip = find("ALIEN_OPERATOR_POD_IP").expect("the pod address variable");
    assert_eq!(pod_ip["valueFrom"]["fieldRef"]["fieldPath"], "status.podIP");
    let address = find(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_ADDRESS).expect("the address");
    let value = address["value"].as_str().unwrap();
    assert!(value.starts_with("$(ALIEN_OPERATOR_POD_IP):"), "{value}");
    // The variable must be defined before the one that expands it.
    let position = |name: &str| env.iter().position(|variable| variable["name"] == name);
    assert!(
        position("ALIEN_OPERATOR_POD_IP")
            < position(alien_core::ENV_ALIEN_SANDBOX_OPERATOR_ADDRESS)
    );
}

/// The chart, not the Operator, chooses the sandbox RuntimeClass: the value wins, and without
/// one the cluster's own version string picks the vendor class.
#[test]
fn the_sandbox_runtime_class_follows_the_value_then_the_cluster_version() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let render_for = |values: Option<&str>, version: &str| {
        super::test_utils::helm_template_for_kube_version(
            &chart.files,
            values,
            "acme",
            Some(version),
        )
    };
    for (version, expected) in [
        ("v1.36.0-eks-1234567", "isolated-sandbox"),
        ("v1.33.5-gke.1080000", "gvisor"),
    ] {
        let rendered = render_for(Some("management:\n  name: customer-1\n"), version);
        rendered.assert_ok(version);
        assert_eq!(
            sandbox_runtime_class_env(&rendered).as_deref(),
            Some(expected),
            "{version}"
        );
    }

    let rendered = render_for(
        Some("management:\n  name: customer-1\nsandbox:\n  runtimeClassName: kata-vm-isolation\n"),
        "v1.33.4",
    );
    rendered.assert_ok("explicit value on AKS");
    assert_eq!(
        sandbox_runtime_class_env(&rendered).as_deref(),
        Some("kata-vm-isolation")
    );

    // An explicit value beats the version default.
    let rendered = render_for(
        Some("management:\n  name: customer-1\nsandbox:\n  runtimeClassName: kata\n"),
        "v1.36.0-eks-1234567",
    );
    rendered.assert_ok("explicit value on EKS");
    assert_eq!(
        sandbox_runtime_class_env(&rendered).as_deref(),
        Some("kata")
    );
}

/// A cluster the chart cannot name a class for fails the render, naming the value to set.
#[test]
fn an_unknown_cluster_without_a_runtime_class_fails_the_render() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_kube_version(
        &chart.files,
        Some("management:\n  name: customer-1\n"),
        "acme",
        Some("v1.33.4"),
    );
    assert!(!rendered.is_ok(), "the render must fail");
    for needle in [
        "sandbox.runtimeClassName",
        "kata-vm-isolation",
        "gvisor",
        "isolated-sandbox",
    ] {
        assert!(
            rendered.stderr.contains(needle),
            "missing {needle}: {}",
            rendered.stderr
        );
    }
}

#[test]
fn sandbox_node_placement_values_reach_the_operator() {
    let chart = render(&sandbox_with_linked_app(), StackSettings::default());
    let rendered = super::test_utils::helm_template_for_kube_version(
        &chart.files,
        Some("management:\n  name: customer-1\nsandbox:\n  runtimeClassName: gvisor\n  warmPoolSize: 0\n  nodeSelector:\n    pool: sandboxes\n  tolerations:\n    - key: sandbox\n      operator: Exists\n"),
        "acme",
        None,
    );
    rendered.assert_ok("sandbox placement");
    let documents = rendered_documents(&rendered);
    let operator = documents
        .iter()
        .find(|document| document["kind"] == "Deployment" && document["metadata"]["name"] == "acme")
        .expect("operator");
    let env = |name: &str| {
        operator["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|variable| variable["name"] == name)
            .and_then(|variable| variable["value"].as_str().map(str::to_string))
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(
        env(alien_core::ENV_ALIEN_SANDBOX_NODE_SELECTOR),
        r#"{"pool":"sandboxes"}"#
    );
    assert!(env(alien_core::ENV_ALIEN_SANDBOX_TOLERATIONS).contains("\"key\":\"sandbox\""));
    assert_eq!(
        env(alien_core::ENV_ALIEN_SANDBOX_WARM_POOL_SIZE),
        "0",
        "an explicit zero turns the pool off; every create is cold"
    );
}

/// An upgrade of a stack with an allow-egress sandbox and no `sandbox.clusterDNS` must stop at
/// render time with a message that names the value and how to find it, so the release that is
/// running keeps its policy. Without the value the policy would admit no DNS at all.
#[test]
fn an_allow_sandbox_without_cluster_dns_fails_the_render_and_names_the_value() {
    let stack = Stack::new("sandbox-allow-no-dns".to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu:24.04".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let chart = render(&stack, StackSettings::default());
    let rendered = super::test_utils::helm_template(&chart.files, None);
    let output = format!("{rendered:?}");
    assert!(
        output.contains("sandbox.clusterDNS")
            && output.contains("grep nameserver /etc/resolv.conf"),
        "the failure must name the value and the command that finds it: {output}"
    );
    // An address that is not an IPv4 literal is refused by the values schema.
    let bad = super::test_utils::helm_template(
        &chart.files,
        Some("sandbox:\n  clusterDNS:\n    - kube-dns.kube-system\n"),
    );
    assert!(format!("{bad:?}").contains("clusterDNS"), "{bad:?}");
}

/// A chart whose only egress user is a Container has no sandbox, so no sandbox RuntimeClass
/// helper. The network-policy probe it carries must not refer to that helper.
#[test]
fn a_container_with_egress_and_no_sandbox_renders_its_probe() {
    use alien_core::{Container, ContainerCode, ContainerEgress, ResourceSpec};
    let container = Container::new("app".to_string())
        .code(ContainerCode::Image {
            image: "example.invalid/app:1".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.1".to_string(),
            desired: "0.1".to_string(),
        })
        .memory(ResourceSpec {
            min: "128Mi".to_string(),
            desired: "128Mi".to_string(),
        })
        .permissions("app".to_string())
        .egress(ContainerEgress::Internet)
        .build();
    let stack = Stack::new("container-egress-chart".to_string())
        .add(container, ResourceLifecycle::Frozen)
        .build();
    let chart = render(&stack, StackSettings::default());
    assert!(
        chart.files.contains_key("templates/sandbox-cni-probe.yaml"),
        "{:?}",
        chart.files.keys().collect::<Vec<_>>()
    );
    assert!(!chart.files.contains_key("templates/_sandbox.tpl"));
    // An air-gapped install sets the probe image whenever the chart emits the probe, so the
    // values schema of a chart with no Sandbox must accept it.
    let bundled = super::test_utils::helm_template(
        &chart.files,
        Some("sandbox:\n  cniProbeImage: docker.io/library/python:3.13-slim@sha256:aaaa\n"),
    );
    bundled.assert_ok("probe image on a chart without a sandbox");
    assert!(bundled.stdout.contains("python:3.13-slim@sha256:aaaa"), "{}", bundled.stdout);
    assert_helm_valid(&chart, "container_egress_no_sandbox");
}

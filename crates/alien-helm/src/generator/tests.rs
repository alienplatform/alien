use super::*;
use alien_core::{
    import::data::AzureApplicationGatewayForContainersBootstrap, KubernetesCluster,
    KubernetesClusterOutputs, KubernetesClusterOwnership, KubernetesClusterProvider,
    PermissionProfile, Platform, Queue, RemoteStackManagement, RemoteStackManagementOutputs,
    Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus, Stack, StackResourceState,
    StackSettings, Storage, Worker, WorkerCode, WorkerPublicEndpoint, WorkerTrigger,
};
use serde::Deserialize;
use serde_yaml::Value as YamlValue;

fn operator_test_manifest() -> String {
    generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        environment_name: Some("acme-prod-eu"),
        install_namespace: Some("demo"),
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    })
    .expect("operator manifest should render")
}

fn operator_test_manifest_with_log_collector() -> String {
    generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        image: "registry.example.com/operator:test",
        log_collector: Some(OperatorLogCollectorOptions {
            image: "fluent/fluent-bit:3.2",
            token: "collector-secret",
        }),
        project_name: "my-saas",
        environment_name: Some("acme-prod-eu"),
        install_namespace: Some("demo"),
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    })
    .expect("operator manifest should render")
}

fn operator_env_value<'a>(deployment: &'a YamlValue, name: &str) -> Option<&'a str> {
    deployment
        .get("spec")
        .and_then(|spec| spec.get("template"))
        .and_then(|template| template.get("spec"))
        .and_then(|spec| spec.get("containers"))
        .and_then(YamlValue::as_sequence)
        .and_then(|containers| containers.first())
        .and_then(|container| container.get("env"))
        .and_then(YamlValue::as_sequence)?
        .iter()
        .find(|entry| yaml_str(entry, "name") == Some(name))
        .and_then(|entry| yaml_str(entry, "value"))
}

fn parse_manifest_docs(manifest: &str) -> Vec<YamlValue> {
    serde_yaml::Deserializer::from_str(manifest)
        .map(|doc| YamlValue::deserialize(doc).expect("manifest doc should parse as YAML"))
        .filter(|doc| !doc.is_null())
        .collect()
}

fn yaml_str<'a>(value: &'a YamlValue, key: &str) -> Option<&'a str> {
    value.get(key).and_then(YamlValue::as_str)
}

fn yaml_path<'a>(value: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

fn docs_by_kind(docs: &[YamlValue], kind: &str) -> Vec<YamlValue> {
    docs.iter()
        .filter(|doc| yaml_str(doc, "kind") == Some(kind))
        .cloned()
        .collect()
}

#[test]
fn operator_manifest_renders_flat_namespaced_documents() {
    let docs = parse_manifest_docs(&operator_test_manifest());
    let kinds = docs
        .iter()
        .map(|doc| yaml_str(doc, "kind").expect("doc should have kind"))
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            "ServiceAccount",
            "Role",
            "RoleBinding",
            "Secret",
            "PersistentVolumeClaim",
            "Deployment"
        ]
    );
    assert!(
        docs_by_kind(&docs, "ClusterRole").is_empty(),
        "operator manifest must not grant cluster-scoped RBAC"
    );
    assert!(
        docs_by_kind(&docs, "ClusterRoleBinding").is_empty(),
        "operator manifest must not bind cluster-scoped RBAC"
    );
    for doc in docs {
        assert_eq!(
            yaml_path(&doc, &["metadata", "namespace"]).and_then(YamlValue::as_str),
            Some("demo"),
            "every operator document should be namespaced"
        );
    }
}

#[test]
fn operator_manifest_role_is_read_only_and_does_not_read_secrets() {
    let docs = parse_manifest_docs(&operator_test_manifest());
    let role = docs_by_kind(&docs, "Role")
        .into_iter()
        .next()
        .expect("operator manifest should include Role");
    let rules = role
        .get("rules")
        .and_then(YamlValue::as_sequence)
        .expect("Role should include rules");

    for rule in rules {
        let verbs = rule
            .get("verbs")
            .and_then(YamlValue::as_sequence)
            .expect("rule should include verbs")
            .iter()
            .map(|verb| verb.as_str().expect("verb should be string"))
            .collect::<Vec<_>>();
        assert_eq!(verbs, vec!["get", "list", "watch"]);

        let resources = rule
            .get("resources")
            .and_then(YamlValue::as_sequence)
            .expect("rule should include resources")
            .iter()
            .map(|resource| resource.as_str().expect("resource should be string"))
            .collect::<Vec<_>>();
        assert!(
            !resources.contains(&"secrets"),
            "observe Operator must not read customer Secrets"
        );
        assert!(
            !resources.contains(&"pods/log"),
            "logs must flow through the log collector, not Kubernetes API tailing"
        );
    }
}

#[test]
fn operator_manifest_deployment_uses_group_token_and_persistent_identity() {
    let docs = parse_manifest_docs(&operator_test_manifest());
    let deployment = docs_by_kind(&docs, "Deployment")
        .into_iter()
        .next()
        .expect("operator manifest should include Deployment");
    let env = deployment
        .get("spec")
        .and_then(|spec| spec.get("template"))
        .and_then(|template| template.get("spec"))
        .and_then(|spec| spec.get("containers"))
        .and_then(YamlValue::as_sequence)
        .and_then(|containers| containers.first())
        .and_then(|container| container.get("env"))
        .and_then(YamlValue::as_sequence)
        .expect("operator container should include env");
    let env_names = env
        .iter()
        .filter_map(|entry| entry.get("name").and_then(YamlValue::as_str))
        .collect::<Vec<_>>();

    assert!(env_names.contains(&"OPERATOR_SCOPE"));
    assert!(env_names.contains(&"OPERATOR_PERMISSION"));
    assert!(env_names.contains(&"OPERATOR_INITIAL_DESIRED_RELEASE"));
    assert!(env_names.contains(&"SYNC_TOKEN_FILE"));
    assert!(
        !env_names.contains(&"DEPLOYMENT_ID"),
        "first boot must self-register and then persist deployment identity"
    );

    // Object names derive from the project (stable per app); the per-environment
    // identity is carried only by OPERATOR_NAME so one project can host many envs.
    assert_eq!(
        operator_env_value(&deployment, "OPERATOR_NAME"),
        Some("acme-prod-eu"),
        "OPERATOR_NAME is the per-environment identity"
    );
    assert_eq!(
        operator_env_value(&deployment, "OPERATOR_INITIAL_DESIRED_RELEASE"),
        Some("none"),
        "a connection manifest must register without requesting a release"
    );
    assert_eq!(
        operator_env_value(&deployment, "KUBERNETES_NAMESPACE"),
        Some("demo")
    );
    assert_eq!(
        deployment
            .get("spec")
            .and_then(|spec| spec.get("template"))
            .and_then(|template| template.get("spec"))
            .and_then(|spec| spec.get("volumes"))
            .and_then(YamlValue::as_sequence)
            .and_then(|volumes| volumes.get(1))
            .and_then(|volume| volume.get("persistentVolumeClaim"))
            .and_then(|claim| claim.get("claimName"))
            .and_then(YamlValue::as_str),
        Some("my-saas-operator-identity")
    );

    let secret = docs_by_kind(&docs, "Secret")
        .into_iter()
        .next()
        .expect("operator manifest should include Secret");
    assert_eq!(
        secret
            .get("stringData")
            .and_then(|data| data.get("sync-token"))
            .and_then(YamlValue::as_str),
        Some("ax_dg_test")
    );
}

#[test]
fn operator_manifest_can_include_log_collector_without_control_plane_credentials() {
    let manifest = operator_test_manifest_with_log_collector();
    let docs = parse_manifest_docs(&manifest);
    let kinds = docs
        .iter()
        .map(|doc| yaml_str(doc, "kind").expect("doc should have kind"))
        .collect::<Vec<_>>();

    assert!(kinds.contains(&"Service"));
    assert!(kinds.contains(&"Role"));
    assert!(kinds.contains(&"RoleBinding"));
    assert!(kinds.contains(&"DaemonSet"));
    assert!(manifest.contains("whitelabeled-log-collector"));
    assert!(manifest.contains("/var/log/pods/demo_"));
    assert!(manifest.contains("/internal/logs"));
    assert!(manifest.contains("COLLECTOR_TOKEN_FILE"));
    assert!(manifest.contains("collector-token"));
    assert!(manifest.contains("fluent/fluent-bit:3.2"));
    assert!(!manifest.contains("pods/log"));
    assert!(!manifest.contains("void"));

    let collector_role = docs_by_kind(&docs, "Role")
        .into_iter()
        .find(|role| {
            role.get("metadata")
                .and_then(|metadata| metadata.get("name"))
                .and_then(YamlValue::as_str)
                .is_some_and(|name| name.ends_with("-whitelabeled-log-collector"))
        })
        .expect("operator manifest should include collector Role");
    let resources = collector_role
        .get("rules")
        .and_then(YamlValue::as_sequence)
        .and_then(|rules| rules.first())
        .and_then(|rule| rule.get("resources"))
        .and_then(YamlValue::as_sequence)
        .expect("collector Role should include resources")
        .iter()
        .filter_map(YamlValue::as_str)
        .collect::<Vec<_>>();
    assert_eq!(resources, vec!["pods"]);

    let daemonset = docs_by_kind(&docs, "DaemonSet")
        .into_iter()
        .next()
        .expect("operator manifest should include collector DaemonSet");
    let env = daemonset
        .get("spec")
        .and_then(|spec| spec.get("template"))
        .and_then(|template| template.get("spec"))
        .and_then(|spec| spec.get("containers"))
        .and_then(YamlValue::as_sequence)
        .and_then(|containers| containers.first())
        .and_then(|container| container.get("env"))
        .and_then(YamlValue::as_sequence)
        .expect("collector container should include env");
    let env_names = env
        .iter()
        .filter_map(|entry| entry.get("name").and_then(YamlValue::as_str))
        .collect::<Vec<_>>();
    assert_eq!(env_names, vec!["COLLECTOR_TOKEN"]);
}

#[test]
fn operator_manifest_emits_label_selector_only_when_scoped() {
    // Default test scope has no label selector.
    let docs = parse_manifest_docs(&operator_test_manifest());
    let deployment = docs_by_kind(&docs, "Deployment")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        operator_env_value(&deployment, "OPERATOR_LABEL_SELECTOR"),
        None
    );

    // Namespace scope grants a namespaced Role, no cluster-wide RBAC.
    assert_eq!(docs_by_kind(&docs, "Role").len(), 1);
    assert!(docs_by_kind(&docs, "ClusterRole").is_empty());

    // Label scope is cluster-wide: emits the selector env and ClusterRole/
    // ClusterRoleBinding instead of a namespaced Role.
    let manifest = generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        environment_name: Some("acme-prod-eu"),
        install_namespace: Some("demo"),
        scope: OperatorScope::Cluster,
        label_selector: Some("app.kubernetes.io/part-of=my-saas"),
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    })
    .expect("operator manifest should render");
    let docs = parse_manifest_docs(&manifest);
    let deployment = docs_by_kind(&docs, "Deployment")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        operator_env_value(&deployment, "OPERATOR_LABEL_SELECTOR"),
        Some("app.kubernetes.io/part-of=my-saas")
    );

    assert!(
        docs_by_kind(&docs, "Role").is_empty(),
        "cluster-wide scope must not emit a namespaced Role"
    );
    let cluster_role = docs_by_kind(&docs, "ClusterRole")
        .into_iter()
        .next()
        .expect("label scope should emit a ClusterRole");
    assert!(
        yaml_path(&cluster_role, &["metadata", "namespace"]).is_none(),
        "ClusterRole must not be namespaced"
    );
    let crb = docs_by_kind(&docs, "ClusterRoleBinding")
        .into_iter()
        .next()
        .expect("label scope should emit a ClusterRoleBinding");
    let subject = crb
        .get("subjects")
        .and_then(YamlValue::as_sequence)
        .and_then(|s| s.first())
        .expect("ClusterRoleBinding should have a subject");
    assert_eq!(
        subject.get("namespace").and_then(YamlValue::as_str),
        Some("demo"),
        "ClusterRoleBinding subject must reference the install namespace"
    );
}

#[test]
fn operator_helm_template_sources_namespace_and_identity_from_helm() {
    let manifest = generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        // Ignored for Helm output — the value comes from .Values / .Release per install.
        environment_name: None,
        install_namespace: None,
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::HelmTemplate,
    })
    .expect("helm template should render");

    let docs = parse_manifest_docs(&manifest);
    for doc in &docs {
        assert_eq!(
            yaml_path(doc, &["metadata", "namespace"]).and_then(YamlValue::as_str),
            Some("{{ .Release.Namespace }}"),
            "helm documents install into the release namespace"
        );
    }
    let deployment = docs_by_kind(&docs, "Deployment")
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        operator_env_value(&deployment, "OPERATOR_NAME"),
        Some("{{ .Values.alien.environmentName }}"),
        "each install registers under its own environment name"
    );
    // Object names still come from the project, not from any environment.
    assert_eq!(
        yaml_path(&deployment, &["metadata", "name"]).and_then(YamlValue::as_str),
        Some("my-saas-operator")
    );
}

#[test]
fn raw_manifest_requires_install_namespace_and_environment_name() {
    let missing_namespace = generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        environment_name: Some("acme"),
        install_namespace: None,
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    });
    assert!(
        missing_namespace.is_err(),
        "raw output needs an install namespace"
    );

    let missing_env = generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        environment_name: None,
        install_namespace: Some("demo"),
        scope: OperatorScope::Namespace,
        label_selector: None,
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    });
    assert!(missing_env.is_err(), "raw output needs an environment name");

    // Label scope must carry a non-empty selector.
    let empty_label = generate_operator_manifest(OperatorManifestOptions {
        manager_url: "https://manager.example.com",
        group_token: "ax_dg_test",
        encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        image: "registry.example.com/operator:test",
        log_collector: None,
        project_name: "my-saas",
        environment_name: Some("acme"),
        install_namespace: Some("demo"),
        scope: OperatorScope::Cluster,
        label_selector: Some("   "),
        permission: OperatorPermission::Observe,
        format: OperatorOutputFormat::RawManifest,
    });
    assert!(
        empty_label.is_err(),
        "label scope needs a non-empty selector"
    );
}

const TEST_RUNTIME_ENCRYPTION_KEY: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sample_stack() -> Stack {
    let storage = Storage::new("assets".to_string()).versioning(true).build();
    let queue = Queue::new("jobs".to_string()).build();
    let function = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "example.com/api:1".to_string(),
        })
        .permissions("runtime".to_string())
        .public_endpoint(WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: None,
            wildcard_subdomains: false,
        })
        .link(&storage)
        .trigger(WorkerTrigger::queue(&queue))
        .build();

    Stack::new("sample-stack".to_string())
        .permission(
            "runtime",
            PermissionProfile::new().global(["storage/data-read"]),
        )
        .add(storage, ResourceLifecycle::Frozen)
        .add(queue, ResourceLifecycle::Frozen)
        .add(function, ResourceLifecycle::Live)
        .build()
}

#[test]
fn generated_chart_lints_and_templates() {
    let registry = HelmRegistry::built_in();
    let chart = generate_helm_chart(
        &sample_stack(),
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: "sample-stack".to_string(),
        },
    )
    .expect("chart should render");

    let files = chart.files.clone();
    crate::test_utils::helm_lint(&files).assert_ok("helm chart");
    crate::test_utils::helm_template_and_validate(&files, None)
        .assert_ok("helm template registered setup");
    crate::test_utils::helm_template_and_validate(&files, Some(&files["examples/onprem.yaml"]))
        .assert_ok("helm template external-bindings initialize path");
}

#[test]
fn log_collector_enabled_chart_lints_and_templates() {
    let registry = HelmRegistry::built_in();
    let chart = generate_helm_chart(
        &sample_stack(),
        HelmOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            chart_name: "sample-stack".to_string(),
        },
    )
    .expect("chart should render");

    let values = r#"
logCollector:
  enabled: true
  token: test-collector-token
  scope:
    deploymentLabelValue: e2e123
"#;

    let files = chart.files.clone();
    crate::test_utils::helm_template_and_validate(&files, Some(values))
        .assert_ok("helm template log collector");
    let rendered = crate::test_utils::helm_template(&files, Some(values));
    rendered.assert_ok("helm render log collector");
    assert!(rendered.stdout.contains("kind: DaemonSet"));
    assert!(rendered.stdout.contains("whitelabeled-log-collector"));
    assert!(rendered.stdout.contains("COLLECTOR_TOKEN_FILE"));
    assert!(rendered.stdout.contains("/var/log/pods/default_"));
    assert!(rendered.stdout.contains("fluent/fluent-bit:3.2"));
    assert!(rendered
        .stdout
        .contains("$kubernetes['labels']['alien.dev/deployment'] ^e2e123$"));
    assert!(rendered.stdout.contains("resources: [\"pods\"]"));
    assert!(rendered
        .stdout
        .contains("verbs: [\"get\", \"list\", \"watch\"]"));
    assert!(!rendered.stdout.contains("resources: [\"pods/log\"]"));
    assert!(!rendered.stdout.contains("void"));
}

#[test]
fn registered_setup_values_include_runtime_encryption_key() {
    let stack_state =
        alien_core::StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());

    let values = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        stack: &sample_stack(),
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Aws),
        region: Some("us-east-1"),
        gcp_project_id: None,
        azure_location: None,
    })
    .expect("registered setup values should render");

    assert!(values.contains("runtime:\n  encryption:\n"));
    assert!(values.contains(&format!("    key: '{}'", TEST_RUNTIME_ENCRYPTION_KEY)));

    let values_yaml: YamlValue =
        serde_yaml::from_str(&values).expect("registered setup values should parse");
    assert_eq!(
        yaml_path(
            &values_yaml,
            &["logCollector", "scope", "deploymentLabelValue"]
        )
        .and_then(YamlValue::as_str),
        Some("e2e123")
    );
}

#[test]
fn registered_setup_values_reject_invalid_runtime_encryption_key() {
    let stack_state =
        alien_core::StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());

    let error = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: "replace-me-with-a-stable-64-character-encryption-secret",
        stack: &sample_stack(),
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Aws),
        region: Some("us-east-1"),
        gcp_project_id: None,
        azure_location: None,
    })
    .expect_err("invalid runtime encryption key should fail");

    assert!(error
        .to_string()
        .contains("runtime encryption key must be exactly 64 hex characters"));
}

#[test]
fn registered_setup_values_enable_eks_cluster_bootstrap_from_registered_config() {
    let cluster = KubernetesCluster::new("kubernetes".to_string())
        .provider(KubernetesClusterProvider::Eks)
        .ownership(KubernetesClusterOwnership::Managed)
        .namespace("alien-test".to_string())
        .heartbeat_mode(alien_core::KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
        .build();
    let stack = Stack::new("sample-stack".to_string())
        .add(cluster.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut stack_state =
        alien_core::StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());
    stack_state.resources.insert(
        "kubernetes".to_string(),
        StackResourceState::new_pending(
            KubernetesCluster::RESOURCE_TYPE.to_string(),
            Resource::new(cluster),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        ),
    );

    let values = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        stack: &stack,
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Aws),
        region: Some("us-east-1"),
        gcp_project_id: None,
        azure_location: None,
    })
    .expect("registered setup values should render");

    assert!(values.contains("clusterBootstrap:"));
    assert!(
        values.contains("storageClass:\n    default:\n      enabled: true\n      name: \"gp3\"")
    );
    assert!(values.contains("ingress:\n    eksAutoMode:\n      enabled: true\n      name: alb"));
    assert!(
        values.contains("compute:\n    eksAutoMode:\n      arm64NodePool:\n        enabled: true")
    );
}

#[test]
fn registered_setup_values_use_azure_workload_identity_client_id() {
    let mut stack_state =
        alien_core::StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());
    let rsm = RemoteStackManagement::new("management".to_string()).build();
    stack_state.resources.insert(
        "management".to_string(),
        StackResourceState::new_pending(
            RemoteStackManagement::RESOURCE_TYPE.to_string(),
            Resource::new(rsm),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/manager".to_string(),
                access_configuration: serde_json::json!({
                    "uamiClientId": "11111111-2222-3333-4444-555555555555",
                    "tenantId": "tenant"
                })
                .to_string(),
            }));
        }),
    );

    let values = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        stack: &sample_stack(),
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Azure),
        region: Some("eastus"),
        gcp_project_id: None,
        azure_location: Some("eastus"),
    })
    .expect("registered setup values should render");

    assert!(values
        .contains("'azure.workload.identity/client-id': '11111111-2222-3333-4444-555555555555'"));
    assert!(!values.contains("'azure.workload.identity/client-id': '/subscriptions/sub"));
    assert!(values.contains("azure.workload.identity/use: 'true'"));
    assert!(values.contains("subscriptionId: 'sub'"));
    assert!(values.contains("tenantId: 'tenant'"));
}

#[test]
fn registered_setup_values_include_azure_agc_cluster_bootstrap() {
    let mut stack_state =
        alien_core::StackState::with_resource_prefix(Platform::Kubernetes, "e2e123".into());
    let cluster = KubernetesCluster::new("kubernetes".to_string())
        .provider(KubernetesClusterProvider::Aks)
        .ownership(KubernetesClusterOwnership::Managed)
        .namespace("alien-test".to_string())
        .heartbeat_mode(alien_core::KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
        .build();
    stack_state.resources.insert(
        "kubernetes".to_string(),
        StackResourceState::new_pending(
            KubernetesCluster::RESOURCE_TYPE.to_string(),
            Resource::new(cluster),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(KubernetesClusterOutputs {
                provider: KubernetesClusterProvider::Aks,
                ownership: KubernetesClusterOwnership::Managed,
                namespace: "alien-test".to_string(),
                cluster_name: Some("e2e-k8s".to_string()),
                cluster_id: Some("e2e-k8s".to_string()),
                kubernetes_api_reachable: true,
                namespace_ready: true,
                rbac_ready: true,
                operator_ready: false,
                cloud_metadata_ready: Some(true),
                azure_application_gateway_for_containers: Some(
                    AzureApplicationGatewayForContainersBootstrap {
                        alb_name: "e2e-alb".to_string(),
                        alb_namespace: "alien-test".to_string(),
                        association_subnet_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/alb".to_string(),
                    },
                ),
                version: None,
                status_message: None,
            }));
        }),
    );

    let values = render_manager_fetch_values(ManagerFetchHelmValuesOptions {
        deployment_id: "dep_123",
        deployment_name: "deployment",
        manager_url: "https://management.example.com",
        deployment_token: "token",
        runtime_encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
        stack: &sample_stack(),
        stack_state: &stack_state,
        stack_settings: &StackSettings::default(),
        base_platform: Some(Platform::Azure),
        region: Some("eastus"),
        gcp_project_id: None,
        azure_location: Some("eastus"),
    })
    .expect("registered setup values should render");

    assert!(values.contains("azureApplicationGatewayForContainers:"));
    assert!(values.contains("enabled: true"));
    assert!(values.contains("name: 'e2e-alb'"));
    assert!(values.contains("namespace: 'alien-test'"));
    assert!(values.contains(
        "associationSubnetId: '/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet/subnets/alb'"
    ));
}

#[test]
fn fullname_defaults_to_release_name() {
    let helpers = helpers_tpl();

    assert!(helpers.contains("{{- .Release.Name | trunc 63 | trimSuffix \"-\" -}}"));
    assert!(!helpers.contains("printf \"%s-%s\" .Release.Name"));
}

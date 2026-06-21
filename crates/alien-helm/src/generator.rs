//! Top-level Helm chart generator.
//!
//! Drives per-resource [`HelmEmitter`]s through the [`HelmRegistry`] and
//! assembles the chart shell — `Chart.yaml`, the templates, and the
//! values + schema for both bootstrap paths (`registered setup` when
//! `management.deploymentId` is set; external-bindings initialize otherwise).

use crate::{
    emitter::{HelmFragment, InfrastructureValue},
    registry::HelmRegistry,
};
use alien_core::{
    import::EmitContext, AzureResourceGroupOutputs, Container, ContainerCode, Daemon, DaemonCode,
    ErrorData, ExposeProtocol, Ingress, KubernetesCluster, KubernetesClusterOutputs,
    KubernetesClusterOwnership, KubernetesClusterProvider, Platform, RemoteStackManagementOutputs,
    ResourceLifecycle, Result, ServiceAccount, ServiceAccountOutputs, Stack, StackSettings, Worker,
    WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Generated Helm chart files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelmChart {
    pub name: String,
    pub files: IndexMap<String, String>,
}

/// Options for Helm chart generation.
pub struct HelmOptions<'a> {
    /// Per-`(ResourceType, Platform)` emitter dispatch. Most callers
    /// pass [`HelmRegistry::built_in()`]; plugin-aware callers extend it
    /// before passing.
    pub registry: &'a HelmRegistry,
    pub stack_settings: StackSettings,
    pub chart_name: String,
}

/// Inputs for rendering `values.yaml` from registered setup state.
pub struct ManagerFetchHelmValuesOptions<'a> {
    pub deployment_id: &'a str,
    pub deployment_name: &'a str,
    pub manager_url: &'a str,
    pub deployment_token: &'a str,
    pub runtime_encryption_key: &'a str,
    pub stack: &'a Stack,
    pub stack_state: &'a alien_core::StackState,
    pub stack_settings: &'a StackSettings,
    pub base_platform: Option<Platform>,
    pub region: Option<&'a str>,
    pub gcp_project_id: Option<&'a str>,
    pub azure_location: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorPermission {
    /// Namespaced, read-only workload observation.
    Observe,
}

impl OperatorPermission {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorScope {
    pub namespaces: Vec<String>,
}

impl OperatorScope {
    pub fn single_namespace(namespace: impl Into<String>) -> Self {
        Self {
            namespaces: vec![namespace.into()],
        }
    }
}

pub struct OperatorManifestOptions<'a> {
    pub manager_url: &'a str,
    pub group_token: &'a str,
    pub encryption_key: &'a str,
    pub image: &'a str,
    pub namespace: &'a str,
    pub scope: OperatorScope,
    pub permission: OperatorPermission,
    pub release_name: &'a str,
}

/// Generate a Helm chart for `stack`.
pub fn generate_helm_chart(stack: &Stack, options: HelmOptions<'_>) -> Result<HelmChart> {
    let chart_name = sanitize_chart_name(&options.chart_name);
    let analysis = ChartAnalysis::from_stack(stack, options.registry)?;

    let stack_json = to_stable_pretty_json(stack).context(ErrorData::JsonSerializationFailed {
        reason: "failed to serialize stack into chart metadata".to_string(),
    })?;
    let stack_settings_json = to_stable_pretty_json(&options.stack_settings).context(
        ErrorData::JsonSerializationFailed {
            reason: "failed to serialize stack settings into chart metadata".to_string(),
        },
    )?;

    let mut files = IndexMap::new();
    files.insert("Chart.yaml".to_string(), chart_yaml(&chart_name, stack));
    files.insert(
        "values.yaml".to_string(),
        values_yaml(&analysis, &options.stack_settings)?,
    );
    files.insert("values.schema.json".to_string(), values_schema_json());
    files.insert("templates/_helpers.tpl".to_string(), helpers_tpl());
    files.insert(
        "templates/serviceaccount.yaml".to_string(),
        serviceaccount_tpl(),
    );
    files.insert("templates/role.yaml".to_string(), role_tpl());
    files.insert("templates/rolebinding.yaml".to_string(), rolebinding_tpl());
    files.insert("templates/clusterrole.yaml".to_string(), clusterrole_tpl());
    files.insert(
        "templates/clusterrolebinding.yaml".to_string(),
        clusterrolebinding_tpl(),
    );
    files.insert("templates/secret.yaml".to_string(), secret_tpl());
    files.insert("templates/configmap.yaml".to_string(), configmap_tpl());
    files.insert("templates/deployment.yaml".to_string(), deployment_tpl());
    files.insert("templates/pvc.yaml".to_string(), pvc_tpl());
    files.insert("templates/service.yaml".to_string(), service_tpl());
    files.insert("templates/cleanup-job.yaml".to_string(), cleanup_job_tpl());
    files.insert("templates/app-service.yaml".to_string(), app_service_tpl());
    files.insert(
        "templates/cluster-bootstrap.yaml".to_string(),
        cluster_bootstrap_tpl(),
    );
    files.insert(
        "templates/poddisruptionbudget.yaml".to_string(),
        poddisruptionbudget_tpl(),
    );
    files.insert(
        "templates/networkpolicy.yaml".to_string(),
        networkpolicy_tpl(),
    );

    // Per-resource extra templates contributed by emitters.
    for (path, contents) in &analysis.extra_templates {
        files.insert(format!("templates/{path}"), contents.clone());
    }

    files.insert(
        "examples/eks.yaml".to_string(),
        eks_values_example(&analysis),
    );
    files.insert(
        "examples/gke.yaml".to_string(),
        gke_values_example(&analysis),
    );
    files.insert(
        "examples/aks.yaml".to_string(),
        aks_values_example(&analysis),
    );
    files.insert(
        "examples/onprem.yaml".to_string(),
        onprem_values_example(&analysis),
    );
    files.insert("README.md".to_string(), readme_md(&chart_name, stack));

    files.insert(
        "files/stack.json".to_string(),
        ensure_trailing_newline(stack_json),
    );
    files.insert(
        "files/stack-settings.json".to_string(),
        ensure_trailing_newline(stack_settings_json),
    );

    Ok(HelmChart {
        name: chart_name,
        files,
    })
}

pub fn generate_operator_manifest(options: OperatorManifestOptions<'_>) -> Result<String> {
    validate_runtime_encryption_key(options.encryption_key)?;
    validate_operator_scope(&options.scope)?;

    let base_name = sanitize_chart_name(options.release_name);
    let operator_name = format!("{base_name}-operator");
    let identity_pvc_name = format!("{operator_name}-identity");
    let namespace = options.namespace;
    let observed_namespace = &options.scope.namespaces[0];
    let labels = operator_labels(&base_name);

    let mut docs = Vec::new();
    docs.push(operator_service_account_doc(
        namespace,
        &operator_name,
        &labels,
    ));
    docs.push(operator_role_doc(namespace, &operator_name, &labels));
    docs.push(operator_rolebinding_doc(namespace, &operator_name, &labels));
    docs.push(operator_secret_doc(
        namespace,
        &operator_name,
        options.group_token,
        options.encryption_key,
        &labels,
    ));
    docs.push(operator_identity_pvc_doc(
        namespace,
        &identity_pvc_name,
        &labels,
    ));
    docs.push(operator_deployment_doc(
        namespace,
        &operator_name,
        &identity_pvc_name,
        &options,
        observed_namespace,
        &labels,
    ));

    Ok(ensure_trailing_newline(docs.join("---\n")))
}

/// Render one complete values file from registered deployment state.
pub fn render_manager_fetch_values(options: ManagerFetchHelmValuesOptions<'_>) -> Result<String> {
    validate_runtime_encryption_key(options.runtime_encryption_key)?;

    let registry = HelmRegistry::built_in();
    let analysis = ChartAnalysis::from_stack(options.stack, &registry)?;
    let mut yaml = String::new();

    yaml.push_str("management:\n");
    yaml.push_str(&format!(
        "  token: {}\n",
        yaml_string(options.deployment_token)
    ));
    yaml.push_str(&format!(
        "  name: {}\n",
        yaml_string(options.deployment_name)
    ));
    yaml.push_str(&format!("  url: {}\n", yaml_string(options.manager_url)));
    yaml.push_str(&format!(
        "  deploymentId: {}\n",
        yaml_string(options.deployment_id)
    ));
    yaml.push_str(&format!(
        "  updates: {}\n",
        yaml_string(updates_mode_value(options.stack_settings.updates))
    ));
    yaml.push_str(&format!(
        "  telemetry: {}\n",
        yaml_string(telemetry_mode_value(options.stack_settings.telemetry))
    ));
    yaml.push_str(&format!(
        "  healthChecks: {}\n\n",
        yaml_string(heartbeats_mode_value(options.stack_settings.heartbeats))
    ));

    yaml.push_str("runtime:\n");
    yaml.push_str("  encryption:\n");
    yaml.push_str(&format!(
        "    key: {}\n\n",
        yaml_string(options.runtime_encryption_key)
    ));

    append_stack_settings(&mut yaml, options.stack_settings)?;
    yaml.push_str("\ninfrastructure: null\n\n");

    match options.base_platform {
        Some(platform) => yaml.push_str(&format!(
            "basePlatform: {}\n",
            yaml_string(platform.as_str())
        )),
        None => yaml.push_str("basePlatform: null\n"),
    }
    yaml.push_str("basePlatformConfig:\n");
    yaml.push_str("  gcp:\n");
    yaml.push_str(&format!(
        "    projectId: {}\n",
        yaml_string(options.gcp_project_id.unwrap_or(""))
    ));
    yaml.push_str(&format!(
        "    region: {}\n",
        yaml_string(options.region.unwrap_or(""))
    ));
    yaml.push_str("  aws:\n");
    yaml.push_str(&format!(
        "    region: {}\n",
        yaml_string(options.region.unwrap_or(""))
    ));
    yaml.push_str("  azure:\n");
    yaml.push_str(&format!(
        "    location: {}\n",
        yaml_string(options.azure_location.or(options.region).unwrap_or(""))
    ));
    if let Some(azure_config) =
        azure_base_platform_config(options.stack_state, options.base_platform)?
    {
        yaml.push_str(&format!(
            "    subscriptionId: {}\n",
            yaml_string(&azure_config.subscription_id)
        ));
        if let Some(tenant_id) = azure_config.tenant_id {
            yaml.push_str(&format!("    tenantId: {}\n", yaml_string(&tenant_id)));
        }
    }
    yaml.push_str(&format!(
        "serviceAccountPrefix: {}\n",
        yaml_string(&options.stack_state.resource_prefix)
    ));

    append_manager_service_account(&mut yaml, options.stack_state, options.base_platform)?;
    append_registered_service_accounts(
        &mut yaml,
        &analysis,
        options.stack_state,
        options.base_platform,
    );
    append_runtime_cloud_identity(&mut yaml, options.base_platform);
    append_cluster_bootstrap(
        &mut yaml,
        options.stack,
        options.stack_state,
        options.base_platform,
    );
    append_services(&mut yaml, &analysis);
    yaml.push_str("\npublicUrls: {}\n");

    Ok(yaml)
}

fn validate_runtime_encryption_key(key: &str) -> Result<()> {
    if key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::GenericError {
        message: "runtime encryption key must be exactly 64 hex characters".to_string(),
    }))
}

fn validate_operator_scope(scope: &OperatorScope) -> Result<()> {
    match scope.namespaces.as_slice() {
        [namespace] if !namespace.trim().is_empty() => Ok(()),
        [_] => Err(AlienError::new(ErrorData::GenericError {
            message: "operator scope namespace must not be empty".to_string(),
        })),
        [] => Err(AlienError::new(ErrorData::GenericError {
            message: "operator scope must include one namespace".to_string(),
        })),
        _ => Err(AlienError::new(ErrorData::GenericError {
            message: "operator manifest supports exactly one namespace in v1".to_string(),
        })),
    }
}

fn operator_labels(base_name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), "operator".to_string()),
        (
            "app.kubernetes.io/instance".to_string(),
            base_name.to_string(),
        ),
        (
            "app.kubernetes.io/component".to_string(),
            "operator".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            "kubectl".to_string(),
        ),
    ])
}

fn operator_service_account_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, operator_name, labels);
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_role_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        operator_name,
        labels,
    );
    yaml.push_str(
        r#"rules:
  - apiGroups: [""]
    resources: ["pods", "services", "configmaps", "persistentvolumeclaims", "events", "endpoints"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs", "cronjobs"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
"#,
    );
    yaml
}

fn operator_rolebinding_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "RoleBinding",
        namespace,
        operator_name,
        labels,
    );
    yaml.push_str(&format!(
        r#"subjects:
  - kind: ServiceAccount
    name: {}
    namespace: {}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {}
"#,
        yaml_string(operator_name),
        yaml_string(namespace),
        yaml_string(operator_name)
    ));
    yaml
}

fn operator_secret_doc(
    namespace: &str,
    operator_name: &str,
    group_token: &str,
    encryption_key: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "Secret", namespace, operator_name, labels);
    yaml.push_str("type: Opaque\n");
    yaml.push_str("stringData:\n");
    yaml.push_str(&format!("  sync-token: {}\n", yaml_string(group_token)));
    yaml.push_str(&format!(
        "  encryption-key: {}\n",
        yaml_string(encryption_key)
    ));
    yaml
}

fn operator_identity_pvc_doc(
    namespace: &str,
    pvc_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml =
        operator_metadata_doc("v1", "PersistentVolumeClaim", namespace, pvc_name, labels);
    yaml.push_str(
        r#"spec:
  accessModes: ["ReadWriteOnce"]
  resources:
    requests:
      storage: "1Gi"
"#,
    );
    yaml
}

fn operator_deployment_doc(
    namespace: &str,
    operator_name: &str,
    identity_pvc_name: &str,
    options: &OperatorManifestOptions<'_>,
    observed_namespace: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("apps/v1", "Deployment", namespace, operator_name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  replicas: 1\n");
    yaml.push_str("  selector:\n");
    yaml.push_str("    matchLabels:\n");
    append_operator_selector_labels(&mut yaml, labels, 6);
    yaml.push_str("  template:\n");
    yaml.push_str("    metadata:\n");
    yaml.push_str("      labels:\n");
    append_operator_labels(&mut yaml, labels, 8);
    yaml.push_str("    spec:\n");
    yaml.push_str(&format!(
        "      serviceAccountName: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("      automountServiceAccountToken: true\n");
    yaml.push_str("      securityContext:\n");
    yaml.push_str("        runAsNonRoot: true\n");
    yaml.push_str("        runAsUser: 1000\n");
    yaml.push_str("        runAsGroup: 1000\n");
    yaml.push_str("        fsGroup: 1000\n");
    yaml.push_str("        seccompProfile:\n");
    yaml.push_str("          type: RuntimeDefault\n");
    yaml.push_str("      containers:\n");
    yaml.push_str("        - name: operator\n");
    yaml.push_str(&format!(
        "          image: {}\n",
        yaml_string(options.image)
    ));
    yaml.push_str("          imagePullPolicy: IfNotPresent\n");
    yaml.push_str("          securityContext:\n");
    yaml.push_str("            allowPrivilegeEscalation: false\n");
    yaml.push_str("            readOnlyRootFilesystem: true\n");
    yaml.push_str("            capabilities:\n");
    yaml.push_str("              drop: [\"ALL\"]\n");
    yaml.push_str("          env:\n");
    append_env_value(&mut yaml, "PLATFORM", "kubernetes");
    append_env_value(&mut yaml, "SYNC_URL", options.manager_url);
    append_env_value(&mut yaml, "OPERATOR_NAME", options.release_name);
    append_env_value(&mut yaml, "KUBERNETES_NAMESPACE", namespace);
    append_env_value(&mut yaml, "OPERATOR_SCOPE", observed_namespace);
    append_env_value(
        &mut yaml,
        "OPERATOR_PERMISSION",
        options.permission.as_str(),
    );
    append_env_value(&mut yaml, "DATA_DIR", "/var/lib/operator");
    append_env_value(
        &mut yaml,
        "SYNC_TOKEN_FILE",
        "/etc/operator/secrets/sync-token",
    );
    append_env_value(
        &mut yaml,
        "OPERATOR_ENCRYPTION_KEY_FILE",
        "/etc/operator/secrets/encryption-key",
    );
    append_env_value(&mut yaml, "SYNC_INTERVAL", "30");
    yaml.push_str("          volumeMounts:\n");
    yaml.push_str("            - name: credentials\n");
    yaml.push_str("              mountPath: /etc/operator/secrets\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: identity\n");
    yaml.push_str("              mountPath: /var/lib/operator\n");
    yaml.push_str("            - name: tmp\n");
    yaml.push_str("              mountPath: /tmp\n");
    yaml.push_str("          resources:\n");
    yaml.push_str("            requests:\n");
    yaml.push_str("              cpu: 50m\n");
    yaml.push_str("              memory: 128Mi\n");
    yaml.push_str("            limits:\n");
    yaml.push_str("              cpu: 500m\n");
    yaml.push_str("              memory: 512Mi\n");
    yaml.push_str("      volumes:\n");
    yaml.push_str("        - name: credentials\n");
    yaml.push_str("          secret:\n");
    yaml.push_str(&format!(
        "            secretName: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("            defaultMode: 384\n");
    yaml.push_str("        - name: identity\n");
    yaml.push_str("          persistentVolumeClaim:\n");
    yaml.push_str(&format!(
        "            claimName: {}\n",
        yaml_string(identity_pvc_name)
    ));
    yaml.push_str("        - name: tmp\n");
    yaml.push_str("          emptyDir:\n");
    yaml.push_str("            sizeLimit: 64Mi\n");
    yaml
}

fn operator_metadata_doc(
    api_version: &str,
    kind: &str,
    namespace: &str,
    name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = String::new();
    yaml.push_str(&format!("apiVersion: {}\n", yaml_string(api_version)));
    yaml.push_str(&format!("kind: {}\n", yaml_string(kind)));
    yaml.push_str("metadata:\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(name)));
    yaml.push_str(&format!("  namespace: {}\n", yaml_string(namespace)));
    yaml.push_str("  labels:\n");
    append_operator_labels(&mut yaml, labels, 4);
    yaml
}

fn append_operator_selector_labels(
    yaml: &mut String,
    labels: &BTreeMap<String, String>,
    indent: usize,
) {
    for key in ["app.kubernetes.io/name", "app.kubernetes.io/instance"] {
        if let Some(value) = labels.get(key) {
            yaml.push_str(&format!(
                "{}{}: {}\n",
                " ".repeat(indent),
                yaml_key(key),
                yaml_string(value)
            ));
        }
    }
}

fn append_operator_labels(yaml: &mut String, labels: &BTreeMap<String, String>, indent: usize) {
    for (key, value) in labels {
        yaml.push_str(&format!(
            "{}{}: {}\n",
            " ".repeat(indent),
            yaml_key(key),
            yaml_string(value)
        ));
    }
}

fn append_env_value(yaml: &mut String, name: &str, value: &str) {
    yaml.push_str(&format!("            - name: {}\n", yaml_string(name)));
    yaml.push_str(&format!("              value: {}\n", yaml_string(value)));
}

/// Result of dispatching every stack resource through the
/// `HelmRegistry`. Aggregated values land in `values.yaml`; extra
/// templates land under `templates/`.
#[derive(Debug, Default)]
struct ChartAnalysis {
    service_accounts: BTreeSet<String>,
    service_account_rbac: BTreeMap<String, Vec<KubernetesRoleRule>>,
    infrastructure: Vec<InfrastructureValue>,
    services: Vec<ServiceValue>,
    extra_templates: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KubernetesRoleRule {
    api_groups: Vec<&'static str>,
    resources: Vec<&'static str>,
    verbs: Vec<&'static str>,
}

impl ChartAnalysis {
    fn from_stack(stack: &Stack, registry: &HelmRegistry) -> Result<Self> {
        let mut analysis = Self::default();

        let mut service_accounts = stack
            .permission_profiles()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let service_account_rbac = stack
            .permission_profiles()
            .iter()
            .filter_map(|(name, profile)| {
                let rules = kubernetes_rbac_rules_for_permission_profile(profile);
                (!rules.is_empty()).then(|| (name.clone(), rules))
            })
            .collect::<BTreeMap<_, _>>();

        let names = IndexMap::new();
        let stack_settings = StackSettings::default();

        for (resource_id, entry) in stack.resources() {
            if let Some(function) = entry.config.downcast_ref::<Worker>() {
                fail_if_worker_source_remains(resource_id, function)?;
                service_accounts.insert(function.permissions.clone());
                if function.ingress == Ingress::Public {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "worker".to_string(),
                        target_port: 8080,
                    });
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                fail_if_container_source_remains(resource_id, container)?;
                if let Some(port) = container
                    .ports
                    .iter()
                    .find(|port| port.expose == Some(ExposeProtocol::Http))
                {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "container".to_string(),
                        target_port: port.port,
                    });
                }
            }
            if let Some(build) = entry.config.downcast_ref::<alien_core::Build>() {
                service_accounts.insert(build.permissions.clone());
            }
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                fail_if_daemon_source_remains(resource_id, daemon)?;
            }

            // Frozen resources contribute operator-local infrastructure bindings; live
            // (workload) resources do not — they ARE the workload.
            if entry.lifecycle != ResourceLifecycle::Frozen {
                continue;
            }

            let resource_type = entry.config.resource_type();
            let Some(emitter) = registry.emitter(&resource_type, Platform::Kubernetes) else {
                continue;
            };

            let ctx = EmitContext {
                stack,
                resource: entry,
                resource_id,
                platform: Platform::Kubernetes,
                stack_settings: &stack_settings,
                names: &names,
            };
            let HelmFragment {
                infrastructure,
                extra_templates,
            } = emitter.emit(&ctx)?;
            if let Some(value) = infrastructure {
                analysis.infrastructure.push(value);
            }
            for (path, contents) in extra_templates {
                analysis.extra_templates.insert(path, contents);
            }
        }

        analysis.service_accounts = service_accounts;
        analysis.service_account_rbac = service_account_rbac;
        Ok(analysis)
    }
}

fn kubernetes_rbac_rules_for_permission_profile(
    profile: &alien_core::PermissionProfile,
) -> Vec<KubernetesRoleRule> {
    let mut secret_verbs = BTreeSet::new();
    let mut needs_jobs = false;

    for permission in profile.0.values().flatten() {
        match permission.id() {
            "vault/data-read" => {
                secret_verbs.extend(["get", "list", "watch"]);
            }
            "vault/data-write" => {
                secret_verbs.extend([
                    "get", "list", "watch", "create", "update", "patch", "delete",
                ]);
            }
            "build/execute" => {
                needs_jobs = true;
            }
            _ => {}
        }
    }

    let mut rules = Vec::new();
    if !secret_verbs.is_empty() {
        rules.push(KubernetesRoleRule {
            api_groups: vec![""],
            resources: vec!["secrets"],
            verbs: secret_verbs.into_iter().collect(),
        });
    }
    if needs_jobs {
        rules.push(KubernetesRoleRule {
            api_groups: vec!["batch"],
            resources: vec!["jobs"],
            verbs: vec!["get", "list", "watch", "create", "delete"],
        });
    }

    rules
}

fn fail_if_worker_source_remains(resource_id: &str, worker: &Worker) -> Result<()> {
    if matches!(&worker.code, WorkerCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Worker '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

fn fail_if_container_source_remains(resource_id: &str, container: &Container) -> Result<()> {
    if matches!(&container.code, ContainerCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Container '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

fn fail_if_daemon_source_remains(resource_id: &str, daemon: &Daemon) -> Result<()> {
    if matches!(&daemon.code, DaemonCode::Source { .. }) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "Daemon '{resource_id}' still has source code before Helm chart generation; build and inject an image first"
            ),
        }));
    }
    Ok(())
}

#[derive(Debug)]
struct ServiceValue {
    id: String,
    component: String,
    target_port: u16,
}

fn to_stable_pretty_json<T: Serialize>(value: &T) -> alien_error::Result<String> {
    let value = serde_json::to_value(value).into_alien_error()?;
    serde_json::to_string_pretty(&sort_json_object_keys(value)).into_alien_error()
}

fn sort_json_object_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(sort_json_object_keys)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, sort_json_object_keys(value));
            }
            serde_json::Value::Object(sorted)
        }
        value => value,
    }
}

fn chart_yaml(chart_name: &str, stack: &Stack) -> String {
    format!(
        "apiVersion: v2\nname: {chart_name}\ndescription: Deployment chart for {stack_id}\ntype: application\nversion: 0.1.0\nappVersion: \"0.1.0\"\n",
        stack_id = stack.id()
    )
}

fn values_yaml(analysis: &ChartAnalysis, stack_settings: &StackSettings) -> Result<String> {
    let mut yaml = String::new();
    yaml.push_str(
        r#"management:
  token: ""
  existingSecret:
    name: ""
    tokenKey: sync-token
  name: ""
  url: ""
  deploymentId: "dep_replace_me"
  updates: auto
  telemetry: auto
  healthChecks: "on"

runtime:
  image:
    repository: registry.example.com/deployment/operator
    tag: latest
    pullPolicy: IfNotPresent
  imagePullSecrets: []
  podLabels: {}
  podAnnotations: {}
  automountServiceAccountToken: true
  encryption:
    # Set this explicitly, or reference an existing Secret below.
    key: "replace-me-with-a-stable-64-character-encryption-secret"
    existingSecret:
      name: ""
      key: encryption-key
  replicas: 1
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      memory: 512Mi
  api:
    enabled: false
    bindHost: 0.0.0.0
    port: 8080
    service:
      type: ClusterIP
  probes:
    liveness:
      enabled: true
      path: /health
      initialDelaySeconds: 10
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
    readiness:
      enabled: true
      path: /health
      initialDelaySeconds: 5
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
  security:
    podSecurityContext:
      runAsNonRoot: true
      runAsUser: 10001
      runAsGroup: 10001
      fsGroup: 10001
      seccompProfile:
        type: RuntimeDefault
    containerSecurityContext:
      allowPrivilegeEscalation: false
      readOnlyRootFilesystem: true
      capabilities:
        drop:
          - ALL
  tmp:
    enabled: true
    sizeLimit: 256Mi
  data:
    mountPath: /var/lib/deployment-operator
    persistence:
      enabled: false
      existingClaim: ""
      storageClassName: ""
      accessModes:
        - ReadWriteOnce
      size: 1Gi
  scheduling:
    nodeSelector: {}
    tolerations: []
    affinity: {}
    topologySpreadConstraints: []
    priorityClassName: ""
    runtimeClassName: ""
  pdb:
    enabled: false
    minAvailable: 1
  networkPolicy:
    enabled: false
    ingress:
      enabled: true
    egress:
      enabled: true
  cleanup:
    onUninstall:
      enabled: true
      deletePersistentVolumeClaims: false
      image:
        repository: bitnami/kubectl
        tag: "1.32"
        pullPolicy: IfNotPresent

heartbeat:
  collection:
    nodes:
      enabled: true

clusterBootstrap:
  metricsServer:
    enabled: false
    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1
  storageClass:
    default:
      enabled: false
      name: ""
      provisioner: ""
      parameters: {}
  ingress:
    eksAutoMode:
      enabled: false
      name: alb
      controller: eks.amazonaws.com/alb
      scheme: internet-facing
      subnetIds: []
    azureApplicationGatewayForContainers:
      enabled: false
      applicationLoadBalancer:
        name: ""
        namespace: ""
        associationSubnetId: ""
  compute:
    eksAutoMode:
      arm64NodePool:
        enabled: false
        name: general-purpose-arm64
        nodeClassName: default
        capacityType: on-demand
        instanceCategories:
          - c
          - m
          - r
        minInstanceGeneration: "5"
        limits:
          cpu: "1000"
          memory: 1000Gi

"#,
    );

    append_service_accounts(&mut yaml, analysis);
    append_stack_settings(&mut yaml, stack_settings)?;
    yaml.push_str("\ninfrastructure: null\n\nbasePlatform: null\nbasePlatformConfig:\n  gcp:\n    projectId: \"\"\n    region: \"\"\n  aws:\n    region: \"\"\n  azure:\n    location: \"\"\n    subscriptionId: \"\"\n    tenantId: \"\"\nserviceAccountPrefix: \"\"\nmanagerServiceAccount:\n  annotations: {}\n  labels: {}\n");
    append_services(&mut yaml, analysis);
    yaml.push_str("\npublicUrls: {}\n");

    yaml.push_str(
        r#"
persistentStorage:
  storageClassName: ""

ephemeralStorage:
  nodeSelector: {}
"#,
    );
    Ok(yaml)
}

fn append_stack_settings(yaml: &mut String, stack_settings: &StackSettings) -> Result<()> {
    if stack_settings == &StackSettings::default() {
        yaml.push_str("\nstackSettings: null\n");
        return Ok(());
    }

    let serialized = serde_yaml::to_string(stack_settings)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize stack settings into chart values".to_string(),
        })?;
    let serialized = serialized
        .strip_prefix("---\n")
        .unwrap_or(&serialized)
        .trim_end();

    if serialized == "{}" || serialized.is_empty() {
        yaml.push_str("\nstackSettings: null\n");
        return Ok(());
    }

    yaml.push_str("\nstackSettings:\n");
    for line in serialized.lines() {
        yaml.push_str("  ");
        yaml.push_str(line);
        yaml.push('\n');
    }

    Ok(())
}

fn append_service_accounts(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for name in &analysis.service_accounts {
            yaml.push_str(&format!(
                "  {}:\n    annotations: {{}}\n    labels: {{}}\n",
                yaml_key(name)
            ));
            append_service_account_rbac(yaml, analysis.service_account_rbac.get(name));
        }
    }
}

fn append_registered_service_accounts(
    yaml: &mut String,
    analysis: &ChartAnalysis,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
        return;
    }

    for name in &analysis.service_accounts {
        yaml.push_str(&format!("  {}:\n", yaml_key(name)));
        match service_account_identity_for_profile(stack_state, name) {
            Some(identity) => {
                yaml.push_str("    annotations:\n");
                yaml.push_str(&format!(
                    "      {}: {}\n",
                    yaml_key(identity_annotation_key(base_platform)),
                    yaml_string(identity)
                ));
            }
            None => yaml.push_str("    annotations: {}\n"),
        }
        yaml.push_str("    labels: {}\n");
        append_service_account_rbac(yaml, analysis.service_account_rbac.get(name));
    }
}

fn append_service_account_rbac(yaml: &mut String, rules: Option<&Vec<KubernetesRoleRule>>) {
    let Some(rules) = rules.filter(|rules| !rules.is_empty()) else {
        return;
    };

    yaml.push_str("    rbac:\n");
    yaml.push_str("      rules:\n");
    for rule in rules {
        yaml.push_str("        - apiGroups: ");
        append_yaml_inline_string_list(yaml, &rule.api_groups);
        yaml.push('\n');
        yaml.push_str("          resources: ");
        append_yaml_inline_string_list(yaml, &rule.resources);
        yaml.push('\n');
        yaml.push_str("          verbs: ");
        append_yaml_inline_string_list(yaml, &rule.verbs);
        yaml.push('\n');
    }
}

fn append_yaml_inline_string_list(yaml: &mut String, values: &[&str]) {
    yaml.push('[');
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            yaml.push_str(", ");
        }
        yaml.push_str(&yaml_string(value));
    }
    yaml.push(']');
}

fn append_manager_service_account(
    yaml: &mut String,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<()> {
    yaml.push_str("managerServiceAccount:\n");
    match remote_stack_management_identity(stack_state, base_platform)? {
        Some(identity) => {
            yaml.push_str("  annotations:\n");
            yaml.push_str(&format!(
                "    {}: {}\n",
                yaml_key(identity_annotation_key(base_platform)),
                yaml_string(&identity)
            ));
        }
        None => yaml.push_str("  annotations: {}\n"),
    }
    if base_platform == Some(Platform::Azure) {
        yaml.push_str("  labels:\n");
        yaml.push_str("    azure.workload.identity/use: 'true'\n");
    } else {
        yaml.push_str("  labels: {}\n");
    }
    Ok(())
}

fn append_runtime_cloud_identity(yaml: &mut String, base_platform: Option<Platform>) {
    if base_platform != Some(Platform::Azure) {
        return;
    }

    yaml.push_str("runtime:\n");
    yaml.push_str("  podLabels:\n");
    yaml.push_str("    azure.workload.identity/use: 'true'\n");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AzureBasePlatformConfig {
    subscription_id: String,
    tenant_id: Option<String>,
}

fn azure_base_platform_config(
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<Option<AzureBasePlatformConfig>> {
    if base_platform != Some(Platform::Azure) {
        return Ok(None);
    }

    let subscription_id = stack_state
        .resources
        .values()
        .find_map(|resource| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
                .and_then(|outputs| {
                    azure_subscription_id_from_resource_id(&outputs.management_resource_id)
                })
        })
        .or_else(|| {
            stack_state.resources.values().find_map(|resource| {
                resource
                    .outputs
                    .as_ref()
                    .and_then(|outputs| outputs.downcast_ref::<AzureResourceGroupOutputs>())
                    .and_then(|outputs| {
                        azure_subscription_id_from_resource_id(&outputs.resource_id)
                    })
            })
        });

    let tenant_id = azure_remote_stack_management_access_config(stack_state)?
        .and_then(|access_config| access_config.tenant_id);

    Ok(
        subscription_id.map(|subscription_id| AzureBasePlatformConfig {
            subscription_id,
            tenant_id,
        }),
    )
}

fn azure_subscription_id_from_resource_id(resource_id: &str) -> Option<String> {
    let mut parts = resource_id.split('/').filter(|part| !part.is_empty());
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("subscriptions") {
            return parts.next().map(str::to_string);
        }
    }
    None
}

fn append_cluster_bootstrap(
    yaml: &mut String,
    stack: &Stack,
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) {
    let eks_managed =
        base_platform == Some(Platform::Aws) && managed_eks_cluster_present(stack, stack_state);

    yaml.push_str("clusterBootstrap:\n");
    yaml.push_str("  metricsServer:\n");
    yaml.push_str(&format!("    enabled: {}\n", eks_managed));
    yaml.push_str("    image: registry.k8s.io/metrics-server/metrics-server:v0.8.1\n");
    yaml.push_str("  storageClass:\n");
    yaml.push_str("    default:\n");
    yaml.push_str(&format!("      enabled: {}\n", eks_managed));
    yaml.push_str("      name: \"gp3\"\n");
    yaml.push_str("      provisioner: \"ebs.csi.eks.amazonaws.com\"\n");
    yaml.push_str("      parameters:\n");
    yaml.push_str("        type: \"gp3\"\n");
    yaml.push_str("        fsType: \"ext4\"\n");
    yaml.push_str("        encrypted: \"true\"\n");
    yaml.push_str("  ingress:\n");
    yaml.push_str("    eksAutoMode:\n");
    yaml.push_str(&format!("      enabled: {}\n", eks_managed));
    yaml.push_str("      name: alb\n");
    yaml.push_str("      controller: eks.amazonaws.com/alb\n");
    yaml.push_str("      scheme: internet-facing\n");
    yaml.push_str("      subnetIds: []\n");
    yaml.push_str("    azureApplicationGatewayForContainers:\n");
    match azure_application_gateway_for_containers_bootstrap(stack_state) {
        Some(bootstrap) => {
            yaml.push_str("      enabled: true\n");
            yaml.push_str("      applicationLoadBalancer:\n");
            yaml.push_str(&format!(
                "        name: {}\n",
                yaml_string(&bootstrap.alb_name)
            ));
            yaml.push_str(&format!(
                "        namespace: {}\n",
                yaml_string(&bootstrap.alb_namespace)
            ));
            yaml.push_str(&format!(
                "        associationSubnetId: {}\n",
                yaml_string(&bootstrap.association_subnet_id)
            ));
        }
        None => {
            yaml.push_str("      enabled: false\n");
            yaml.push_str("      applicationLoadBalancer:\n");
            yaml.push_str("        name: \"\"\n");
            yaml.push_str("        namespace: \"\"\n");
            yaml.push_str("        associationSubnetId: \"\"\n");
        }
    }
    yaml.push_str("  compute:\n");
    yaml.push_str("    eksAutoMode:\n");
    yaml.push_str("      arm64NodePool:\n");
    yaml.push_str(&format!("        enabled: {}\n", eks_managed));
    yaml.push_str("        name: general-purpose-arm64\n");
    yaml.push_str("        nodeClassName: default\n");
    yaml.push_str("        capacityType: on-demand\n");
    yaml.push_str("        instanceCategories:\n");
    yaml.push_str("          - c\n");
    yaml.push_str("          - m\n");
    yaml.push_str("          - r\n");
    yaml.push_str("        minInstanceGeneration: \"5\"\n");
    yaml.push_str("        limits:\n");
    yaml.push_str("          cpu: \"1000\"\n");
    yaml.push_str("          memory: 1000Gi\n");
}

fn managed_eks_cluster_present(stack: &Stack, stack_state: &alien_core::StackState) -> bool {
    stack_state.resources.values().any(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<KubernetesClusterOutputs>())
            .is_some_and(is_managed_eks_cluster_outputs)
            || resource
                .config
                .downcast_ref::<KubernetesCluster>()
                .is_some_and(is_managed_eks_cluster_config)
    }) || stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<KubernetesCluster>()
            .is_some_and(is_managed_eks_cluster_config)
    })
}

fn is_managed_eks_cluster_outputs(outputs: &KubernetesClusterOutputs) -> bool {
    outputs.provider == KubernetesClusterProvider::Eks
        && outputs.ownership == KubernetesClusterOwnership::Managed
}

fn is_managed_eks_cluster_config(cluster: &KubernetesCluster) -> bool {
    cluster.provider == KubernetesClusterProvider::Eks
        && cluster.ownership == KubernetesClusterOwnership::Managed
}

fn azure_application_gateway_for_containers_bootstrap(
    stack_state: &alien_core::StackState,
) -> Option<&alien_core::import::data::AzureApplicationGatewayForContainersBootstrap> {
    stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<alien_core::KubernetesClusterOutputs>())
            .and_then(|outputs| outputs.azure_application_gateway_for_containers.as_ref())
    })
}

fn service_account_identity_for_profile<'a>(
    stack_state: &'a alien_core::StackState,
    profile: &str,
) -> Option<&'a str> {
    stack_state
        .resources
        .iter()
        .find_map(|(resource_id, resource)| {
            let outputs = resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<ServiceAccountOutputs>())?;
            let service_account = resource.config.downcast_ref::<ServiceAccount>()?;
            let account_profile =
                alien_core::permission_profile_from_service_account_id(service_account.id());
            (account_profile == profile || resource_id == profile)
                .then_some(outputs.identity.as_str())
        })
}

fn remote_stack_management_identity(
    stack_state: &alien_core::StackState,
    base_platform: Option<Platform>,
) -> Result<Option<String>> {
    let Some(outputs) = stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
    }) else {
        return Ok(None);
    };

    if base_platform == Some(Platform::Azure) {
        let Some(access_config) = azure_remote_stack_management_access_config(stack_state)? else {
            return Ok(None);
        };
        return Ok(Some(access_config.uami_client_id));
    }

    Ok(Some(outputs.management_resource_id.clone()))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureRemoteStackManagementAccessConfig {
    uami_client_id: String,
    tenant_id: Option<String>,
}

fn azure_remote_stack_management_access_config(
    stack_state: &alien_core::StackState,
) -> Result<Option<AzureRemoteStackManagementAccessConfig>> {
    let Some(outputs) = stack_state.resources.values().find_map(|resource| {
        resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
    }) else {
        return Ok(None);
    };

    let access_config: AzureRemoteStackManagementAccessConfig =
        serde_json::from_str(&outputs.access_configuration)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "Failed to parse Azure management access configuration".to_string(),
            })?;

    if access_config.uami_client_id.is_empty() {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "Azure management access configuration is missing uamiClientId".to_string(),
        }));
    }

    Ok(Some(access_config))
}

fn identity_annotation_key(base_platform: Option<Platform>) -> &'static str {
    match base_platform {
        Some(Platform::Gcp) => "iam.gke.io/gcp-service-account",
        Some(Platform::Azure) => "azure.workload.identity/client-id",
        _ => "eks.amazonaws.com/role-arn",
    }
}

fn updates_mode_value(mode: alien_core::UpdatesMode) -> &'static str {
    match mode {
        alien_core::UpdatesMode::Auto => "auto",
        alien_core::UpdatesMode::ApprovalRequired => "approval-required",
    }
}

fn telemetry_mode_value(mode: alien_core::TelemetryMode) -> &'static str {
    match mode {
        alien_core::TelemetryMode::Off => "off",
        alien_core::TelemetryMode::Auto => "auto",
        alien_core::TelemetryMode::ApprovalRequired => "approval-required",
    }
}

fn heartbeats_mode_value(mode: alien_core::HeartbeatsMode) -> &'static str {
    match mode {
        alien_core::HeartbeatsMode::Off => "off",
        alien_core::HeartbeatsMode::On => "on",
    }
}

fn append_infrastructure(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("infrastructure:\n");
    if analysis.infrastructure.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for resource in &analysis.infrastructure {
            yaml.push_str(&format!(
                "  {}:\n    type: {}\n    service: {}\n",
                yaml_key(&resource.id),
                resource.binding_type,
                resource.service
            ));
            for (key, value) in &resource.fields {
                let value = if value == "null" {
                    "null".to_string()
                } else {
                    yaml_string(value)
                };
                yaml.push_str(&format!("    {key}: {value}\n"));
            }
        }
    }
}

fn append_services(yaml: &mut String, analysis: &ChartAnalysis) {
    yaml.push_str("\nservices:\n");
    if analysis.services.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for service in &analysis.services {
            yaml.push_str(&format!(
                "  {}:\n    type: clusterIp\n    port: 80\n    targetPort: {}\n    component: {}\n",
                yaml_key(&service.id),
                service.target_port,
                yaml_string(&service.component)
            ));
        }
    }
}

fn values_schema_json() -> String {
    r##"{
  "$schema": "https://json-schema.org/draft-07/schema#",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "nameOverride": { "type": "string" },
    "fullnameOverride": { "type": "string" },
    "management": {
      "type": "object",
      "additionalProperties": false,
      "required": ["token", "updates", "telemetry", "healthChecks"],
      "properties": {
        "token": { "type": "string" },
        "existingSecret": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "name": { "type": "string" },
            "tokenKey": { "type": "string", "minLength": 1 }
          }
        },
        "name": { "type": "string" },
        "url": { "type": "string" },
        "deploymentId": { "type": ["string", "null"] },
        "updates": { "type": "string", "enum": ["auto", "approval-required"] },
        "telemetry": { "type": "string", "enum": ["auto", "approval-required", "off"] },
        "healthChecks": { "type": "string", "enum": ["on", "off"] }
      }
    },
    "runtime": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "image": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "repository": { "type": "string", "minLength": 1 },
            "tag": { "type": "string", "minLength": 1 },
            "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
          }
        },
        "imagePullSecrets": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name"],
            "properties": { "name": { "type": "string", "minLength": 1 } }
          }
        },
        "podLabels": { "type": "object", "additionalProperties": { "type": "string" } },
        "podAnnotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "automountServiceAccountToken": { "type": "boolean" },
        "encryption": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "key": { "type": "string" },
            "existingSecret": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "name": { "type": "string" },
                "key": { "type": "string", "minLength": 1 }
              }
            }
          }
        },
        "replicas": { "type": "integer", "minimum": 1 },
        "resources": { "type": "object" },
        "api": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "bindHost": { "type": "string" },
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "service": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "type": { "type": "string", "enum": ["ClusterIP", "NodePort", "LoadBalancer"] }
              }
            }
          }
        },
        "probes": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "liveness": { "$ref": "#/definitions/httpProbe" },
            "readiness": { "$ref": "#/definitions/httpProbe" }
          }
        },
        "security": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "podSecurityContext": { "type": "object" },
            "containerSecurityContext": { "type": "object" }
          }
        },
        "tmp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "sizeLimit": { "type": "string" }
          }
        },
        "data": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "mountPath": { "type": "string", "minLength": 1 },
            "persistence": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "existingClaim": { "type": "string" },
                "storageClassName": { "type": "string" },
                "accessModes": { "type": "array", "items": { "type": "string" } },
                "size": { "type": "string" }
              }
            }
          }
        },
        "scheduling": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodeSelector": { "type": "object", "additionalProperties": { "type": "string" } },
            "tolerations": { "type": "array" },
            "affinity": { "type": "object" },
            "topologySpreadConstraints": { "type": "array" },
            "priorityClassName": { "type": "string" },
            "runtimeClassName": { "type": "string" }
          }
        },
        "pdb": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "minAvailable": { "type": ["integer", "string"] },
            "maxUnavailable": { "type": ["integer", "string"] }
          }
        },
        "networkPolicy": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "ingress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            },
            "egress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            }
          }
        },
        "cleanup": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "onUninstall": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "deletePersistentVolumeClaims": { "type": "boolean" },
                "image": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "repository": { "type": "string", "minLength": 1 },
                    "tag": { "type": "string", "minLength": 1 },
                    "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
                  }
                }
              }
            }
          }
        }
      }
    },
    "managerServiceAccount": {
      "type": "object",
      "properties": {
        "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "labels": { "type": "object", "additionalProperties": { "type": "string" } }
      }
    },
    "serviceAccounts": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
          "labels": { "type": "object", "additionalProperties": { "type": "string" } },
          "rbac": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "rules": {
                "type": "array",
                "items": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "apiGroups": {
                      "type": "array",
                      "items": { "type": "string" }
                    },
                    "resources": {
                      "type": "array",
                      "items": { "type": "string", "minLength": 1 }
                    },
                    "verbs": {
                      "type": "array",
                      "items": { "type": "string", "minLength": 1 }
                    }
                  },
                  "required": ["apiGroups", "resources", "verbs"]
                }
              }
            }
          }
        }
      }
    },
    "stackSettings": {
      "type": ["object", "null"],
      "properties": {
        "deploymentModel": { "type": "string", "enum": ["pull", "Pull"] },
        "updates": { "type": "string" },
        "telemetry": { "type": "string" },
        "heartbeats": { "type": "string" }
      },
      "additionalProperties": true
    },
    "infrastructure": { "type": ["object", "null"] },
    "basePlatform": { "type": ["string", "null"], "enum": ["aws", "gcp", "azure", null] },
    "basePlatformConfig": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "gcp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "projectId": { "type": "string" },
            "region": { "type": "string" }
          }
        },
        "aws": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "region": { "type": "string" }
          }
        },
        "azure": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "location": { "type": "string" },
            "subscriptionId": { "type": "string" },
            "tenantId": { "type": "string" }
          }
        }
      }
    },
    "heartbeat": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "collection": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodes": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" }
              }
            }
          }
        }
      }
    },
    "clusterBootstrap": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "metricsServer": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "image": { "type": "string" }
          }
        },
        "storageClass": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "default": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "provisioner": { "type": "string" },
                "parameters": { "type": "object", "additionalProperties": { "type": "string" } }
              }
            }
          }
        },
        "ingress": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "controller": { "type": "string" },
                "scheme": { "type": "string" },
                "subnetIds": {
                  "type": "array",
                  "items": { "type": "string" }
                }
              }
            },
            "azureApplicationGatewayForContainers": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "applicationLoadBalancer": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "name": { "type": "string" },
                    "namespace": { "type": "string" },
                    "associationSubnetId": { "type": "string" }
                  }
                }
              }
            }
          }
        },
        "compute": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "arm64NodePool": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "enabled": { "type": "boolean" },
                    "name": { "type": "string" },
                    "nodeClassName": { "type": "string" },
                    "capacityType": { "type": "string" },
                    "instanceCategories": {
                      "type": "array",
                      "items": { "type": "string" }
                    },
                    "minInstanceGeneration": { "type": "string" },
                    "limits": {
                      "type": "object",
                      "additionalProperties": false,
                      "properties": {
                        "cpu": { "type": "string" },
                        "memory": { "type": "string" }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "serviceAccountPrefix": { "type": "string" },
    "services": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "type": { "type": "string", "enum": ["clusterIp", "loadBalancer"] },
          "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "targetPort": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "component": { "type": "string" }
        }
      }
    },
    "publicUrls": { "type": "object", "additionalProperties": { "type": "string" } },
    "persistentStorage": { "type": "object" },
    "ephemeralStorage": { "type": "object" }
  },
  "definitions": {
    "httpProbe": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "path": { "type": "string", "minLength": 1 },
        "initialDelaySeconds": { "type": "integer", "minimum": 0 },
        "periodSeconds": { "type": "integer", "minimum": 1 },
        "timeoutSeconds": { "type": "integer", "minimum": 1 },
        "failureThreshold": { "type": "integer", "minimum": 1 }
      }
    }
  },
  "oneOf": [
    {
      "title": "registered setup",
      "required": ["management"],
      "properties": {
        "management": {
          "required": ["token", "deploymentId"],
          "properties": {
            "deploymentId": { "type": "string", "minLength": 1 }
          }
        },
        "infrastructure": { "type": "null" }
      }
    },
    {
      "title": "external-bindings initialize path",
      "required": ["management", "infrastructure"],
      "properties": {
        "management": {
          "properties": {
            "deploymentId": { "type": "null" }
          }
        },
        "stackSettings": { "type": ["object", "null"] },
        "infrastructure": { "type": "object" }
      }
    }
  ]
}
"##
    .to_string()
}

fn helpers_tpl() -> String {
    r#"{{- define "deployment.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "deployment.labels" -}}
app.kubernetes.io/name: {{ include "deployment.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "deployment.managerServiceAccountName" -}}
{{- $prefix := default (include "deployment.fullname" .) .Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-manager-sa" $prefix | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.serviceAccountName" -}}
{{- $prefix := default (include "deployment.fullname" .root) .root.Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-%s-sa" $prefix .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.resourceName" -}}
{{- $raw := .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.managementSecretName" -}}
{{- default (include "deployment.fullname" .) .Values.management.existingSecret.name -}}
{{- end -}}

{{- define "deployment.managementSecretTokenKey" -}}
{{- default "sync-token" .Values.management.existingSecret.tokenKey -}}
{{- end -}}

{{- define "deployment.encryptionSecretName" -}}
{{- default (include "deployment.fullname" .) .Values.runtime.encryption.existingSecret.name -}}
{{- end -}}

{{- define "deployment.encryptionSecretKey" -}}
{{- default "encryption-key" .Values.runtime.encryption.existingSecret.key -}}
{{- end -}}

{{- define "deployment.heartbeatNodeClusterRoleName" -}}
{{- printf "%s-heartbeat-nodes" (include "deployment.fullname" .) | trunc 63 | trimSuffix "-" -}}
{{- end -}}
"#
    .to_string()
}

fn serviceaccount_tpl() -> String {
    r#"apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.managerServiceAccountName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    {{- with .Values.managerServiceAccount.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with .Values.managerServiceAccount.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- range $name, $account := .Values.serviceAccounts }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
    {{- with $account.labels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  {{- with $account.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
---
{{- end }}
"#
    .to_string()
}

fn role_tpl() -> String {
    r#"{{- $stackSettings := default dict .Values.stackSettings -}}
{{- $exposure := dig "kubernetes" "exposure" dict $stackSettings -}}
{{- $exposureMode := dig "mode" "" $exposure -}}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps", "secrets", "services", "pods", "pods/log", "persistentvolumeclaims"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: [""]
    resources: ["events"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments", "statefulsets", "daemonsets", "replicasets"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["batch"]
    resources: ["jobs"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["networkpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["networking.k8s.io"]
    resources: ["ingresses"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["gateway.networking.k8s.io"]
    resources: ["gateways", "httproutes"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- $route := dig "route" dict $exposure -}}
  {{- $routeApi := dig "routeApi" "" $route -}}
  {{- if and (ne $exposureMode "disabled") (eq $routeApi "gateway") }}
  - apiGroups: ["networking.gke.io"]
    resources: ["healthcheckpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["alb.networking.azure.io"]
    resources: ["healthcheckpolicy"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  {{- end }}
"#
    .to_string()
}

fn rolebinding_tpl() -> String {
    r#"apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.fullname" . }}
---
{{- range $name, $account := .Values.serviceAccounts }}
{{- $rbac := default dict $account.rbac }}
{{- $rules := default list $rbac.rules }}
{{- if $rules }}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
rules:
{{- toYaml $rules | nindent 2 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
---
{{- end }}
{{- end }}
"#
    .to_string()
}

fn clusterrole_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["metrics.k8s.io"]
    resources: ["nodes"]
    verbs: ["get", "list", "watch"]
{{- end }}
"#
    .to_string()
}

fn clusterrolebinding_tpl() -> String {
    r#"{{- $nodeCollectionEnabled := dig "collection" "nodes" "enabled" true (default dict .Values.heartbeat) -}}
{{- if $nodeCollectionEnabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.managerServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "deployment.heartbeatNodeClusterRoleName" . }}
{{- end }}
"#
    .to_string()
}

fn secret_tpl() -> String {
    r#"{{- $createManagementSecret := not .Values.management.existingSecret.name -}}
{{- $createEncryptionSecret := not .Values.runtime.encryption.existingSecret.name -}}
{{- if or $createManagementSecret $createEncryptionSecret .Values.infrastructure }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
type: Opaque
stringData:
  {{- if $createManagementSecret }}
  sync-token: {{ .Values.management.token | quote }}
  {{- end }}
  {{- if $createEncryptionSecret }}
  encryption-key: {{ required "runtime.encryption.key or runtime.encryption.existingSecret.name is required" .Values.runtime.encryption.key | quote }}
  {{- end }}
  {{- if .Values.infrastructure }}
  external-bindings.json: {{ toJson .Values.infrastructure | quote }}
  {{- end }}
{{- end }}
"#
    .to_string()
}

fn configmap_tpl() -> String {
    r#"{{- $defaultStackSettings := dict "deploymentModel" "pull" "updates" .Values.management.updates "telemetry" .Values.management.telemetry "heartbeats" .Values.management.healthChecks -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
data:
  stack.json: |-
{{ .Files.Get "files/stack.json" | indent 4 }}
  stack-settings.json: {{ toJson (default $defaultStackSettings .Values.stackSettings) | quote }}
  services.json: {{ toJson .Values.services | quote }}
  public-urls.json: {{ toJson (default dict .Values.publicUrls) | quote }}
"#
    .to_string()
}

fn cleanup_job_tpl() -> String {
    r#"{{- $cleanup := dig "cleanup" "onUninstall" dict .Values.runtime -}}
{{- if dig "enabled" true $cleanup }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.fullname" . }}-cleanup
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-delete
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
spec:
  backoffLimit: 1
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      restartPolicy: Never
      containers:
        - name: cleanup
          image: "{{ dig "image" "repository" "bitnami/kubectl" $cleanup }}:{{ dig "image" "tag" "1.32" $cleanup }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" $cleanup }}
          command:
            - /bin/sh
            - -ec
            - |
              selector='managed-by=runtime'
              kubectl -n {{ .Release.Namespace | quote }} delete deployments.apps,statefulsets.apps,daemonsets.apps,services,configmaps,secrets,networkpolicies.networking.k8s.io,ingresses.networking.k8s.io -l "$selector" --ignore-not-found=true
              if kubectl api-resources --api-group gateway.networking.k8s.io --no-headers 2>/dev/null | awk '{print $1}' | grep -qx 'httproutes'; then
                kubectl -n {{ .Release.Namespace | quote }} delete httproutes.gateway.networking.k8s.io -l "$selector" --ignore-not-found=true
              fi
              if kubectl api-resources --api-group gateway.networking.k8s.io --no-headers 2>/dev/null | awk '{print $1}' | grep -qx 'gateways'; then
                kubectl -n {{ .Release.Namespace | quote }} delete gateways.gateway.networking.k8s.io -l "$selector" --ignore-not-found=true
              fi
              {{- if dig "deletePersistentVolumeClaims" false $cleanup }}
              kubectl -n {{ .Release.Namespace | quote }} delete persistentvolumeclaims -l "$selector" --ignore-not-found=true
              {{- else }}
              echo "Preserving runtime PersistentVolumeClaims. Set runtime.cleanup.onUninstall.deletePersistentVolumeClaims=true to delete them."
              {{- end }}
{{- end }}
"#
    .to_string()
}

fn deployment_tpl() -> String {
    r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.runtime.replicas }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
        {{- with .Values.runtime.podLabels }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- with .Values.runtime.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      automountServiceAccountToken: {{ .Values.runtime.automountServiceAccountToken }}
      securityContext:
        {{- toYaml .Values.runtime.security.podSecurityContext | nindent 8 }}
      {{- with .Values.runtime.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.runtime.scheduling.topologySpreadConstraints }}
      topologySpreadConstraints:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- if .Values.runtime.scheduling.priorityClassName }}
      priorityClassName: {{ .Values.runtime.scheduling.priorityClassName | quote }}
      {{- end }}
      {{- if .Values.runtime.scheduling.runtimeClassName }}
      runtimeClassName: {{ .Values.runtime.scheduling.runtimeClassName | quote }}
      {{- end }}
      containers:
        - name: operator
          image: "{{ .Values.runtime.image.repository }}:{{ .Values.runtime.image.tag }}"
          imagePullPolicy: {{ .Values.runtime.image.pullPolicy }}
          securityContext:
            {{- toYaml .Values.runtime.security.containerSecurityContext | nindent 12 }}
          env:
            - name: PLATFORM
              value: kubernetes
            {{- if .Values.basePlatform }}
            - name: ALIEN_BASE_PLATFORM
              value: {{ .Values.basePlatform | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "aws") .Values.basePlatformConfig.aws.region }}
            - name: AWS_REGION
              value: {{ .Values.basePlatformConfig.aws.region | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.projectId }}
            - name: GCP_PROJECT_ID
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            - name: GOOGLE_CLOUD_PROJECT
              value: {{ .Values.basePlatformConfig.gcp.projectId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "gcp") .Values.basePlatformConfig.gcp.region }}
            - name: GCP_REGION
              value: {{ .Values.basePlatformConfig.gcp.region | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.subscriptionId }}
            - name: AZURE_SUBSCRIPTION_ID
              value: {{ .Values.basePlatformConfig.azure.subscriptionId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.tenantId }}
            - name: AZURE_TENANT_ID
              value: {{ .Values.basePlatformConfig.azure.tenantId | quote }}
            {{- end }}
            {{- if and (eq .Values.basePlatform "azure") .Values.basePlatformConfig.azure.location }}
            - name: AZURE_REGION
              value: {{ .Values.basePlatformConfig.azure.location | quote }}
            {{- end }}
            - name: SYNC_URL
              value: {{ .Values.management.url | quote }}
            - name: OPERATOR_NAME
              value: {{ .Values.management.name | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
            - name: DATA_DIR
              value: {{ .Values.runtime.data.mountPath | quote }}
            - name: SYNC_TOKEN_FILE
              value: /etc/deployment/secrets/sync-token
            - name: OPERATOR_ENCRYPTION_KEY_FILE
              value: /etc/deployment/secrets/encryption-key
            - name: STACK_SETTINGS_FILE
              value: /etc/deployment/config/stack-settings.json
            - name: PUBLIC_URLS_FILE
              value: /etc/deployment/config/public-urls.json
            {{- if .Values.infrastructure }}
            - name: EXTERNAL_BINDINGS_FILE
              value: /etc/deployment/secrets/external-bindings.json
            {{- end }}
            - name: SYNC_INTERVAL
              value: "30"
            - name: OTLP_PORT
              value: {{ .Values.runtime.api.port | quote }}
            - name: OTLP_HOST
              value: {{ .Values.runtime.api.bindHost | quote }}
          ports:
            - name: otlp
              containerPort: {{ .Values.runtime.api.port }}
          {{- if .Values.runtime.probes.liveness.enabled }}
          livenessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.liveness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.liveness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.liveness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.liveness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.liveness.failureThreshold }}
          {{- end }}
          {{- if .Values.runtime.probes.readiness.enabled }}
          readinessProbe:
            httpGet:
              path: {{ .Values.runtime.probes.readiness.path | quote }}
              port: otlp
            initialDelaySeconds: {{ .Values.runtime.probes.readiness.initialDelaySeconds }}
            periodSeconds: {{ .Values.runtime.probes.readiness.periodSeconds }}
            timeoutSeconds: {{ .Values.runtime.probes.readiness.timeoutSeconds }}
            failureThreshold: {{ .Values.runtime.probes.readiness.failureThreshold }}
          {{- end }}
          volumeMounts:
            - name: config
              mountPath: /etc/deployment/config
              readOnly: true
            - name: management-token
              mountPath: /etc/deployment/secrets/sync-token
              subPath: sync-token
              readOnly: true
            - name: encryption-key
              mountPath: /etc/deployment/secrets/encryption-key
              subPath: {{ include "deployment.encryptionSecretKey" . }}
              readOnly: true
            {{- if .Values.infrastructure }}
            - name: external-bindings
              mountPath: /etc/deployment/secrets/external-bindings.json
              subPath: external-bindings.json
              readOnly: true
            {{- end }}
            {{- if .Values.runtime.tmp.enabled }}
            - name: tmp
              mountPath: /tmp
            {{- end }}
            - name: runtime-data
              mountPath: {{ .Values.runtime.data.mountPath | quote }}
          resources:
            {{- toYaml .Values.runtime.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "deployment.fullname" . }}
        - name: management-token
          secret:
            secretName: {{ include "deployment.managementSecretName" . }}
            items:
              - key: {{ include "deployment.managementSecretTokenKey" . }}
                path: sync-token
            defaultMode: 384
        - name: encryption-key
          secret:
            secretName: {{ include "deployment.encryptionSecretName" . }}
            defaultMode: 384
        {{- if .Values.infrastructure }}
        - name: external-bindings
          secret:
            secretName: {{ include "deployment.fullname" . }}
            items:
              - key: external-bindings.json
                path: external-bindings.json
            defaultMode: 384
        {{- end }}
        {{- if .Values.runtime.tmp.enabled }}
        - name: tmp
          emptyDir:
            sizeLimit: {{ .Values.runtime.tmp.sizeLimit | quote }}
        {{- end }}
        - name: runtime-data
          {{- if .Values.runtime.data.persistence.enabled }}
          persistentVolumeClaim:
            claimName: {{ default (printf "%s-runtime-data" (include "deployment.fullname" .)) .Values.runtime.data.persistence.existingClaim }}
          {{- else }}
          emptyDir: {}
          {{- end }}
"#
    .to_string()
}

fn service_tpl() -> String {
    r#"{{- if .Values.runtime.api.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  type: {{ .Values.runtime.api.service.type }}
  selector:
    app.kubernetes.io/name: {{ include "deployment.name" . }}
    app.kubernetes.io/instance: {{ .Release.Name }}
  ports:
    - name: http
      port: {{ .Values.runtime.api.port }}
      targetPort: otlp
{{- end }}
"#
    .to_string()
}

fn pvc_tpl() -> String {
    r#"{{- if and .Values.runtime.data.persistence.enabled (not .Values.runtime.data.persistence.existingClaim) }}
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: {{ printf "%s-runtime-data" (include "deployment.fullname" .) }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  accessModes:
    {{- toYaml .Values.runtime.data.persistence.accessModes | nindent 4 }}
  {{- if .Values.runtime.data.persistence.storageClassName }}
  storageClassName: {{ .Values.runtime.data.persistence.storageClassName | quote }}
  {{- end }}
  resources:
    requests:
      storage: {{ .Values.runtime.data.persistence.size | quote }}
{{- end }}
"#
    .to_string()
}

fn poddisruptionbudget_tpl() -> String {
    r#"{{- if .Values.runtime.pdb.enabled }}
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  {{- if hasKey .Values.runtime.pdb "maxUnavailable" }}
  maxUnavailable: {{ .Values.runtime.pdb.maxUnavailable }}
  {{- else }}
  minAvailable: {{ .Values.runtime.pdb.minAvailable }}
  {{- end }}
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}
"#
    .to_string()
}

fn networkpolicy_tpl() -> String {
    r#"{{- if .Values.runtime.networkPolicy.enabled }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
  policyTypes:
    {{- if .Values.runtime.networkPolicy.ingress.enabled }}
    - Ingress
    {{- end }}
    {{- if .Values.runtime.networkPolicy.egress.enabled }}
    - Egress
    {{- end }}
  {{- if .Values.runtime.networkPolicy.ingress.enabled }}
  ingress:
    - {}
  {{- end }}
  {{- if .Values.runtime.networkPolicy.egress.enabled }}
  egress:
    - {}
  {{- end }}
{{- end }}
"#
    .to_string()
}

fn app_service_tpl() -> String {
    r#"{{- range $id, $service := .Values.services }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "deployment.resourceName" (dict "root" $ "name" $id) }}
  labels:
    {{- include "deployment.labels" $ | nindent 4 }}
    resource-id: {{ $id | quote }}
spec:
  type: {{ if eq $service.type "loadBalancer" }}LoadBalancer{{ else }}ClusterIP{{ end }}
  selector:
    app: {{ include "deployment.resourceName" (dict "root" $ "name" $id) }}
    managed-by: deployment
    component: {{ $service.component | quote }}
  ports:
    - name: http
      port: {{ default 80 $service.port }}
      targetPort: {{ default 8080 $service.targetPort }}
---
{{- end }}
"#
    .to_string()
}

fn cluster_bootstrap_tpl() -> String {
    r#"{{- $bootstrap := default dict .Values.clusterBootstrap -}}
{{- $storage := dig "storageClass" "default" dict $bootstrap -}}
{{- if dig "enabled" false $storage }}
{{- $storageName := required "clusterBootstrap.storageClass.default.name is required when enabled" $storage.name -}}
{{- $provisioner := required "clusterBootstrap.storageClass.default.provisioner is required when enabled" $storage.provisioner -}}
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: {{ $storageName | quote }}
  annotations:
    storageclass.kubernetes.io/is-default-class: "true"
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
provisioner: {{ $provisioner | quote }}
{{ with $storage.parameters }}
parameters:
  {{ range $key, $value := . }}
  {{ $key }}: {{ $value | quote }}
  {{ end }}
{{ end }}
reclaimPolicy: Delete
volumeBindingMode: WaitForFirstConsumer
allowVolumeExpansion: true
{{- if eq $provisioner "ebs.csi.eks.amazonaws.com" }}
allowedTopologies:
  - matchLabelExpressions:
      - key: eks.amazonaws.com/compute-type
        values:
          - auto
{{ end }}
{{ end }}
{{- $eksAlb := dig "ingress" "eksAutoMode" dict $bootstrap -}}
{{- if dig "enabled" false $eksAlb }}
{{- $ingressClassName := required "clusterBootstrap.ingress.eksAutoMode.name is required when enabled" $eksAlb.name -}}
{{- $controller := required "clusterBootstrap.ingress.eksAutoMode.controller is required when enabled" $eksAlb.controller -}}
---
apiVersion: eks.amazonaws.com/v1
kind: IngressClassParams
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  scheme: {{ default "internet-facing" $eksAlb.scheme | quote }}
  {{ with $eksAlb.subnetIds }}
  subnets:
    ids:
      {{ range . }}
      - {{ . | quote }}
      {{ end }}
  {{ end }}
---
apiVersion: networking.k8s.io/v1
kind: IngressClass
metadata:
  name: {{ $ingressClassName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  controller: {{ $controller | quote }}
  parameters:
    apiGroup: eks.amazonaws.com
    kind: IngressClassParams
    name: {{ $ingressClassName | quote }}
{{ end }}
{{- $azureAgc := dig "ingress" "azureApplicationGatewayForContainers" dict $bootstrap -}}
{{- if dig "enabled" false $azureAgc }}
{{- $azureAlb := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer is required when enabled" $azureAgc.applicationLoadBalancer -}}
{{- $azureAlbName := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.name is required when enabled" $azureAlb.name -}}
{{- $azureAlbNamespace := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.namespace is required when enabled" $azureAlb.namespace -}}
{{- $azureAssociationSubnetId := required "clusterBootstrap.ingress.azureApplicationGatewayForContainers.applicationLoadBalancer.associationSubnetId is required when enabled" $azureAlb.associationSubnetId -}}
---
apiVersion: alb.networking.azure.io/v1
kind: ApplicationLoadBalancer
metadata:
  name: {{ $azureAlbName | quote }}
  namespace: {{ $azureAlbNamespace | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  associations:
    - {{ $azureAssociationSubnetId | quote }}
{{ end }}
{{- $eksArm64NodePool := dig "compute" "eksAutoMode" "arm64NodePool" dict $bootstrap -}}
{{- if dig "enabled" false $eksArm64NodePool }}
{{- $nodePoolName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.name is required when enabled" $eksArm64NodePool.name -}}
{{- $nodeClassName := required "clusterBootstrap.compute.eksAutoMode.arm64NodePool.nodeClassName is required when enabled" $eksArm64NodePool.nodeClassName -}}
---
apiVersion: karpenter.sh/v1
kind: NodePool
metadata:
  name: {{ $nodePoolName | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  template:
    spec:
      nodeClassRef:
        group: eks.amazonaws.com
        kind: NodeClass
        name: {{ $nodeClassName | quote }}
      requirements:
        - key: karpenter.sh/capacity-type
          operator: In
          values:
            - {{ default "on-demand" $eksArm64NodePool.capacityType | quote }}
        - key: kubernetes.io/arch
          operator: In
          values:
            - "arm64"
        - key: eks.amazonaws.com/instance-category
          operator: In
          values:
            {{ range (default (list "c" "m" "r") $eksArm64NodePool.instanceCategories) }}
            - {{ . | quote }}
            {{ end }}
        - key: eks.amazonaws.com/instance-generation
          operator: Gt
          values:
            - {{ default "5" $eksArm64NodePool.minInstanceGeneration | quote }}
  {{ with $eksArm64NodePool.limits }}
  limits:
    {{ with .cpu }}
    cpu: {{ . | quote }}
    {{ end }}
    {{ with .memory }}
    memory: {{ . | quote }}
    {{ end }}
  {{ end }}
{{ end }}
{{- $metrics := dig "metricsServer" dict $bootstrap -}}
{{- if dig "enabled" false $metrics }}
---
apiVersion: v1
kind: ServiceAccount
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:aggregated-metrics-reader
  labels:
    k8s-app: metrics-server
    rbac.authorization.k8s.io/aggregate-to-admin: "true"
    rbac.authorization.k8s.io/aggregate-to-edit: "true"
    rbac.authorization.k8s.io/aggregate-to-view: "true"
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: ["metrics.k8s.io"]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["nodes/metrics"]
    verbs: ["get"]
  - apiGroups: [""]
    resources: ["pods", "nodes"]
    verbs: ["get", "list", "watch"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: metrics-server-auth-reader
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: extension-apiserver-authentication-reader
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: metrics-server:system:auth-delegator
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:auth-delegator
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: system:metrics-server
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:metrics-server
subjects:
  - kind: ServiceAccount
    name: metrics-server
    namespace: kube-system
---
apiVersion: v1
kind: Service
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  selector:
    k8s-app: metrics-server
  ports:
    - name: https
      port: 443
      protocol: TCP
      targetPort: https
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: metrics-server
  namespace: kube-system
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  selector:
    matchLabels:
      k8s-app: metrics-server
  template:
    metadata:
      labels:
        k8s-app: metrics-server
    spec:
      serviceAccountName: metrics-server
      containers:
        - name: metrics-server
          image: {{ default "registry.k8s.io/metrics-server/metrics-server:v0.8.1" $metrics.image | quote }}
          imagePullPolicy: IfNotPresent
          args:
            - --cert-dir=/tmp
            - --secure-port=10250
            - --kubelet-preferred-address-types=InternalIP,ExternalIP,Hostname
            - --kubelet-use-node-status-port
            - --metric-resolution=15s
          ports:
            - name: https
              containerPort: 10250
              protocol: TCP
          livenessProbe:
            httpGet:
              path: /livez
              port: https
              scheme: HTTPS
            initialDelaySeconds: 10
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /readyz
              port: https
              scheme: HTTPS
            initialDelaySeconds: 20
            periodSeconds: 10
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            runAsNonRoot: true
            runAsUser: 1000
          volumeMounts:
            - name: tmp
              mountPath: /tmp
      volumes:
        - name: tmp
          emptyDir: {}
---
apiVersion: apiregistration.k8s.io/v1
kind: APIService
metadata:
  name: v1beta1.metrics.k8s.io
  labels:
    k8s-app: metrics-server
    {{- include "deployment.labels" . | nindent 4 }}
spec:
  service:
    name: metrics-server
    namespace: kube-system
  group: metrics.k8s.io
  version: v1beta1
  insecureSkipTLSVerify: true
  groupPriorityMinimum: 100
  versionPriority: 100
{{- end }}
"#
    .to_string()
}

fn eks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "eks.amazonaws.com/role-arn",
        "arn:aws:iam::123456789012:role",
    )
}

fn gke_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "iam.gke.io/gcp-service-account",
        "deployment@project.iam.gserviceaccount.com",
    )
}

fn aks_values_example(analysis: &ChartAnalysis) -> String {
    cloud_identity_example(
        analysis,
        "azure.workload.identity/client-id",
        "00000000-0000-0000-0000-000000000000",
    )
}

fn onprem_values_example(analysis: &ChartAnalysis) -> String {
    let mut yaml = external_bindings_initialize_example_values();
    append_service_accounts(&mut yaml, analysis);
    yaml.push('\n');
    append_infrastructure(&mut yaml, analysis);
    append_services(&mut yaml, analysis);
    yaml
}

fn cloud_identity_example(
    analysis: &ChartAnalysis,
    annotation: &str,
    identity_prefix: &str,
) -> String {
    let mut yaml = registered_setup_example_values();
    yaml.push_str("serviceAccounts:\n");
    if analysis.service_accounts.is_empty() {
        yaml.push_str("  {}\n");
    } else {
        for name in &analysis.service_accounts {
            let value = if annotation == "eks.amazonaws.com/role-arn" {
                format!("{identity_prefix}/{}-{name}", "deployment")
            } else if annotation == "iam.gke.io/gcp-service-account" {
                identity_prefix.to_string()
            } else {
                identity_prefix.to_string()
            };
            yaml.push_str(&format!(
                "  {}:\n    annotations:\n      {}: {}\n    labels: {{}}\n",
                yaml_key(name),
                yaml_key(annotation),
                yaml_string(&value)
            ));
        }
    }
    append_services(&mut yaml, analysis);
    yaml
}

fn registered_setup_example_values() -> String {
    r#"management:
  token: "dg_replace_me"
  name: "production"
  url: "https://management.example.com"
  deploymentId: "dep_replace_me"
  updates: auto
  telemetry: auto
  healthChecks: "on"

"#
    .to_string()
}

fn external_bindings_initialize_example_values() -> String {
    r#"management:
  token: "dg_replace_me"
  name: "production"
  url: "https://management.example.com"
  deploymentId: null
  updates: auto
  telemetry: auto
  healthChecks: "on"

stackSettings:
  deploymentModel: pull
  network: null
  domains: null
  updates: auto
  telemetry: auto
  heartbeats: "on"

"#
    .to_string()
}

fn readme_md(chart_name: &str, stack: &Stack) -> String {
    format!(
        "# {chart_name}\n\nInstall this chart into an existing Kubernetes cluster:\n\n```bash\nhelm install {chart_name} ./{} --namespace production --create-namespace --values values.yaml\n```\n\nThe generated `values.yaml` contains placeholders for management, service-account identity annotations, operator-local infrastructure bindings, and the Kubernetes exposure profile. The chart no longer renders per-app public `Ingress` objects from `services.*.host` or hostless ingress values; public endpoints are runtime-owned through `stackSettings.kubernetes.exposure`.\n\nSee `examples/<target>.yaml` for ready-to-use values matching EKS / GKE / AKS / on-prem.\n",
        stack.id()
    )
}

fn sanitize_chart_name(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else if !last_dash {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        out.push(next);
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "alien-deployment".to_string()
    } else {
        out.chars()
            .take(63)
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    }
}

fn yaml_key(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        value.to_string()
    } else {
        yaml_string(value)
    }
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        import::data::AzureApplicationGatewayForContainersBootstrap, KubernetesCluster,
        KubernetesClusterOutputs, KubernetesClusterOwnership, KubernetesClusterProvider,
        PermissionProfile, Queue, RemoteStackManagement, Resource, ResourceLifecycle,
        ResourceOutputs, ResourceStatus, StackResourceState, Storage, WorkerCode, WorkerTrigger,
    };
    use serde::Deserialize;
    use serde_yaml::Value as YamlValue;

    fn operator_test_manifest() -> String {
        generate_operator_manifest(OperatorManifestOptions {
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            namespace: "demo",
            scope: OperatorScope::single_namespace("demo"),
            permission: OperatorPermission::Observe,
            release_name: "acme-prod-eu",
        })
        .expect("operator manifest should render")
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
        assert!(env_names.contains(&"SYNC_TOKEN_FILE"));
        assert!(
            !env_names.contains(&"DEPLOYMENT_ID"),
            "first boot must self-register and then persist deployment identity"
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
            Some("acme-prod-eu-operator-identity")
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
            .ingress(Ingress::Public)
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
        assert!(values
            .contains("storageClass:\n    default:\n      enabled: true\n      name: \"gp3\""));
        assert!(values.contains("ingress:\n    eksAutoMode:\n      enabled: true\n      name: alb"));
        assert!(values
            .contains("compute:\n    eksAutoMode:\n      arm64NodePool:\n        enabled: true"));
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

        assert!(values.contains(
            "'azure.workload.identity/client-id': '11111111-2222-3333-4444-555555555555'"
        ));
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
                    agent_ready: false,
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
}

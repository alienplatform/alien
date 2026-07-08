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
    ErrorData, KubernetesCluster, KubernetesClusterOutputs, KubernetesClusterOwnership,
    KubernetesClusterProvider, Platform, RemoteStackManagementOutputs, ResourceLifecycle, Result,
    ServiceAccount, ServiceAccountOutputs, Stack, StackSettings, Worker, WorkerCode,
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

/// How the rendered operator documents are meant to be consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorOutputFormat {
    /// Flat multi-document manifest for `kubectl apply` to a single cluster.
    /// Namespace and environment name are concrete literals.
    RawManifest,
    /// Helm-templated documents to paste into an existing chart's `templates/`.
    /// Namespace resolves to `.Release.Namespace` and the per-environment name
    /// to `.Values.alien.environmentName`, so one file serves every install.
    HelmTemplate,
}

/// How much of the cluster the operator manages. This is the single decision
/// that flips a namespaced `Role` to a cluster-wide `ClusterRole` and widens
/// what the operator observes from one namespace to all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorScope {
    /// Manage only the namespace the operator is installed in. Grants a
    /// namespaced `Role`/`RoleBinding`.
    Namespace,
    /// Manage the **whole cluster** (spans namespaces). Grants a
    /// `ClusterRole`/`ClusterRoleBinding`.
    Cluster,
}

impl OperatorScope {
    /// Whether this scope requires cluster-wide (cluster-scoped) RBAC.
    fn is_cluster_wide(self) -> bool {
        matches!(self, OperatorScope::Cluster)
    }
}

pub struct OperatorManifestOptions<'a> {
    pub manager_url: &'a str,
    pub group_token: &'a str,
    pub encryption_key: &'a str,
    pub image: &'a str,
    pub log_collector: Option<OperatorLogCollectorOptions<'a>>,
    /// Names the Kubernetes objects and labels. Stable per app/project — the
    /// same across every customer install. (Formerly `release_name`.)
    pub project_name: &'a str,
    /// The per-environment identity reported as `OPERATOR_NAME`. Required for
    /// `RawManifest`; ignored for `HelmTemplate`, which sources it from
    /// `.Values.alien.environmentName` so each install is distinct.
    pub environment_name: Option<&'a str>,
    /// The namespace the operator installs into. Required (non-empty) for
    /// `RawManifest`; ignored for `HelmTemplate`, which uses `.Release.Namespace`.
    /// In `Namespace` scope this is also the namespace observed.
    pub install_namespace: Option<&'a str>,
    pub scope: OperatorScope,
    /// Optional Kubernetes label selector that narrows what the operator manages,
    /// applied on top of `scope`. Independent of namespace vs cluster scope: a
    /// cluster-scoped operator can still filter to labeled resources, and a
    /// namespaced one can filter within its namespace. `None` manages everything
    /// in scope.
    pub label_selector: Option<&'a str>,
    pub permission: OperatorPermission,
    pub format: OperatorOutputFormat,
}

pub struct OperatorLogCollectorOptions<'a> {
    pub image: &'a str,
    pub token: &'a str,
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
    files.insert(
        "templates/whitelabeled-log-collector-serviceaccount.yaml".to_string(),
        whitelabeled_log_collector_serviceaccount_tpl(),
    );
    files.insert(
        "templates/whitelabeled-log-collector-role.yaml".to_string(),
        whitelabeled_log_collector_role_tpl(),
    );
    files.insert(
        "templates/whitelabeled-log-collector-rolebinding.yaml".to_string(),
        whitelabeled_log_collector_rolebinding_tpl(),
    );
    files.insert(
        "templates/whitelabeled-log-collector-configmap.yaml".to_string(),
        whitelabeled_log_collector_configmap_tpl(),
    );
    files.insert(
        "templates/whitelabeled-log-collector-daemonset.yaml".to_string(),
        whitelabeled_log_collector_daemonset_tpl(),
    );
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
    validate_operator_options(&options)?;

    let base_name = sanitize_chart_name(options.project_name);
    let operator_name = format!("{base_name}-operator");
    let identity_pvc_name = format!("{operator_name}-identity");

    // The install namespace: where every operator object lives, the binding
    // subject's namespace, and (in `Namespace` scope) the observed namespace. A
    // Helm template defers it to `.Release.Namespace`; a raw manifest pins it to
    // a concrete value so `kubectl apply` and binding subjects resolve.
    let namespace_expr = match options.format {
        OperatorOutputFormat::HelmTemplate => "{{ .Release.Namespace }}".to_string(),
        OperatorOutputFormat::RawManifest => options
            .install_namespace
            .expect("validated by validate_operator_options")
            .to_string(),
    };
    let namespace = namespace_expr.as_str();

    // OPERATOR_NAME is the per-environment identity. A raw manifest carries the
    // concrete name; a Helm template sources it per install from values so one
    // file registers every customer environment distinctly.
    let environment_name_expr = match options.format {
        OperatorOutputFormat::HelmTemplate => "{{ .Values.alien.environmentName }}".to_string(),
        OperatorOutputFormat::RawManifest => options
            .environment_name
            .expect("validated by validate_operator_options")
            .to_string(),
    };

    let labels = operator_labels(&base_name);
    let cluster_wide = options.scope.is_cluster_wide();

    let mut docs = Vec::new();
    docs.push(operator_service_account_doc(
        namespace,
        &operator_name,
        &labels,
    ));
    // Cluster-wide (label) scope needs cluster-scoped read RBAC; namespace scope
    // stays a namespaced Role. Both grant only get/list/watch.
    if cluster_wide {
        docs.push(operator_clusterrole_doc(&operator_name, &labels));
        docs.push(operator_clusterrolebinding_doc(
            namespace,
            &operator_name,
            &labels,
        ));
    } else {
        docs.push(operator_role_doc(namespace, &operator_name, &labels));
        docs.push(operator_rolebinding_doc(namespace, &operator_name, &labels));
    }
    docs.push(operator_secret_doc(
        namespace,
        &operator_name,
        options.group_token,
        options.encryption_key,
        options
            .log_collector
            .as_ref()
            .map(|collector| collector.token),
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
        namespace,
        &environment_name_expr,
        options.label_selector,
        &labels,
    ));
    if let Some(log_collector) = options.log_collector.as_ref() {
        let mut collector_labels = labels.clone();
        collector_labels.insert(
            "app.kubernetes.io/component".to_string(),
            "whitelabeled-log-collector".to_string(),
        );
        docs.push(operator_service_doc(namespace, &operator_name, &labels));
        docs.push(operator_log_collector_service_account_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_binding_doc(
            namespace,
            &operator_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_configmap_doc(
            namespace,
            &operator_name,
            namespace,
            &collector_labels,
        ));
        docs.push(operator_log_collector_daemonset_doc(
            namespace,
            &operator_name,
            log_collector.image,
            &collector_labels,
        ));
    }

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
    yaml.push_str("logCollector:\n");
    yaml.push_str("  scope:\n");
    yaml.push_str(&format!(
        "    deploymentLabelValue: {}\n",
        yaml_string(&options.stack_state.resource_prefix)
    ));
    yaml.push('\n');

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
    yaml.push_str("\npublicEndpoints: {}\n");

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

fn validate_operator_options(options: &OperatorManifestOptions<'_>) -> Result<()> {
    let invalid = |message: &str| {
        Err(AlienError::new(ErrorData::GenericError {
            message: message.to_string(),
        }))
    };

    // A label selector, when given, must be non-empty.
    if let Some(selector) = options.label_selector {
        if selector.trim().is_empty() {
            return invalid("operator label selector must not be empty");
        }
    }

    // Raw manifests are applied to one concrete cluster, so the install namespace
    // and per-environment identity must be concrete. Helm defers both to install.
    if options.format == OperatorOutputFormat::RawManifest {
        if options
            .install_namespace
            .map(|ns| ns.trim().is_empty())
            .unwrap_or(true)
        {
            return invalid("raw manifests require an install namespace");
        }
        if options
            .environment_name
            .map(|name| name.trim().is_empty())
            .unwrap_or(true)
        {
            return invalid("raw manifests require an environment name");
        }
    }

    Ok(())
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
    yaml.push_str(OPERATOR_OBSERVE_RULES);
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

/// Read-only observe rules, shared by the namespaced `Role` and the cluster-wide
/// `ClusterRole`. Only `get/list/watch`; never `secrets` or `pods/log`.
const OPERATOR_OBSERVE_RULES: &str = r#"rules:
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
"#;

/// Cluster-scoped metadata header (no `metadata.namespace`) for `ClusterRole` and
/// `ClusterRoleBinding`, which are not namespaced objects.
fn operator_cluster_metadata_doc(
    kind: &str,
    name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = String::new();
    yaml.push_str("apiVersion: rbac.authorization.k8s.io/v1\n");
    yaml.push_str(&format!("kind: {}\n", yaml_string(kind)));
    yaml.push_str("metadata:\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(name)));
    yaml.push_str("  labels:\n");
    append_operator_labels(&mut yaml, labels, 4);
    yaml
}

fn operator_clusterrole_doc(operator_name: &str, labels: &BTreeMap<String, String>) -> String {
    let mut yaml = operator_cluster_metadata_doc("ClusterRole", operator_name, labels);
    yaml.push_str(OPERATOR_OBSERVE_RULES);
    yaml
}

fn operator_clusterrolebinding_doc(
    subject_namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_cluster_metadata_doc("ClusterRoleBinding", operator_name, labels);
    yaml.push_str(&format!(
        r#"subjects:
  - kind: ServiceAccount
    name: {}
    namespace: {}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {}
"#,
        yaml_string(operator_name),
        yaml_string(subject_namespace),
        yaml_string(operator_name)
    ));
    yaml
}

fn operator_secret_doc(
    namespace: &str,
    operator_name: &str,
    group_token: &str,
    encryption_key: &str,
    collector_token: Option<&str>,
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
    if let Some(collector_token) = collector_token {
        yaml.push_str(&format!(
            "  collector-token: {}\n",
            yaml_string(collector_token)
        ));
    }
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

#[allow(clippy::too_many_arguments)]
fn operator_deployment_doc(
    namespace: &str,
    operator_name: &str,
    identity_pvc_name: &str,
    options: &OperatorManifestOptions<'_>,
    observed_namespace: &str,
    environment_name: &str,
    label_selector: Option<&str>,
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
    append_env_value(&mut yaml, "OPERATOR_NAME", environment_name);
    append_env_value(&mut yaml, "KUBERNETES_NAMESPACE", namespace);
    append_env_value(&mut yaml, "OPERATOR_SCOPE", observed_namespace);
    // Cluster scope observes every namespace; namespace scope stays in its own.
    // The selector (if any) filters within whichever scope is chosen.
    if options.scope.is_cluster_wide() {
        append_env_value(&mut yaml, "OPERATOR_OBSERVE_ALL_NAMESPACES", "true");
    }
    if let Some(label_selector) = label_selector {
        append_env_value(&mut yaml, "OPERATOR_LABEL_SELECTOR", label_selector);
    }
    // Helm distributions surface the running app version as a value, so each install
    // reports the release it's on. Raw manifests omit it; the vendor sets
    // OPERATOR_RELEASE_VERSION themselves if they want version/rollout visibility.
    if options.format == OperatorOutputFormat::HelmTemplate {
        append_env_value(
            &mut yaml,
            "OPERATOR_RELEASE_VERSION",
            "{{ .Values.alien.version }}",
        );
    }
    append_env_value(
        &mut yaml,
        "OPERATOR_PERMISSION",
        options.permission.as_str(),
    );
    append_env_value(&mut yaml, "OPERATOR_SETUP_METHOD", "manual");
    append_env_value(&mut yaml, "DATA_DIR", "/var/lib/operator");
    if options.log_collector.is_some() {
        append_env_value(&mut yaml, "OTLP_HOST", "0.0.0.0");
        append_env_value(&mut yaml, "OTLP_PORT", "8080");
        append_env_value(
            &mut yaml,
            "COLLECTOR_TOKEN_FILE",
            "/etc/operator/secrets/collector-token",
        );
    }
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
    if options.log_collector.is_some() {
        yaml.push_str("          ports:\n");
        yaml.push_str("            - name: http\n");
        yaml.push_str("              containerPort: 8080\n");
    }
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

fn operator_service_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "Service", namespace, operator_name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  type: ClusterIP\n");
    yaml.push_str("  selector:\n");
    append_operator_selector_labels(&mut yaml, labels, 4);
    yaml.push_str("  ports:\n");
    yaml.push_str("    - name: http\n");
    yaml.push_str("      port: 8080\n");
    yaml.push_str("      targetPort: http\n");
    yaml
}

fn operator_log_collector_service_account_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, &name, labels);
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_log_collector_role_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        &name,
        labels,
    );
    yaml.push_str("rules:\n");
    yaml.push_str("  - apiGroups: [\"\"]\n");
    yaml.push_str("    resources: [\"pods\"]\n");
    yaml.push_str("    verbs: [\"get\", \"list\", \"watch\"]\n");
    yaml
}

fn operator_log_collector_role_binding_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "RoleBinding",
        namespace,
        &name,
        labels,
    );
    yaml.push_str("roleRef:\n");
    yaml.push_str("  apiGroup: rbac.authorization.k8s.io\n");
    yaml.push_str("  kind: Role\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(&name)));
    yaml.push_str("subjects:\n");
    yaml.push_str("  - kind: ServiceAccount\n");
    yaml.push_str(&format!("    name: {}\n", yaml_string(&name)));
    yaml.push_str(&format!("    namespace: {}\n", yaml_string(namespace)));
    yaml
}

fn operator_log_collector_configmap_doc(
    namespace: &str,
    operator_name: &str,
    observed_namespace: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("v1", "ConfigMap", namespace, &name, labels);
    yaml.push_str("data:\n");
    yaml.push_str("  collector.conf: |\n");
    yaml.push_str("    [SERVICE]\n");
    yaml.push_str("        Flush        2\n");
    yaml.push_str("        Log_Level    info\n");
    yaml.push_str("        Parsers_File parsers.conf\n");
    yaml.push_str("        storage.path /buffers\n");
    yaml.push_str("        storage.sync normal\n");
    yaml.push_str("        storage.backlog.mem_limit 64M\n\n");
    yaml.push_str("    [INPUT]\n");
    yaml.push_str("        Name              tail\n");
    yaml.push_str(&format!(
        "        Path              /var/log/pods/{}_*/*/*.log\n",
        observed_namespace
    ));
    yaml.push_str(&format!(
        "        Exclude_Path      /var/log/pods/{}_{}-*/*/*.log\n",
        observed_namespace, operator_name
    ));
    yaml.push_str("        Path_Key          filename\n");
    // Built-in multiline parsers auto-detect the runtime log format: `cri` for
    // containerd (EKS/GKE/AKS, k8s >=1.24) and `docker` for the docker-json format
    // used by Docker-runtime clusters (Docker Desktop, OrbStack, legacy on-prem).
    yaml.push_str("        multiline.parser  docker, cri\n");
    yaml.push_str("        Tag               kube.*\n");
    yaml.push_str(&format!(
        "        DB                /buffers/{operator_name}-whitelabeled-log-collector.db\n"
    ));
    yaml.push_str("        Mem_Buf_Limit     64MB\n");
    yaml.push_str("        Skip_Long_Lines   On\n");
    yaml.push_str("        Read_from_Head    On\n");
    yaml.push_str("        Refresh_Interval  5\n");
    yaml.push_str("        storage.type      filesystem\n\n");
    yaml.push_str("    [FILTER]\n");
    yaml.push_str("        Name                kubernetes\n");
    yaml.push_str("        Match               kube.*\n");
    yaml.push_str("        Merge_Log           Off\n");
    yaml.push_str("        Keep_Log            On\n");
    yaml.push_str("        Labels              On\n");
    yaml.push_str("        Annotations         Off\n\n");
    yaml.push_str("    [OUTPUT]\n");
    yaml.push_str("        Name          http\n");
    // Without a Match the router never routes the tailed kube.* records to this
    // output ("NO match for http.0 output instance"), so no pod logs are shipped.
    yaml.push_str("        Match         kube.*\n");
    yaml.push_str(&format!(
        "        Host          {operator_name}.{namespace}.svc.cluster.local\n"
    ));
    yaml.push_str("        Port          8080\n");
    yaml.push_str("        URI           /internal/logs\n");
    yaml.push_str("        Format        json\n");
    yaml.push_str("        Json_Date_Key observed_at\n");
    yaml.push_str("        Header        Authorization Bearer ${COLLECTOR_TOKEN}\n\n");
    yaml.push_str("  parsers.conf: |\n");
    yaml.push_str("    [PARSER]\n");
    yaml.push_str("        Name        cri\n");
    yaml.push_str("        Format      regex\n");
    yaml.push_str(
        "        Regex       ^(?<time>[^ ]+) (?<stream>stdout|stderr) (?<logtag>[^ ]*) (?<log>.*)$\n",
    );
    yaml.push_str("        Time_Key    time\n");
    yaml.push_str("        Time_Format %Y-%m-%dT%H:%M:%S.%L%z\n");
    yaml
}

fn operator_log_collector_daemonset_doc(
    namespace: &str,
    operator_name: &str,
    image: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let name = format!("{operator_name}-whitelabeled-log-collector");
    let mut yaml = operator_metadata_doc("apps/v1", "DaemonSet", namespace, &name, labels);
    yaml.push_str("spec:\n");
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
        yaml_string(&name)
    ));
    yaml.push_str("      tolerations:\n");
    yaml.push_str("        - operator: Exists\n");
    yaml.push_str("      containers:\n");
    yaml.push_str("        - name: collector\n");
    yaml.push_str(&format!("          image: {}\n", yaml_string(image)));
    yaml.push_str("          imagePullPolicy: IfNotPresent\n");
    yaml.push_str("          args: [\"-c\", \"/collector/etc/collector.conf\"]\n");
    yaml.push_str("          env:\n");
    yaml.push_str("            - name: COLLECTOR_TOKEN\n");
    yaml.push_str("              valueFrom:\n");
    yaml.push_str("                secretKeyRef:\n");
    yaml.push_str(&format!(
        "                  name: {}\n",
        yaml_string(operator_name)
    ));
    yaml.push_str("                  key: collector-token\n");
    yaml.push_str("          volumeMounts:\n");
    yaml.push_str("            - name: config\n");
    yaml.push_str("              mountPath: /collector/etc\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: varlog\n");
    yaml.push_str("              mountPath: /var/log\n");
    yaml.push_str("              readOnly: true\n");
    // Docker-runtime clusters symlink /var/log/pods/.../*.log to the json log files
    // under /var/lib/docker/containers, so the symlink targets must be mounted too or
    // fluent-bit reads nothing. Harmless on containerd nodes (DirectoryOrCreate).
    yaml.push_str("            - name: dockercontainers\n");
    yaml.push_str("              mountPath: /var/lib/docker/containers\n");
    yaml.push_str("              readOnly: true\n");
    yaml.push_str("            - name: buffers\n");
    yaml.push_str("              mountPath: /buffers\n");
    yaml.push_str("      volumes:\n");
    yaml.push_str("        - name: config\n");
    yaml.push_str("          configMap:\n");
    yaml.push_str(&format!("            name: {}\n", yaml_string(&name)));
    yaml.push_str("        - name: varlog\n");
    yaml.push_str("          hostPath:\n");
    yaml.push_str("            path: /var/log\n");
    yaml.push_str("            type: Directory\n");
    yaml.push_str("        - name: dockercontainers\n");
    yaml.push_str("          hostPath:\n");
    yaml.push_str("            path: /var/lib/docker/containers\n");
    yaml.push_str("            type: DirectoryOrCreate\n");
    yaml.push_str("        - name: buffers\n");
    yaml.push_str("          emptyDir: {}\n");
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
                if !function.public_endpoints.is_empty() {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "worker".to_string(),
                        target_port: 8080,
                    });
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                fail_if_container_source_remains(resource_id, container)?;
                if let Some(endpoint) = container.public_endpoints.first() {
                    analysis.services.push(ServiceValue {
                        id: resource_id.clone(),
                        component: "container".to_string(),
                        target_port: endpoint.port,
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
    # Leave empty to let the chart generate a stable random 64-hex-char key
    # on first install (preserved across upgrades via `lookup`). To pin the
    # key explicitly, set it here (must be 64 hex chars = 256-bit AES). To
    # source it from an external secret store, set `existingSecret.name`.
    key: ""
    existingSecret:
      name: ""
      key: encryption-key
  # Agent self-update inputs the agent passes to the Helm-runner Job it
  # spawns on `operator_target.helm`. Set chartRef + chartVersion to the OCI
  # ref + version used at install time — the agent re-uses them in
  # `helm upgrade --reuse-values`. Leave blank if you don't want to enable
  # in-cluster agent upgrades for this install.
  upgrade:
    chartRef: ""
    chartVersion: ""
    helmRunnerImage: "alpine/helm:3.18.4"
    # Extra flags appended to the `helm upgrade` command the agent's
    # helm-runner Job runs (e.g. `--plain-http` for local-dev OCI
    # registries served over HTTP). Production should leave empty.
    extraArgs: ""
  replicas: 1
  # Helm's --atomic --wait gives up after this many seconds if /readyz
  # hasn't returned 200 — the revision is then rolled back automatically.
  progressDeadlineSeconds: 120
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
    # /livez is process liveness; /readyz turns 200 only after the agent
    # completes at least one /v1/sync round-trip with the manager — the
    # gate Helm's --atomic --wait relies on so a freshly-rolled agent
    # is not considered ready until it has actually reached the manager.
    liveness:
      enabled: true
      path: /livez
      initialDelaySeconds: 10
      periodSeconds: 10
      timeoutSeconds: 2
      failureThreshold: 3
    readiness:
      enabled: true
      path: /readyz
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
      # Enabled by default: the agent's `data_dir` holds its persistent
      # deployment_id + sync-token. Without a PVC, any pod restart (e.g.
      # the rolling restart triggered by self-update / `operator_target.helm`)
      # wipes that state, the new pod re-runs `/v1/initialize`, hits a
      # name-conflict 409, crashloops, and helm `--atomic` rolls back.
      # Operators on clusters without a default StorageClass must either
      # set `storageClassName`, point at an `existingClaim`, or
      # explicitly disable this and accept that self-update will not
      # survive a pod roll.
      enabled: true
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
        repository: alpine/k8s
        tag: "1.32.0"
        pullPolicy: IfNotPresent

logCollector:
  enabled: false
  token: "replace-me-with-a-stable-in-cluster-collector-token"
  image:
    repository: fluent/fluent-bit
    tag: "3.2"
    pullPolicy: IfNotPresent
  resources:
    requests:
      cpu: 50m
      memory: 64Mi
    limits:
      memory: 256Mi
  scope:
    deploymentLabelKey: "alien.dev/deployment"
    deploymentLabelValue: ""

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
    yaml.push_str("\ninfrastructure: null\n\nbasePlatform: null\nbasePlatformConfig:\n  gcp:\n    projectId: \"\"\n    region: \"\"\n  aws:\n    region: \"\"\n  azure:\n    location: \"\"\n    subscriptionId: \"\"\n    tenantId: \"\"\nserviceAccountPrefix: \"\"\nmanagerServiceAccount:\n  annotations: {}\n  labels: {}\n\n# Operator self-update. When the operator receives operator_target.helm on\n# /v1/sync it creates a short-lived Helm-runner Job that runs `helm upgrade\n# --atomic`. The Job runs under the release-derived upgrader SA; keep it\n# optional so charts that don't want self-update can disable it.\nupgrader:\n  enabled: true\n");
    append_services(&mut yaml, analysis);
    yaml.push_str("\npublicEndpoints: {}\n");

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
        "upgrade": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "chartRef": { "type": "string" },
            "chartVersion": { "type": "string" },
            "helmRunnerImage": { "type": "string" },
            "extraArgs": { "type": "string" }
          }
        },
        "replicas": { "type": "integer", "minimum": 1 },
        "progressDeadlineSeconds": { "type": "integer", "minimum": 1 },
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
    "logCollector": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "token": { "type": "string" },
        "image": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "repository": { "type": "string", "minLength": 1 },
            "tag": { "type": "string", "minLength": 1 },
            "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
          }
        },
        "resources": { "type": "object" },
        "scope": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "deploymentLabelKey": { "type": "string" },
            "deploymentLabelValue": { "type": "string" }
          }
        }
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
    "upgrader": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" }
      }
    },
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
    "publicEndpoints": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": { "type": "string" }
      }
    },
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

{{/*
  ServiceAccount used by the Helm-runner Job the agent creates when it
  acts on operator_target.helm. Held as a least-privilege boundary; bound
  to the existing Role so the Job can mutate the Deployment + release
  Secrets.
*/}}
{{- define "deployment.upgraderServiceAccountName" -}}
{{- $prefix := default (include "deployment.fullname" .) .Values.serviceAccountPrefix -}}
{{- $raw := printf "%s-upgrader-sa" $prefix | lower -}}
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

{{- /* Name of the ClusterRole that grants the agent self-update Job permission
       to manage the chart-owned cluster-scoped resources (currently just the
       heartbeat ClusterRole+Binding). Only created when both `upgrader.enabled`
       and the heartbeat node-collection feature are on. */ -}}
{{- define "deployment.upgraderClusterRoleName" -}}
{{- printf "%s-upgrader" (include "deployment.fullname" .) | trunc 63 | trimSuffix "-" -}}
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
{{- if .Values.upgrader.enabled }}
# The upgrader ServiceAccount (release-derived, see
# `deployment.upgraderServiceAccountName`) is used by the Helm-runner Job the
# operator creates when it acts on operator_target.helm. It exists as a
# least-privilege boundary for the Job — the operator pod itself uses its own
# manager SA and only needs to create Jobs + stage ConfigMaps/Secrets. The
# protection against bad helm upgrades is the chart's `required` values, not
# RBAC.
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.upgraderServiceAccountName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
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
  # ServiceAccounts + RBAC objects need to live here for `helm upgrade
  # --reuse-values` to inspect (and patch) the release's existing
  # resources during agent self-update. Without this, the upgrader SA
  # — which is bound to this Role — can't `get` the SAs the chart
  # already created and helm 4xx's out.
  - apiGroups: [""]
    resources: ["serviceaccounts"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: ["roles", "rolebindings"]
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
  {{- /* Execution ServiceAccounts (workers/daemons/containers) need RBAC to
         read their own vault secrets (e.g. the injected ALIEN_COMMANDS_TOKEN).
         Without this binding, the worker pod gets 403 reading any secret
         provisioned by the KubernetesVaultController. */}}
  {{- range $name, $account := .Values.serviceAccounts }}
  - kind: ServiceAccount
    name: {{ include "deployment.serviceAccountName" (dict "root" $ "name" $name) }}
  {{- end }}
  {{- if .Values.upgrader.enabled }}
  - kind: ServiceAccount
    name: {{ include "deployment.upgraderServiceAccountName" . }}
  {{- end }}
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
{{- if .Values.upgrader.enabled }}
---
# Narrow cluster-scoped RBAC for the agent self-update helm-runner Job.
# The chart creates exactly one cluster-scoped resource type pair —
# the heartbeat ClusterRole + ClusterRoleBinding above — and the
# upgrader SA needs to be able to `get/update/patch/delete` them
# during `helm upgrade --reuse-values`. `resourceNames` scopes this
# to ONLY the chart's own cluster objects; no enumeration of other
# tenants' cluster resources. Verbs are deliberately minimal (no
# `list`/`watch`, no `create` — chart install is what creates them
# the first time, run by the customer's helm operator). If a future
# chart version introduces new cluster-scoped resources, add their
# names to `resourceNames` (and a `create` verb on a separate rule
# without `resourceNames` if the upgrader needs to add new ones).
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: {{ include "deployment.upgraderClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: ["rbac.authorization.k8s.io"]
    resources: ["clusterroles", "clusterrolebindings"]
    resourceNames:
      # The heartbeat-nodes cluster pair — the existing reason the
      # upgrader needs cluster-scope access at all.
      - {{ include "deployment.heartbeatNodeClusterRoleName" . | quote }}
      # And the upgrader cluster pair itself — helm now tracks these as
      # chart-owned, so every `helm upgrade --reuse-values` does a `get`
      # on them to compute the diff. Without self-reference, the first
      # upgrade trips on `clusterroles "<name>-upgrader" is forbidden`.
      - {{ include "deployment.upgraderClusterRoleName" . | quote }}
    verbs: ["get", "update", "patch", "delete"]
{{- end }}
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
{{- if .Values.upgrader.enabled }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: {{ include "deployment.upgraderClusterRoleName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.upgraderServiceAccountName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: {{ include "deployment.upgraderClusterRoleName" . }}
{{- end }}
{{- end }}
"#
    .to_string()
}

fn secret_tpl() -> String {
    // Encryption key resolution order:
    //   1. user-provided `runtime.encryption.key`
    //   2. existing in-cluster Secret's encryption-key (preserves the key
    //      across `helm upgrade` so previously-encrypted data stays readable)
    //   3. freshly generated via `randBytes 32` — crypto/rand-backed in
    //      sprig 3.2+; if your Helm bundles an older sprig, set the key
    //      explicitly via `runtime.encryption.key` or
    //      `runtime.encryption.existingSecret.name`.
    //
    // `lookup` returns nil during `helm template` (no cluster access), so
    // a `helm template | kubectl apply -f -` workflow would generate a
    // fresh key on each render — install via `helm install/upgrade` to
    // keep the key stable, or always set `runtime.encryption.key`.
    r#"{{- $createManagementSecret := not .Values.management.existingSecret.name -}}
{{- $createEncryptionSecret := not .Values.runtime.encryption.existingSecret.name -}}
{{- $encryptionKey := "" -}}
{{- if $createEncryptionSecret -}}
  {{- $providedKey := .Values.runtime.encryption.key | default "" | trim -}}
  {{- $existingKey := "" -}}
  {{- $existingSecret := lookup "v1" "Secret" .Release.Namespace (include "deployment.fullname" .) -}}
  {{- if and $existingSecret $existingSecret.data -}}
    {{- with index $existingSecret.data "encryption-key" -}}
      {{- $existingKey = b64dec . -}}
    {{- end -}}
  {{- end -}}
  {{- if $providedKey -}}
    {{- $encryptionKey = $providedKey -}}
  {{- else if $existingKey -}}
    {{- $encryptionKey = $existingKey -}}
  {{- else -}}
    {{- /* sprig randBytes returns base64; b64dec to raw bytes then hex */ -}}
    {{- $encryptionKey = printf "%x" (b64dec (randBytes 32)) -}}
  {{- end -}}
{{- end -}}
{{- if or $createManagementSecret $createEncryptionSecret .Values.infrastructure .Values.logCollector.enabled }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
type: Opaque
stringData:
  {{- if $createManagementSecret }}
  sync-token: {{ required "management.token or management.existingSecret.name is required — pass the full values document" .Values.management.token | quote }}
  {{- end }}
  {{- if $createEncryptionSecret }}
  encryption-key: {{ $encryptionKey | quote }}
  {{- end }}
  {{- if .Values.infrastructure }}
  external-bindings.json: {{ toJson .Values.infrastructure | quote }}
  {{- end }}
  {{- if .Values.logCollector.enabled }}
  collector-token: {{ required "logCollector.token is required when logCollector.enabled=true" .Values.logCollector.token | quote }}
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
  public-endpoints.json: {{ toJson (default dict .Values.publicEndpoints) | quote }}
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
          image: "{{ dig "image" "repository" "alpine/k8s" $cleanup }}:{{ dig "image" "tag" "1.32.0" $cleanup }}"
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
  # Recreate guarantees exactly one agent runs at any time, so the
  # InstanceLock is never contended — even during a self-update.
  strategy:
    type: Recreate
  progressDeadlineSeconds: {{ .Values.runtime.progressDeadlineSeconds | default 120 }}
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
            - name: OPERATOR_BASE_PLATFORM
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
            # `required` chart guardrail: any helm upgrade that does not
            # carry the full values document fails to render (Helm aborts
            # before touching the release). This is the protection against
            # bare `helm upgrade` silently resetting the agent's manager
            # config — manager-triggered, operator-triggered, or otherwise.
            - name: SYNC_URL
              value: {{ required "management.url is required — pass the full values document" .Values.management.url | quote }}
            - name: OPERATOR_NAME
              value: {{ required "management.name is required — pass the full values document" .Values.management.name | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
            - name: KUBERNETES_HELM_RELEASE
              value: {{ .Release.Name | quote }}
            - name: ALIEN_OPERATOR_UPGRADER_SA
              value: {{ include "deployment.upgraderServiceAccountName" . | quote }}
            # Reported back on /v1/sync so the dashboard can surface the
            # registry an admin will pull a new tag from when pinning a
            # target operator version.
            - name: ALIEN_OPERATOR_IMAGE_REPOSITORY
              value: {{ .Values.runtime.image.repository | quote }}
            {{- if .Values.runtime.upgrade.chartRef }}
            # Used by the operator when it receives `operator_target.helm` and
            # spawns a Helm-runner Job to apply the new version. The Job
            # runs `helm upgrade --reuse-values` against this chart so only
            # the manager-supplied `values` override (e.g. image.tag) flips.
            - name: ALIEN_OPERATOR_CHART_REF
              value: {{ .Values.runtime.upgrade.chartRef | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.chartVersion }}
            - name: ALIEN_OPERATOR_CHART_VERSION
              value: {{ .Values.runtime.upgrade.chartVersion | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.helmRunnerImage }}
            - name: ALIEN_OPERATOR_HELM_RUNNER_IMAGE
              value: {{ .Values.runtime.upgrade.helmRunnerImage | quote }}
            {{- end }}
            {{- if .Values.runtime.upgrade.extraArgs }}
            # Extra flags spliced into the `helm upgrade` command the
            # operator's helm-runner Job runs. Use sparingly — exists for
            # local-dev/insecure OCI registries (`--plain-http`) and
            # similar one-off escape hatches; production should leave empty.
            - name: ALIEN_OPERATOR_HELM_EXTRA_ARGS
              value: {{ .Values.runtime.upgrade.extraArgs | quote }}
            {{- end }}
            {{- if .Values.serviceAccountPrefix }}
            # Pin the deployment's resource_prefix to the same value used for
            # ServiceAccount naming, so Helm-created SAs and vault secret names
            # stay aligned across operator restarts (pull-model storage is ephemeral
            # and the operator would otherwise regenerate a random prefix each time).
            - name: ALIEN_RESOURCE_PREFIX
              value: {{ .Values.serviceAccountPrefix | quote }}
            {{- end }}
            - name: OPERATOR_SETUP_METHOD
              value: "helm"
            - name: DATA_DIR
              value: {{ .Values.runtime.data.mountPath | quote }}
            - name: SYNC_TOKEN_FILE
              value: /etc/deployment/secrets/sync-token
            - name: OPERATOR_ENCRYPTION_KEY_FILE
              value: /etc/deployment/secrets/encryption-key
            - name: STACK_SETTINGS_FILE
              value: /etc/deployment/config/stack-settings.json
            - name: PUBLIC_ENDPOINTS_FILE
              value: /etc/deployment/config/public-endpoints.json
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
            {{- if .Values.logCollector.enabled }}
            - name: COLLECTOR_TOKEN_FILE
              value: /etc/deployment/secrets/collector-token
            {{- end }}
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
            {{- if .Values.logCollector.enabled }}
            - name: collector-token
              mountPath: /etc/deployment/secrets/collector-token
              subPath: collector-token
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
        {{- if .Values.logCollector.enabled }}
        - name: collector-token
          secret:
            secretName: {{ include "deployment.fullname" . }}
            items:
              - key: collector-token
                path: collector-token
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
    r#"{{- if or .Values.runtime.api.enabled .Values.logCollector.enabled }}
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

fn whitelabeled_log_collector_serviceaccount_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
automountServiceAccountToken: true
{{- end }}
"#
    .to_string()
}

fn whitelabeled_log_collector_role_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
{{- end }}
"#
    .to_string()
}

fn whitelabeled_log_collector_rolebinding_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
    namespace: {{ .Release.Namespace }}
{{- end }}
"#
    .to_string()
}

fn whitelabeled_log_collector_configmap_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
data:
  collector.conf: |
    [SERVICE]
        Flush        2
        Log_Level    info
        Parsers_File parsers.conf
        storage.path /buffers
        storage.sync normal
        storage.backlog.mem_limit 64M

    [INPUT]
        Name              tail
        Path              /var/log/pods/{{ .Release.Namespace }}_*/*/*.log
        Exclude_Path      /var/log/pods/{{ .Release.Namespace }}_{{ include "deployment.fullname" . }}-*/*/*.log
        Path_Key          filename
        multiline.parser  docker, cri
        Tag               kube.*
        DB                /buffers/{{ include "deployment.fullname" . }}-whitelabeled-log-collector.db
        Mem_Buf_Limit     64MB
        Skip_Long_Lines   On
        Read_from_Head    On
        Refresh_Interval  5
        storage.type      filesystem

    [FILTER]
        Name                kubernetes
        Match               kube.*
        Merge_Log           Off
        Keep_Log            On
        Labels              On
        Annotations         Off

    {{- if and .Values.logCollector.scope.deploymentLabelKey .Values.logCollector.scope.deploymentLabelValue }}
    [FILTER]
        Name                grep
        Match               kube.*
        Regex               $kubernetes['labels']['{{ .Values.logCollector.scope.deploymentLabelKey }}'] ^{{ .Values.logCollector.scope.deploymentLabelValue }}$
    {{- end }}

    [OUTPUT]
        Name          http
        Match         kube.*
        Host          {{ include "deployment.fullname" . }}.{{ .Release.Namespace }}.svc.cluster.local
        Port          {{ .Values.runtime.api.port }}
        URI           /internal/logs
        Format        json
        Json_Date_Key observed_at
        Header        Authorization Bearer ${COLLECTOR_TOKEN}

  parsers.conf: |
    [PARSER]
        Name        cri
        Format      regex
        Regex       ^(?<time>[^ ]+) (?<stream>stdout|stderr) (?<logtag>[^ ]*) (?<log>.*)$
        Time_Key    time
        Time_Format %Y-%m-%dT%H:%M:%S.%L%z
{{- end }}
"#
    .to_string()
}

fn whitelabeled_log_collector_daemonset_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: whitelabeled-log-collector
spec:
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
      app.kubernetes.io/component: whitelabeled-log-collector
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
        app.kubernetes.io/component: whitelabeled-log-collector
    spec:
      serviceAccountName: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
      tolerations:
        - operator: Exists
      containers:
        - name: collector
          image: "{{ .Values.logCollector.image.repository }}:{{ .Values.logCollector.image.tag }}"
          imagePullPolicy: {{ .Values.logCollector.image.pullPolicy }}
          args:
            - -c
            - /collector/etc/collector.conf
          env:
            - name: COLLECTOR_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "deployment.fullname" . }}
                  key: collector-token
          volumeMounts:
            - name: config
              mountPath: /collector/etc
              readOnly: true
            - name: varlog
              mountPath: /var/log
              readOnly: true
            - name: dockercontainers
              mountPath: /var/lib/docker/containers
              readOnly: true
            - name: buffers
              mountPath: /buffers
          resources:
            {{- toYaml .Values.logCollector.resources | nindent 12 }}
      volumes:
        - name: config
          configMap:
            name: {{ include "deployment.fullname" . }}-whitelabeled-log-collector
        - name: varlog
          hostPath:
            path: /var/log
            type: Directory
        # Docker-runtime clusters symlink pod logs to /var/lib/docker/containers;
        # mount it so fluent-bit can follow them. DirectoryOrCreate is harmless on
        # containerd nodes where the path doesn't exist.
        - name: dockercontainers
          hostPath:
            path: /var/lib/docker/containers
            type: DirectoryOrCreate
        - name: buffers
          emptyDir: {}
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
        "deployment".to_string()
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
        ResourceOutputs, ResourceStatus, StackResourceState, Storage, WorkerCode,
        WorkerPublicEndpoint, WorkerTrigger,
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
        // Manager-fetch path: a token + deploymentId are required (the chart
        // refuses to install without them — guards against half-configured
        // values).
        let manager_fetch_values = r#"
management:
  url: "https://manager.example.com"
  name: "test-manager"
  token: "test-sync-token"
  deploymentId: "test-deployment-id"
"#;
        crate::test_utils::helm_template_and_validate(&files, Some(manager_fetch_values))
            .assert_ok("helm template manager-fetch path / registered setup");
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
}

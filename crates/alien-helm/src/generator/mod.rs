//! Top-level Helm chart generator.
//!
//! Drives per-resource [`HelmEmitter`]s through the [`HelmRegistry`] and
//! assembles the chart shell — `Chart.yaml`, the templates, and the
//! values + schema for both bootstrap paths (`registered setup` when
//! `management.deploymentId` is set; external-bindings initialize otherwise).

mod examples;
mod operator;
mod schema;
mod templates;
mod values;

#[cfg(test)]
mod tests;

use examples::*;
pub use operator::generate_operator_manifest;
use schema::*;
use templates::*;
use values::*;

use crate::registry::HelmRegistry;
use alien_core::{ErrorData, Platform, Result, Stack, StackSettings};
use alien_error::{AlienError, Context, IntoAlienError};
use indexmap::IndexMap;
use serde::Serialize;

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

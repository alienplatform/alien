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
    access_request_crd::AccessRequestCrdNames,
    branded_tag_key,
    import::EmitContext,
    sync::{OperatorImageReport, OperatorImageSource},
    AzureResourceGroupOutputs, Container, ContainerCode, Daemon, DaemonCode, ErrorData,
    KubernetesCluster, KubernetesClusterOutputs, KubernetesClusterOwnership,
    KubernetesClusterProvider, Platform, RemoteStackManagementOutputs, ResourceLifecycle, Result,
    ServiceAccount, ServiceAccountOutputs, Stack, StackSettings, Worker, WorkerCode,
    ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_operations_sdk::KubernetesOperationPermissions;
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

/// Version of the operator RBAC policy enforced by this generator.
///
/// Renderers expose this value so callers can reject manifests produced by a
/// generator that predates policy-aware Kubernetes operation permissions.
pub const OPERATOR_RBAC_POLICY_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorPermission {
    /// Diagnose workloads without changing them.
    Diagnostics,
    /// Diagnose workloads and run explicitly approved Kubernetes remediation.
    Remediation,
}

impl OperatorPermission {
    fn as_str(self) -> &'static str {
        match self {
            // Both tiers are observe-only for deployment lifecycle. The tier
            // changes operation RBAC, not ownership of the installed workload.
            Self::Diagnostics | Self::Remediation => "observe",
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
    /// to `.Release.Name`, so one file serves every install without another value.
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
    /// Optional deployment settings passed to the operator. Default settings
    /// are omitted from the manifest.
    pub stack_settings: Option<&'a StackSettings>,
    /// Names the Kubernetes objects and labels. Stable per app/project — the
    /// same across every customer install. (Formerly `release_name`.)
    pub project_name: &'a str,
    /// The per-environment identity reported as `OPERATOR_NAME`. Required for
    /// `RawManifest`; ignored for `HelmTemplate`, which sources it from
    /// `.Release.Name` so each install is distinct.
    pub environment_name: Option<&'a str>,
    /// The namespace the operator installs into. Required (non-empty) for
    /// `RawManifest`; ignored for `HelmTemplate`, which uses `.Release.Namespace`.
    /// In `Namespace` scope this is also the namespace observed.
    pub install_namespace: Option<&'a str>,
    /// The vendor's brand name (e.g. `acme`, or a real owned domain like
    /// `acme.dev`), used to white-label the access-request CRD
    /// (group/kind/plural). It's slugified, never resolved as DNS — any
    /// stable customer-facing identity works. `None` → the Alien defaults.
    /// Same value the operator carries at runtime, so both agree on the CRD.
    pub label_domain: Option<&'a str>,
    pub scope: OperatorScope,
    /// Optional Kubernetes label selector that narrows what the operator manages,
    /// applied on top of `scope`. Independent of namespace vs cluster scope: a
    /// cluster-scoped operator can still filter to labeled resources, and a
    /// namespaced one can filter within its namespace. `None` manages everything
    /// in scope.
    pub label_selector: Option<&'a str>,
    /// Whether the installed Operator includes the Kubernetes operations
    /// plugin. This gates operation-specific RBAC independently of the
    /// requested permission tier.
    pub kubernetes_operations_enabled: bool,
    /// Declared requirements from enabled custom operations only. The
    /// generator validates these before applying the permission ceiling.
    pub custom_operation_permissions: &'a [KubernetesOperationPermissions],
    pub permission: OperatorPermission,
    pub format: OperatorOutputFormat,
}

/// Installer-owned source identity for an exact Operator image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorImageIdentityOptions<'a> {
    /// An immutable image configured directly by the installer.
    Configured,
    /// An immutable image selected from an exact generated package.
    Package {
        /// Exact package ID resolved by the installer.
        package_id: &'a str,
        /// Exact package version resolved by the installer.
        package_version: &'a str,
    },
}

/// Product-chart overrides for an embedded Remote Operator.
///
/// This separate input keeps [`OperatorManifestOptions`] source-compatible for
/// callers that construct it with a struct literal while allowing a product
/// chart to use a setup-owned credentials Secret and release-aware object name.
pub struct ProductOperatorManifestOptions<'a> {
    pub manifest: OperatorManifestOptions<'a>,
    /// Existing Secret containing `sync-token` and `encryption-key` (and
    /// `collector-token` when the log collector is enabled).
    pub credentials_secret_name: &'a str,
    /// Expected lowercase SHA-256 fingerprint of the existing Secret's decoded
    /// `encryption-key`. Product charts normally source this from the reviewed
    /// Helm values that setup produced for the installation.
    pub credentials_encryption_key_sha256: &'a str,
    /// Optional exact Kubernetes object name for standalone manifest
    /// generation. Product chart composition replaces this with a name derived
    /// from the immutable Helm release namespace and name.
    pub resource_name: Option<&'a str>,
}

pub struct OperatorLogCollectorOptions<'a> {
    pub image: &'a str,
    pub token: &'a str,
    /// Existing Pod label key to collect. Set together with `pod_label_value`.
    pub pod_label_key: Option<&'a str>,
    /// Existing Pod label value to collect. Set together with `pod_label_key`.
    pub pod_label_value: Option<&'a str>,
}

/// Generate a Helm chart for `stack`.
pub fn generate_helm_chart(stack: &Stack, options: HelmOptions<'_>) -> Result<HelmChart> {
    generate_helm_chart_internal(stack, options, None)
}

/// Generate a product Helm chart with the Remote Operator in the same release.
pub fn generate_product_helm_chart(
    stack: &Stack,
    options: HelmOptions<'_>,
    remote_operator: ProductOperatorManifestOptions<'_>,
) -> Result<HelmChart> {
    generate_helm_chart_internal(stack, options, Some((remote_operator, None)))
}

/// Generate a product Helm chart carrying an exact Remote Operator image receipt.
pub fn generate_product_helm_chart_with_image_identity(
    stack: &Stack,
    options: HelmOptions<'_>,
    remote_operator: ProductOperatorManifestOptions<'_>,
    image_identity: OperatorImageIdentityOptions<'_>,
) -> Result<HelmChart> {
    generate_helm_chart_internal(
        stack,
        options,
        Some((remote_operator, Some(image_identity))),
    )
}

fn generate_helm_chart_internal(
    stack: &Stack,
    options: HelmOptions<'_>,
    remote_operator: Option<(
        ProductOperatorManifestOptions<'_>,
        Option<OperatorImageIdentityOptions<'_>>,
    )>,
) -> Result<HelmChart> {
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
    files.insert("values.schema.json".to_string(), values_schema_json(stack)?);
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
    files.insert(
        "templates/runtime-cleanup-scope.yaml".to_string(),
        runtime_cleanup_scope_tpl(),
    );
    files.insert(
        "templates/runtime-cleanup-history-prune.yaml".to_string(),
        runtime_cleanup_history_prune_tpl(),
    );
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
    let has_remote_operator = remote_operator.is_some();

    if let Some((remote_operator, image_identity)) = remote_operator {
        add_remote_operator_files(&mut files, remote_operator, image_identity)?;
    }

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
    let mut readme = readme_md(&chart_name);
    if has_remote_operator {
        readme.push_str(
            "\n## Runtime cleanup and Helm history\n\nRuntime cleanup uses the Secret Helm history backend by default. When Helm is configured with `HELM_DRIVER=configmap`, set `runtime.cleanup.onUninstall.helmHistoryBackend=configmap`. The SQL and memory backends are unsupported because the chart cannot verify and prune unsafe rollback history.\n\n## Remote Operator\n\nThe Remote Operator is disabled by default and adds no cluster-scoped resources until enabled. Install or upgrade the chart once with it disabled, then enable it in a second upgrade with `remoteOperator.bootstrapIdentity=true`; this proves that Helm uses a supported Kubernetes history backend before any durable identity is created. Enabling it may create the shared access-request CustomResourceDefinition and therefore requires cluster-administrator approval. The chart retains that CRD on rollback and uninstall, reuses an existing matching definition without adopting it, and refuses a conflicting definition instead of changing it. Protected upgrades use the Secret Helm history backend by default. When Helm is configured with `HELM_DRIVER=configmap`, set both `runtime.cleanup.onUninstall.helmHistoryBackend=configmap` and `remoteOperator.helmHistoryBackend=configmap`. Before identity creation, a credential-free one-shot Job mounts the exact pending Helm record from that backend and verifies a render-specific proof, so stale records cannot authorize an upgrade. The SQL and memory storage backends are rejected because the chart cannot verify or prune their rollback history. Uninstall permanently retires this release by deleting its exact retained identity records and identity PVC.\n",
        );
    }
    files.insert("README.md".to_string(), readme);
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

fn add_remote_operator_files(
    files: &mut IndexMap<String, String>,
    mut options: ProductOperatorManifestOptions<'_>,
    image_identity: Option<OperatorImageIdentityOptions<'_>>,
) -> Result<()> {
    if options.manifest.format != OperatorOutputFormat::HelmTemplate {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "a product chart requires the Helm Remote Operator template".to_string(),
        }));
    }

    let chart = files.get_mut("Chart.yaml").ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "the product chart is missing Chart.yaml".to_string(),
        })
    })?;
    chart.push_str("annotations:\n  alien.dev/remote-operator-lifecycle: \"v2\"\n");

    if let Some(label_domain) = options.manifest.label_domain {
        let values = files.get_mut("values.yaml").ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "the product chart is missing values.yaml".to_string(),
            })
        })?;
        let default_label_key = "    deploymentLabelKey: \"alien.dev/deployment\"";
        let default_legacy_label_key = "    legacyDeploymentLabelKey: \"\"";
        if !values.contains(default_label_key) {
            return Err(AlienError::new(ErrorData::GenericError {
                message: "the product chart is missing its runtime deployment label key"
                    .to_string(),
            }));
        }
        if !values.contains(default_legacy_label_key) {
            return Err(AlienError::new(ErrorData::GenericError {
                message: "the product chart is missing its legacy runtime deployment label key"
                    .to_string(),
            }));
        }
        let deployment_label_key = branded_tag_key(
            alien_core::access_request_crd::current_kubernetes_label_domain(label_domain),
            ALIEN_STACK_TAG_KEY,
        );
        *values = values.replacen(
            default_label_key,
            &format!(
                "    deploymentLabelKey: {}",
                yaml_string(&deployment_label_key)
            ),
            1,
        );
        if let Some(legacy_label_domain) =
            alien_core::access_request_crd::explicit_legacy_kubernetes_label_domain(label_domain)
        {
            let legacy_deployment_label_key =
                branded_tag_key(legacy_label_domain, ALIEN_STACK_TAG_KEY);
            *values = values.replacen(
                default_legacy_label_key,
                &format!(
                    "    legacyDeploymentLabelKey: {}",
                    yaml_string(&legacy_deployment_label_key)
                ),
                1,
            );
        }
    }

    let requires_collector_token = options.manifest.log_collector.is_some();
    let identity_record = remote_operator_identity_record_tpl(
        options.credentials_secret_name,
        options.credentials_encryption_key_sha256,
    );
    options.resource_name = Some("{{ include \"deployment.remoteOperatorResourceName\" . }}");
    let manifest = generate_product_operator_manifest_with_identity_marker(
        options,
        Some("{{ include \"deployment.remoteOperatorIdentityInitializedName\" . }}"),
        Some("{{ include \"deployment.remoteOperatorLogCollectorName\" . }}"),
        image_identity,
    )?;
    let mut crd = None;
    let mut templates = Vec::new();
    for document in manifest
        .split("\n---\n")
        .map(str::trim)
        .filter(|doc| !doc.is_empty())
    {
        let kind = document
            .lines()
            .find_map(|line| line.strip_prefix("kind: "))
            .map(|kind| kind.trim_matches(['\'', '"']));
        if kind == Some("CustomResourceDefinition") {
            if crd.replace(document).is_some() {
                return Err(AlienError::new(ErrorData::GenericError {
                    message: "the Remote Operator renderer emitted more than one CRD".to_string(),
                }));
            }
        } else {
            templates.push(document);
        }
    }
    let crd = crd.ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "the Remote Operator renderer did not emit its access-request CRD".to_string(),
        })
    })?;
    if templates.is_empty() {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "the Remote Operator renderer did not emit workload resources".to_string(),
        }));
    }

    files.insert(
        "templates/remote-operator-crd.yaml".to_string(),
        remote_operator_crd_tpl(crd)?,
    );
    files.insert(
        "templates/remote-operator.yaml".to_string(),
        format!(
            "{{{{- define \"deployment.remoteOperatorResources\" }}}}\n{}\n{{{{- end }}}}\n{{{{- if .Values.remoteOperator.enabled }}}}\n{{{{ include \"deployment.remoteOperatorResources\" . }}}}\n{{{{- end }}}}\n",
            templates.join("\n---\n")
        ),
    );
    files.insert(
        "templates/remote-operator-identity-record.yaml".to_string(),
        identity_record,
    );
    files.insert(
        "templates/remote-operator-lifecycle-capability.yaml".to_string(),
        remote_operator_lifecycle_capability_tpl(),
    );
    files.insert(
        "templates/remote-operator-cleanup-rbac.yaml".to_string(),
        remote_operator_cleanup_rbac_tpl(),
    );
    files.insert(
        "templates/remote-operator-identity-initialized.yaml".to_string(),
        remote_operator_identity_initialized_tpl(),
    );
    files.insert(
        "templates/remote-operator-identity-initialized-rbac.yaml".to_string(),
        remote_operator_identity_initialized_rbac_tpl(),
    );
    files.insert(
        "templates/remote-operator-history-backend-check.yaml".to_string(),
        remote_operator_history_backend_check_tpl(),
    );
    files.insert(
        "templates/remote-operator-identity-gate.yaml".to_string(),
        remote_operator_identity_gate_tpl(),
    );
    files.insert(
        "templates/remote-operator-identity-complete.yaml".to_string(),
        remote_operator_identity_completion_tpl(),
    );
    files.insert(
        "templates/remote-operator-checks.yaml".to_string(),
        remote_operator_checks_tpl(requires_collector_token),
    );
    files.insert(
        "templates/remote-operator-cleanup-job.yaml".to_string(),
        remote_operator_cleanup_job_tpl(),
    );
    files.insert(
        "templates/remote-operator-rollback-guard.yaml".to_string(),
        remote_operator_rollback_guard_tpl(),
    );
    files.insert(
        "templates/NOTES.txt".to_string(),
        remote_operator_removal_notes_tpl(),
    );

    let values = files.get_mut("values.yaml").ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "the product chart is missing values.yaml".to_string(),
        })
    })?;
    values.push_str(remote_operator_values());

    let schema = files.get_mut("values.schema.json").ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "the product chart is missing values.schema.json".to_string(),
        })
    })?;
    let mut schema_json: serde_json::Value = serde_json::from_str(schema)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to parse the product chart values schema".to_string(),
        })?;
    let properties = schema_json
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "the product chart values schema has no properties object".to_string(),
            })
        })?;
    properties.insert(
        "remoteOperator".to_string(),
        remote_operator_values_schema(),
    );
    *schema = serde_json::to_string_pretty(&schema_json)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize the product chart values schema".to_string(),
        })?;
    schema.push('\n');

    Ok(())
}

fn remote_operator_crd_tpl(crd: &str) -> Result<String> {
    let parsed: serde_yaml::Value = serde_yaml::from_str(crd).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "failed to parse the Remote Operator access-request CRD".to_string(),
        },
    )?;
    let crd_name = parsed
        .get("metadata")
        .and_then(|metadata| metadata.get("name"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "the Remote Operator access-request CRD has no metadata.name".to_string(),
            })
        })?;
    let retained_crd = crd.replacen(
        "metadata:\n",
        "metadata:\n  annotations:\n    helm.sh/resource-policy: keep\n",
        1,
    );
    if retained_crd == crd {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "the Remote Operator access-request CRD has no metadata block".to_string(),
        }));
    }

    Ok(format!(
        r#"{{{{- define "deployment.remoteOperatorAccessRequestCrd" -}}}}
{retained_crd}{{{{- end -}}}}
{{{{- if .Values.remoteOperator.enabled -}}}}
{{{{- $existing := lookup "apiextensions.k8s.io/v1" "CustomResourceDefinition" "" "{crd_name}" -}}}}
{{{{- if not $existing }}}}
{{{{ include "deployment.remoteOperatorAccessRequestCrd" . }}}}
{{{{- end }}}}
{{{{- end }}}}
"#
    ))
}

fn remote_operator_identity_record_tpl(
    credentials_secret_name: &str,
    credentials_encryption_key_sha256: &str,
) -> String {
    format!(
        r#"{{{{- define "deployment.remoteOperatorReleaseIdentity" -}}}}
{{{{- printf "%s/%s" .Release.Namespace .Release.Name | sha256sum | trunc 16 -}}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorResourceName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 21 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{- printf "%s-remote-operator-%s" $releasePrefix $releaseIdentity | trunc 63 | trimSuffix "-" -}}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorIdentityRecordName" -}}}}
{{{{ include "deployment.remoteOperatorResourceName" . }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorCredentialsSecretName" -}}}}
{credentials_secret_name}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorEncryptionKeySha256" -}}}}
{credentials_encryption_key_sha256}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorIdentityCompletionName" -}}}}
{{{{ printf "%s-complete" (include "deployment.remoteOperatorIdentityRecordName" .) | trunc 253 | trimSuffix "-" }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorIdentityInitializedName" -}}}}
{{{{ printf "%s-initialized" (include "deployment.remoteOperatorIdentityRecordName" .) | trunc 253 | trimSuffix "-" }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorIdentityInitializedRbacName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 30 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{- printf "%s-identity-init-%s" $releasePrefix $releaseIdentity | trunc 63 | trimSuffix "-" -}}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorHistoryBackendCheckName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 30 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{- printf "%s-history-check-%s" $releasePrefix $releaseIdentity | trunc 63 | trimSuffix "-" -}}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorLogCollectorName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 22 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{- printf "%s-log-collector-%s" $releasePrefix $releaseIdentity }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorLifecycleCapabilityName" -}}}}
{{{{ printf "%s-lifecycle-v2" (include "deployment.remoteOperatorIdentityRecordName" .) | trunc 253 | trimSuffix "-" }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorCleanupName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 30 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{ printf "%s-cleanup-%s" $releasePrefix $releaseIdentity }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorIdentityGateName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 30 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{ printf "%s-identity-gate-%s" $releasePrefix $releaseIdentity }}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorRemovalConfirmed" -}}}}
{{{{- if and (not .Values.remoteOperator.enabled) (eq (default "" .Values.remoteOperator.confirmRemoval | toString) .Release.Name) -}}}}
true
{{{{- end -}}}}
{{{{- end -}}}}
{{{{- define "deployment.remoteOperatorRollbackGuardName" -}}}}
{{{{- $releasePrefix := regexReplaceAll "[^a-z0-9-]+" (lower .Release.Name) "-" | trunc 30 | trimAll "-" -}}}}
{{{{- $releaseIdentity := include "deployment.remoteOperatorReleaseIdentity" . -}}}}
{{{{ printf "%s-rollback-guard-%s" $releasePrefix $releaseIdentity | trunc 63 | trimSuffix "-" }}}}
{{{{- end -}}}}
{{{{- if .Values.remoteOperator.enabled -}}}}
{{{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . -}}}}
{{{{- $identityRecord := lookup "v1" "ConfigMap" .Release.Namespace $identityRecordName -}}}}
{{{{- if not $identityRecord }}}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{{{ $identityRecordName }}}}
  namespace: {{{{ .Release.Namespace }}}}
  annotations:
    meta.helm.sh/release-name: {{{{ .Release.Name | quote }}}}
    meta.helm.sh/release-namespace: {{{{ .Release.Namespace | quote }}}}
    helm.sh/hook: pre-install,pre-upgrade
    helm.sh/hook-weight: "-100"
    helm.sh/resource-policy: keep
  labels:
    app.kubernetes.io/managed-by: {{{{ .Release.Service | quote }}}}
    app.kubernetes.io/instance: {{{{ .Release.Name | quote }}}}
    alien.dev/remote-operator-identity-record: "true"
    alien.dev/remote-operator-identity-phase: prepared
    alien.dev/remote-operator-release-id: {{{{ include "deployment.remoteOperatorReleaseIdentity" . | quote }}}}
immutable: true
data:
  version: "3"
  credentialsSecretName: {{{{ include "deployment.remoteOperatorCredentialsSecretName" . | trim | quote }}}}
  encryptionKeySha256: {{{{ include "deployment.remoteOperatorEncryptionKeySha256" . | trim | quote }}}}
{{{{- end }}}}
{{{{- end }}}}
"#,
        credentials_secret_name = credentials_secret_name,
        credentials_encryption_key_sha256 = credentials_encryption_key_sha256,
    )
}

fn remote_operator_lifecycle_capability_tpl() -> String {
    r#"{{- $lifecycleCapabilityName := include "deployment.remoteOperatorLifecycleCapabilityName" . -}}
{{- $lifecycleCapability := lookup "v1" "ConfigMap" .Release.Namespace $lifecycleCapabilityName -}}
{{- $firstGuardRevision := .Release.Revision -}}
{{- if $lifecycleCapability -}}
  {{- $firstGuardRevision = index (default dict $lifecycleCapability.data) "firstGuardRevision" -}}
{{- end }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ $lifecycleCapabilityName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
  labels:
    app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
    app.kubernetes.io/instance: {{ .Release.Name | quote }}
    alien.dev/remote-operator-lifecycle-capability: "v2"
    alien.dev/remote-operator-release-id: {{ include "deployment.remoteOperatorReleaseIdentity" . | quote }}
immutable: true
data:
  version: "2"
  firstGuardRevision: {{ $firstGuardRevision | quote }}
"#
    .to_string()
}

fn remote_operator_cleanup_rbac_tpl() -> String {
    r#"{{- $cleanupName := include "deployment.remoteOperatorCleanupName" . -}}
{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . -}}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . -}}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . -}}
{{- $lifecycleCapabilityName := include "deployment.remoteOperatorLifecycleCapabilityName" . -}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ $cleanupName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ $cleanupName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps"]
    resourceNames:
      - {{ $identityRecordName }}
      - {{ $identityInitializedName }}
      - {{ $identityCompletionName }}
      - {{ $lifecycleCapabilityName }}
    verbs: ["get", "delete"]
  - apiGroups: [""]
    resources: ["persistentvolumeclaims"]
    resourceNames:
      - {{ printf "%s-identity" $identityRecordName }}
    verbs: ["get", "delete"]
  - apiGroups: ["apps"]
    resources: ["deployments"]
    resourceNames:
      - {{ $identityRecordName }}
    verbs: ["get", "delete"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ $cleanupName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ $cleanupName }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ $cleanupName }}
"#
    .to_string()
}

fn remote_operator_identity_initialized_tpl() -> String {
    r#"{{- if .Values.remoteOperator.enabled -}}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . -}}
{{- $identityInitialized := lookup "v1" "ConfigMap" .Release.Namespace $identityInitializedName -}}
{{- if not $identityInitialized }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ $identityInitializedName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
    helm.sh/resource-policy: keep
  labels:
    app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
    app.kubernetes.io/instance: {{ .Release.Name | quote }}
    alien.dev/remote-operator-identity-phase: pending
    alien.dev/remote-operator-release-id: {{ include "deployment.remoteOperatorReleaseIdentity" . | quote }}
immutable: false
data:
  version: "1"
  identityRecordName: {{ include "deployment.remoteOperatorIdentityRecordName" . | quote }}
{{- end }}
{{- end }}
"#
    .to_string()
}

fn remote_operator_identity_initialized_rbac_tpl() -> String {
    r#"{{- if .Values.remoteOperator.enabled }}
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: {{ include "deployment.remoteOperatorIdentityInitializedRbacName" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
rules:
  - apiGroups: [""]
    resources: ["configmaps"]
    resourceNames:
      - {{ include "deployment.remoteOperatorIdentityInitializedName" . }}
    verbs: ["get", "patch", "update"]
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: {{ include "deployment.remoteOperatorIdentityInitializedRbacName" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.remoteOperatorResourceName" . }}
    namespace: {{ .Release.Namespace }}
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.remoteOperatorIdentityInitializedRbacName" . }}
{{- end }}
"#
    .to_string()
}

fn remote_operator_identity_completion_tpl() -> String {
    r#"{{- if .Values.remoteOperator.enabled -}}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . -}}
{{- $identityCompletion := lookup "v1" "ConfigMap" .Release.Namespace $identityCompletionName -}}
{{- if not $identityCompletion }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ $identityCompletionName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
    helm.sh/hook: post-install,post-upgrade
    helm.sh/hook-weight: "100"
    helm.sh/resource-policy: keep
  labels:
    app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
    app.kubernetes.io/instance: {{ .Release.Name | quote }}
    alien.dev/remote-operator-identity-phase: complete
    alien.dev/remote-operator-release-id: {{ include "deployment.remoteOperatorReleaseIdentity" . | quote }}
immutable: true
data:
  version: "1"
  identityRecordName: {{ include "deployment.remoteOperatorIdentityRecordName" . | quote }}
{{- end }}
{{- end }}
"#
    .to_string()
}

fn remote_operator_history_backend_check_tpl() -> String {
    r#"{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled }}
{{- $checkName := include "deployment.remoteOperatorHistoryBackendCheckName" . -}}
{{- $historyRecordName := printf "sh.helm.release.v1.%s.v%d" .Release.Name (.Release.Revision | int) -}}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ $checkName }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-upgrade
    "helm.sh/hook-weight": "-127"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded,hook-failed
spec:
  backoffLimit: 0
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      automountServiceAccountToken: false
      restartPolicy: Never
      volumes:
        - name: helm-history
          {{- if eq .Values.remoteOperator.helmHistoryBackend "secret" }}
          secret:
            secretName: {{ $historyRecordName | quote }}
          {{- else }}
          configMap:
            name: {{ $historyRecordName | quote }}
          {{- end }}
      containers:
        - name: verify-history-backend
          image: "{{ dig "image" "repository" "alpine/k8s" (dig "cleanup" "onUninstall" dict .Values.runtime) }}:{{ dig "image" "tag" "1.32.0" (dig "cleanup" "onUninstall" dict .Values.runtime) }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" (dig "cleanup" "onUninstall" dict .Values.runtime) }}
          command:
            - /bin/sh
            - -ec
            - |
              history_proof={{ randAlphaNum 32 | quote }}
              if ! base64 -d /history/release | gzip -d | grep -F -q "$history_proof"; then
                echo "The configured Helm history backend does not contain this exact pending upgrade. Refusing to initialize Remote Operator identity." >&2
                exit 1
              fi
          volumeMounts:
            - name: helm-history
              mountPath: /history
              readOnly: true
{{- end }}
"#
    .to_string()
}

fn remote_operator_identity_gate_tpl() -> String {
    r#"{{- if .Values.remoteOperator.enabled }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.remoteOperatorIdentityGateName" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": post-install,post-upgrade
    "helm.sh/hook-weight": "90"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded
spec:
  backoffLimit: 0
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      restartPolicy: Never
      containers:
        - name: wait-for-identity
          image: "{{ dig "image" "repository" "alpine/k8s" (dig "cleanup" "onUninstall" dict .Values.runtime) }}:{{ dig "image" "tag" "1.32.0" (dig "cleanup" "onUninstall" dict .Values.runtime) }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" (dig "cleanup" "onUninstall" dict .Values.runtime) }}
          command:
            - /bin/sh
            - -ec
            - |
              deployment_name={{ include "deployment.remoteOperatorResourceName" . | quote }}
              deployment_ready=false
              for _ in $(seq 1 150); do
                generation="$(kubectl --namespace={{ .Release.Namespace | quote }} get deployment "$deployment_name" --output='jsonpath={.metadata.generation}')"
                observed="$(kubectl --namespace={{ .Release.Namespace | quote }} get deployment "$deployment_name" --output='jsonpath={.status.observedGeneration}')"
                desired="$(kubectl --namespace={{ .Release.Namespace | quote }} get deployment "$deployment_name" --output='jsonpath={.spec.replicas}')"
                available="$(kubectl --namespace={{ .Release.Namespace | quote }} get deployment "$deployment_name" --output='jsonpath={.status.availableReplicas}')"
                desired="${desired:-1}"
                available="${available:-0}"
                if [ "$observed" = "$generation" ] && [ "$available" = "$desired" ]; then
                  deployment_ready=true
                  break
                fi
                sleep 2
              done
              if [ "$deployment_ready" != "true" ]; then
                echo "Remote Operator deployment $deployment_name did not become ready within 5 minutes." >&2
                exit 1
              fi
              initialized_name={{ include "deployment.remoteOperatorIdentityInitializedName" . | quote }}
              phase="$(kubectl --namespace={{ .Release.Namespace | quote }} get configmap "$initialized_name" --output='jsonpath={.metadata.labels.alien\.dev/remote-operator-identity-phase}')"
              immutable="$(kubectl --namespace={{ .Release.Namespace | quote }} get configmap "$initialized_name" --output='jsonpath={.immutable}')"
              if [ "$phase" != "initialized" ] || [ "$immutable" != "true" ]; then
                echo "Remote Operator became ready without recording a durable initialized identity." >&2
                exit 1
              fi
{{- end }}
"#
    .to_string()
}

fn remote_operator_cleanup_job_tpl() -> String {
    r#"{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . -}}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . -}}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . -}}
{{- $lifecycleCapabilityName := include "deployment.remoteOperatorLifecycleCapabilityName" . -}}
{{- $identityRecord := lookup "v1" "ConfigMap" .Release.Namespace $identityRecordName -}}
{{- $identityInitialized := lookup "v1" "ConfigMap" .Release.Namespace $identityInitializedName -}}
{{- $identityCompletion := lookup "v1" "ConfigMap" .Release.Namespace $identityCompletionName -}}
{{- $lifecycleCapability := lookup "v1" "ConfigMap" .Release.Namespace $lifecycleCapabilityName -}}
{{- if or (not .Values.remoteOperator.enabled) $lifecycleCapability }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.remoteOperatorCleanupName" . }}
  namespace: {{ .Release.Namespace }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-delete
    "helm.sh/hook-weight": "-20"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded,hook-failed
spec:
  backoffLimit: 1
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "deployment.remoteOperatorCleanupName" . }}
      restartPolicy: Never
      containers:
        - name: cleanup
          image: "{{ dig "image" "repository" "alpine/k8s" (dig "cleanup" "onUninstall" dict .Values.runtime) }}:{{ dig "image" "tag" "1.32.0" (dig "cleanup" "onUninstall" dict .Values.runtime) }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" (dig "cleanup" "onUninstall" dict .Values.runtime) }}
          command:
            - /bin/sh
            - -ec
            - |
              resource_name={{ include "deployment.remoteOperatorResourceName" . | quote }}
              identity_record={{ $identityRecordName | quote }}
              identity_initialized={{ $identityInitializedName | quote }}
              identity_completion={{ $identityCompletionName | quote }}
              identity_pvc={{ printf "%s-identity" $identityRecordName | quote }}
              lifecycle_capability={{ $lifecycleCapabilityName | quote }}
              remote_operator_enabled={{ .Values.remoteOperator.enabled | quote }}
              namespace={{ .Release.Namespace | quote }}
              release_name={{ .Release.Name | quote }}
              release_service={{ .Release.Service | quote }}
              release_id={{ include "deployment.remoteOperatorReleaseIdentity" . | quote }}

              field() {
                kubectl -n "$namespace" get "$1" "$2" -o "jsonpath=$3"
              }
              resource_exists() {
                resource_ref="$(kubectl -n "$namespace" get "$1" "$2" --ignore-not-found -o name)" || {
                  echo "Refusing cleanup: cannot determine whether $1 $namespace/$2 exists." >&2
                  exit 1
                }
                [ -n "$resource_ref" ]
              }
              require_field() {
                kind="$1"
                name="$2"
                path="$3"
                expected="$4"
                description="$5"
                actual="$(field "$kind" "$name" "$path")" || {
                  echo "Refusing cleanup: cannot read $kind $namespace/$name." >&2
                  exit 1
                }
                if [ "$actual" != "$expected" ]; then
                  echo "Refusing cleanup: $kind $namespace/$name has unexpected $description." >&2
                  exit 1
                fi
              }
              identity_initialized_phase=""
              validate_identity_initialized() {
                require_field configmap "$identity_initialized" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                require_field configmap "$identity_initialized" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                require_field configmap "$identity_initialized" '{.metadata.annotations.helm\.sh/resource-policy}' keep resource-policy
                require_field configmap "$identity_initialized" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                require_field configmap "$identity_initialized" '{.metadata.labels.app\.kubernetes\.io/instance}' "$release_name" instance
                require_field configmap "$identity_initialized" '{.metadata.labels.alien\.dev/remote-operator-release-id}' "$release_id" release-id
                require_field configmap "$identity_initialized" '{.data.version}' 1 version
                require_field configmap "$identity_initialized" '{.data.identityRecordName}' "$identity_record" identity-record-reference
                identity_initialized_phase="$(field configmap "$identity_initialized" '{.metadata.labels.alien\.dev/remote-operator-identity-phase}')" || {
                  echo "Refusing cleanup: cannot read configmap $namespace/$identity_initialized." >&2
                  exit 1
                }
                case "$identity_initialized_phase" in
                  pending)
                    require_field configmap "$identity_initialized" '{.immutable}' false immutability
                    ;;
                  initialized)
                    require_field configmap "$identity_initialized" '{.immutable}' true immutability
                    ;;
                  *)
                    echo "Refusing cleanup: configmap $namespace/$identity_initialized has an unknown identity phase." >&2
                    exit 1
                    ;;
                esac
              }
              owned_by_this_release() {
                kind="$1"
                name="$2"
                expected_instance="$3"
                actual_release="$(field "$kind" "$name" '{.metadata.annotations.meta\.helm\.sh/release-name}')" || {
                  echo "Refusing cleanup: cannot read $kind $namespace/$name." >&2
                  exit 1
                }
                actual_namespace="$(field "$kind" "$name" '{.metadata.annotations.meta\.helm\.sh/release-namespace}')" || {
                  echo "Refusing cleanup: cannot read $kind $namespace/$name." >&2
                  exit 1
                }
                actual_service="$(field "$kind" "$name" '{.metadata.labels.app\.kubernetes\.io/managed-by}')" || {
                  echo "Refusing cleanup: cannot read $kind $namespace/$name." >&2
                  exit 1
                }
                actual_instance="$(field "$kind" "$name" '{.metadata.labels.app\.kubernetes\.io/instance}')" || {
                  echo "Refusing cleanup: cannot read $kind $namespace/$name." >&2
                  exit 1
                }
                [ "$actual_release" = "$release_name" ] && [ "$actual_namespace" = "$namespace" ] && [ "$actual_service" = "$release_service" ] && [ "$actual_instance" = "$expected_instance" ]
              }
              if ! resource_exists configmap "$identity_record"; then
                if resource_exists configmap "$identity_completion"; then
                  echo "Refusing cleanup: completion record $namespace/$identity_completion exists without its identity record." >&2
                  exit 1
                fi
                if resource_exists deployment "$resource_name"; then
                  if [ "$remote_operator_enabled" = "true" ] && ! owned_by_this_release deployment "$resource_name" "$resource_name"; then
                    echo "Refusing cleanup: Deployment $namespace/$resource_name is not owned by this exact Helm release." >&2
                    exit 1
                  fi
                  if owned_by_this_release deployment "$resource_name" "$resource_name"; then
                    echo "Refusing cleanup: Remote Operator workload or identity storage remains without its identity record." >&2
                    exit 1
                  fi
                fi
                if resource_exists persistentvolumeclaim "$identity_pvc" && owned_by_this_release persistentvolumeclaim "$identity_pvc" "$resource_name"; then
                  echo "Refusing cleanup: Remote Operator workload or identity storage remains without its identity record." >&2
                  exit 1
                fi
                if resource_exists configmap "$identity_initialized"; then
                  validate_identity_initialized
                  if [ "$identity_initialized_phase" = "initialized" ]; then
                    echo "Refusing cleanup: initialized record $namespace/$identity_initialized exists without its identity record." >&2
                    exit 1
                  fi
                  kubectl -n "$namespace" delete configmap "$identity_initialized"
                fi
                if resource_exists configmap "$lifecycle_capability"; then
                  require_field configmap "$lifecycle_capability" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                  require_field configmap "$lifecycle_capability" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                  require_field configmap "$lifecycle_capability" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                  require_field configmap "$lifecycle_capability" '{.metadata.labels.app\.kubernetes\.io/instance}' "$release_name" instance
                  require_field configmap "$lifecycle_capability" '{.metadata.labels.alien\.dev/remote-operator-lifecycle-capability}' v2 lifecycle-capability
                  require_field configmap "$lifecycle_capability" '{.metadata.labels.alien\.dev/remote-operator-release-id}' "$release_id" release-id
                  require_field configmap "$lifecycle_capability" '{.immutable}' true immutability
                  require_field configmap "$lifecycle_capability" '{.data.version}' 2 version
                  kubectl -n "$namespace" delete configmap "$lifecycle_capability"
                fi
                echo "No Remote Operator identity record exists for this release; nothing to clean up."
                exit 0
              fi

              require_field configmap "$identity_record" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
              require_field configmap "$identity_record" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
              require_field configmap "$identity_record" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
              require_field configmap "$identity_record" '{.metadata.labels.app\.kubernetes\.io/instance}' "$release_name" instance
              require_field configmap "$identity_record" '{.metadata.labels.alien\.dev/remote-operator-identity-record}' true identity-record
              require_field configmap "$identity_record" '{.metadata.labels.alien\.dev/remote-operator-release-id}' "$release_id" release-id
              require_field configmap "$identity_record" '{.immutable}' true immutability
              require_field configmap "$identity_record" '{.data.version}' 3 version

              if resource_exists configmap "$identity_initialized"; then
                validate_identity_initialized
              fi

              if resource_exists configmap "$identity_completion"; then
                require_field configmap "$identity_completion" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                require_field configmap "$identity_completion" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                require_field configmap "$identity_completion" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                require_field configmap "$identity_completion" '{.metadata.labels.app\.kubernetes\.io/instance}' "$release_name" instance
                require_field configmap "$identity_completion" '{.metadata.labels.alien\.dev/remote-operator-identity-phase}' complete identity-phase
                require_field configmap "$identity_completion" '{.metadata.labels.alien\.dev/remote-operator-release-id}' "$release_id" release-id
                require_field configmap "$identity_completion" '{.immutable}' true immutability
                require_field configmap "$identity_completion" '{.data.version}' 1 version
                require_field configmap "$identity_completion" '{.data.identityRecordName}' "$identity_record" identity-record-reference
              fi

              if resource_exists configmap "$lifecycle_capability"; then
                require_field configmap "$lifecycle_capability" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                require_field configmap "$lifecycle_capability" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                require_field configmap "$lifecycle_capability" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                require_field configmap "$lifecycle_capability" '{.metadata.labels.app\.kubernetes\.io/instance}' "$release_name" instance
                require_field configmap "$lifecycle_capability" '{.metadata.labels.alien\.dev/remote-operator-lifecycle-capability}' v2 lifecycle-capability
                require_field configmap "$lifecycle_capability" '{.metadata.labels.alien\.dev/remote-operator-release-id}' "$release_id" release-id
                require_field configmap "$lifecycle_capability" '{.immutable}' true immutability
                require_field configmap "$lifecycle_capability" '{.data.version}' 2 version
              fi

              if resource_exists deployment "$resource_name"; then
                require_field deployment "$resource_name" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                require_field deployment "$resource_name" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                require_field deployment "$resource_name" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                require_field deployment "$resource_name" '{.metadata.labels.app\.kubernetes\.io/instance}' "$resource_name" instance
              fi
              if resource_exists persistentvolumeclaim "$identity_pvc"; then
                require_field persistentvolumeclaim "$identity_pvc" '{.metadata.annotations.meta\.helm\.sh/release-name}' "$release_name" release-name
                require_field persistentvolumeclaim "$identity_pvc" '{.metadata.annotations.meta\.helm\.sh/release-namespace}' "$namespace" release-namespace
                require_field persistentvolumeclaim "$identity_pvc" '{.metadata.labels.app\.kubernetes\.io/managed-by}' "$release_service" managed-by
                require_field persistentvolumeclaim "$identity_pvc" '{.metadata.labels.app\.kubernetes\.io/instance}' "$resource_name" instance
              fi

              # Stop the exact release-owned workload first.
              kubectl -n "$namespace" delete deployment "$resource_name" --ignore-not-found=true --cascade=foreground --wait=false
              # Keep the validated identity record as the retry proof until the
              # durable volume is confirmed gone. Every earlier failure leaves
              # enough ownership evidence for a later uninstall retry.
              kubectl -n "$namespace" delete persistentvolumeclaim "$identity_pvc" --ignore-not-found=true --wait=false
              durable_delete_seconds_remaining=75
              while resource_exists deployment "$resource_name" || resource_exists persistentvolumeclaim "$identity_pvc"; do
                if [ "$durable_delete_seconds_remaining" -le 0 ]; then
                  echo "Refusing cleanup: timed out waiting for deployment $namespace/$resource_name and persistentvolumeclaim $namespace/$identity_pvc to be deleted." >&2
                  exit 1
                fi
                sleep 1
                durable_delete_seconds_remaining=$((durable_delete_seconds_remaining - 1))
              done
              kubectl -n "$namespace" delete configmap "$identity_completion" --ignore-not-found=true
              kubectl -n "$namespace" delete configmap "$identity_initialized" --ignore-not-found=true
              kubectl -n "$namespace" delete configmap "$lifecycle_capability" --ignore-not-found=true
              kubectl -n "$namespace" delete configmap "$identity_record" --ignore-not-found=true
{{- end }}
"#
    .to_string()
}

fn remote_operator_rollback_guard_tpl() -> String {
    r#"{{- /* Only a confirmed removal of a completed identity is a guard-free rollback target. */ -}}
{{- $identityCompletion := lookup "v1" "ConfigMap" .Release.Namespace (include "deployment.remoteOperatorIdentityCompletionName" .) }}
{{- if not (or .Values.remoteOperator.enabled (and (include "deployment.remoteOperatorRemovalConfirmed" .) $identityCompletion)) }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.remoteOperatorRollbackGuardName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-rollback
    "helm.sh/hook-weight": "-100"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded,hook-failed
spec:
  backoffLimit: 0
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      restartPolicy: Never
      containers:
        - name: rollback-guard
          image: "{{ dig "image" "repository" "alpine/k8s" (dig "cleanup" "onUninstall" dict .Values.runtime) }}:{{ dig "image" "tag" "1.32.0" (dig "cleanup" "onUninstall" dict .Values.runtime) }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" (dig "cleanup" "onUninstall" dict .Values.runtime) }}
          command:
            - /bin/sh
            - -ec
            - |
              identity_completion={{ include "deployment.remoteOperatorIdentityCompletionName" . | quote }}
              identity_completion_resource="$(kubectl -n {{ .Release.Namespace | quote }} get configmap "$identity_completion" --ignore-not-found=true --output=name)"
              if [ -n "$identity_completion_resource" ]; then
                echo "Refusing rollback: the Remote Operator identity is complete, and this revision would disable it. To remove only the Remote Operator, upgrade with --set remoteOperator.enabled=false --set-string remoteOperator.confirmRemoval={{ .Release.Name }}" >&2
                exit 1
              fi
{{- end }}
"#
    .to_string()
}

fn remote_operator_removal_notes_tpl() -> String {
    r#"{{- if include "deployment.remoteOperatorRemovalConfirmed" . }}
{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . }}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . }}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . }}
Remote Operator was removed from release {{ .Release.Name }}. The other workloads in this release are unchanged.

Kept in namespace {{ .Release.Namespace }} so the Remote Operator can be restored or its registration retired:
  - PersistentVolumeClaim {{ $identityRecordName }}-identity (the Remote Operator identity)
  - ConfigMaps {{ $identityRecordName }}, {{ $identityInitializedName }}, {{ $identityCompletionName }} (identity records)
{{- with .Values.remoteOperator.existingSecret.name }}
  - Secret {{ . }} (credentials created by setup; this chart does not manage it)
{{- end }}

Keep remoteOperator.confirmRemoval={{ .Release.Name }} on later upgrades while the Remote Operator stays removed.

To restore the Remote Operator with the same identity, upgrade with:
  --set remoteOperator.enabled=true --set-string remoteOperator.confirmRemoval=
and the same remoteOperator.existingSecret values.

To delete the kept identity permanently (restoring then requires the first-time setup again):
  kubectl --namespace {{ .Release.Namespace }} delete persistentvolumeclaim {{ $identityRecordName }}-identity
  kubectl --namespace {{ .Release.Namespace }} delete configmap {{ $identityRecordName }} {{ $identityInitializedName }} {{ $identityCompletionName }}
Uninstalling the release also deletes them.
{{- end }}
"#
    .to_string()
}

fn remote_operator_values() -> &'static str {
    r#"
remoteOperator:
  enabled: false
  # Must match Helm's Kubernetes history backend. Set to configmap when
  # HELM_DRIVER=configmap. The SQL backend is unsupported.
  helmHistoryBackend: secret
  # Created by the one-time setup flow, not by Helm. It must contain
  # sync-token and encryption-key.
  existingSecret:
    name: ""
    # Lowercase SHA-256 of the decoded encryption-key. Setup records this
    # non-secret fingerprint so token rotation cannot replace the identity.
    encryptionKeySha256: ""
  # Set true only for the first upgrade that enables Remote Operator after the
  # required disabled install/upgrade, then immediately persist false.
  bootstrapIdentity: false
  # Set to this release's name together with enabled: false to remove the
  # Remote Operator from an existing release. Its identity volume and records
  # are kept. Keep it set while the Operator stays removed; clear it to enable.
  confirmRemoval: ""
  syncTokenRevision: 0
  # Rollout marker for the independently rotatable collector token. Setup
  # tooling should set this to a digest or revision that changes with the token.
  collectorTokenRevision: ""
  serviceAccountAnnotations: {}
  podLabels: {}
"#
}

fn remote_operator_values_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "enabled",
            "helmHistoryBackend",
            "existingSecret",
            "bootstrapIdentity",
            "syncTokenRevision",
            "collectorTokenRevision",
            "serviceAccountAnnotations",
            "podLabels"
        ],
        "properties": {
            "enabled": { "type": "boolean" },
            "helmHistoryBackend": {
                "type": "string",
                "enum": ["secret", "configmap"]
            },
            "bootstrapIdentity": { "type": "boolean" },
            "confirmRemoval": { "type": "string" },
            "existingSecret": {
                "type": "object",
                "additionalProperties": false,
                "required": ["name", "encryptionKeySha256"],
                "properties": {
                    "name": {
                        "type": "string",
                        "maxLength": 253
                    },
                    "encryptionKeySha256": {
                        "type": "string",
                        "maxLength": 64
                    }
                }
            },
            "syncTokenRevision": { "type": "integer", "minimum": 0 },
            "collectorTokenRevision": {
                "type": "string",
                "maxLength": 64,
                "pattern": "^$|^[0-9a-f]{64}$"
            },
            "serviceAccountAnnotations": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "podLabels": {
                "type": "object",
                "propertyNames": {
                    "not": {
                        "enum": ["app.kubernetes.io/name", "app.kubernetes.io/instance"]
                    }
                },
                "additionalProperties": { "type": "string" }
            }
        },
        "allOf": [{
            "if": {
                "properties": { "enabled": { "const": true } },
                "required": ["enabled"]
            },
            "then": {
                "properties": {
                    "existingSecret": {
                        "properties": {
                            "name": {
                                "minLength": 1,
                                "pattern": "^[a-z0-9]([-.a-z0-9]*[a-z0-9])?$"
                            },
                            "encryptionKeySha256": {
                                "minLength": 64,
                                "pattern": "^[0-9a-f]{64}$"
                            }
                        }
                    }
                }
            }
        }]
    })
}

fn remote_operator_checks_tpl(requires_collector_token: bool) -> String {
    let collector_check = if requires_collector_token {
        r#"{{- $collectorToken := "" -}}
{{- if hasKey $credentials.data "collector-token" -}}
  {{- $collectorToken = index $credentials.data "collector-token" | b64dec -}}
{{- end -}}
{{- if empty $collectorToken -}}
  {{- fail "The Remote Operator credentials Secret must contain a non-empty collector-token when log collection is enabled." -}}
{{- end -}}
"#
    } else {
        ""
    };
    r#"{{- $secretName := include "deployment.remoteOperatorCredentialsSecretName" . | trim -}}
{{- $expectedEncryptionKeySha256 := include "deployment.remoteOperatorEncryptionKeySha256" . | trim -}}
{{- $confirmRemoval := default "" .Values.remoteOperator.confirmRemoval | toString -}}
{{- if and .Values.remoteOperator.enabled $confirmRemoval -}}
  {{- fail "remoteOperator.confirmRemoval must be empty when Remote Operator is enabled. Set remoteOperator.confirmRemoval to an empty string to restore the Remote Operator." -}}
{{- end -}}
{{- if and $confirmRemoval (ne $confirmRemoval .Release.Name) -}}
  {{- fail (printf "remoteOperator.confirmRemoval is %q, but this release is %q. Set it to the exact release name to remove the Remote Operator." $confirmRemoval .Release.Name) -}}
{{- end -}}
{{- if and .Release.IsInstall .Values.remoteOperator.enabled -}}
  {{- fail "Remote Operator cannot be enabled on the initial Helm install. Install once with remoteOperator.enabled=false so Helm records a rollback-guarded Kubernetes history revision, then enable it in an upgrade with remoteOperator.bootstrapIdentity=true." -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled -}}
{{- $cleanupName := include "deployment.remoteOperatorCleanupName" . -}}
{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . -}}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . -}}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . -}}
{{- $lifecycleCapabilityName := include "deployment.remoteOperatorLifecycleCapabilityName" . -}}
{{- $cleanupServiceAccount := lookup "v1" "ServiceAccount" .Release.Namespace $cleanupName -}}
{{- $cleanupRole := lookup "rbac.authorization.k8s.io/v1" "Role" .Release.Namespace $cleanupName -}}
{{- $cleanupRoleBinding := lookup "rbac.authorization.k8s.io/v1" "RoleBinding" .Release.Namespace $cleanupName -}}
{{- if not (and $cleanupServiceAccount $cleanupRole $cleanupRoleBinding) -}}
  {{- fail "Remote Operator cleanup authority is absent or does not match this exact Helm release. Perform one successful disabled bridge upgrade before enabling Remote Operator." -}}
{{- end -}}
{{- $cleanupAuthority := dict "valid" true -}}
{{- range $resource := list $cleanupServiceAccount $cleanupRole $cleanupRoleBinding -}}
  {{- $annotations := default dict $resource.metadata.annotations -}}
  {{- $labels := default dict $resource.metadata.labels -}}
  {{- if or (ne $resource.metadata.name $cleanupName) (ne $resource.metadata.namespace $.Release.Namespace) (ne (index $annotations "meta.helm.sh/release-name") $.Release.Name) (ne (index $annotations "meta.helm.sh/release-namespace") $.Release.Namespace) (ne (index $labels "app.kubernetes.io/managed-by") $.Release.Service) (ne (index $labels "app.kubernetes.io/instance") $.Release.Name) (hasKey $annotations "helm.sh/hook") -}}
    {{- $_ := set $cleanupAuthority "valid" false -}}
  {{- end -}}
{{- end -}}
{{- $expectedCleanupRoleRules := list
  (dict "apiGroups" (list "") "resources" (list "configmaps") "resourceNames" (list $identityRecordName $identityInitializedName $identityCompletionName $lifecycleCapabilityName) "verbs" (list "get" "delete"))
  (dict "apiGroups" (list "") "resources" (list "persistentvolumeclaims") "resourceNames" (list (printf "%s-identity" $identityRecordName)) "verbs" (list "get" "delete"))
  (dict "apiGroups" (list "apps") "resources" (list "deployments") "resourceNames" (list $identityRecordName) "verbs" (list "get" "delete"))
-}}
{{- if or (not (get $cleanupAuthority "valid")) (ne (toJson (default list $cleanupRole.rules)) (toJson $expectedCleanupRoleRules)) -}}
  {{- fail "Remote Operator cleanup authority is absent or does not match this exact Helm release. Perform one successful disabled bridge upgrade before enabling Remote Operator." -}}
{{- end -}}
{{- $cleanupSubjects := default list $cleanupRoleBinding.subjects -}}
{{- if ne (len $cleanupSubjects) 1 -}}
  {{- fail "Remote Operator cleanup authority is absent or does not match this exact Helm release. Perform one successful disabled bridge upgrade before enabling Remote Operator." -}}
{{- end -}}
{{- $cleanupSubject := index $cleanupSubjects 0 -}}
{{- $cleanupRoleRef := default dict $cleanupRoleBinding.roleRef -}}
{{- if or (ne $cleanupSubject.kind "ServiceAccount") (ne $cleanupSubject.name $cleanupName) (ne $cleanupSubject.namespace .Release.Namespace) (ne $cleanupRoleRef.apiGroup "rbac.authorization.k8s.io") (ne $cleanupRoleRef.kind "Role") (ne $cleanupRoleRef.name $cleanupName) -}}
  {{- fail "Remote Operator cleanup authority is absent or does not match this exact Helm release. Perform one successful disabled bridge upgrade before enabling Remote Operator." -}}
{{- end -}}
{{- end -}}
{{- if .Values.remoteOperator.enabled -}}
{{- if not .Values.management.url -}}
  {{- fail "management.url is required when Remote Operator is enabled." -}}
{{- end -}}
{{- $expected := include "deployment.remoteOperatorAccessRequestCrd" . | fromYaml -}}
{{- $existing := lookup "apiextensions.k8s.io/v1" "CustomResourceDefinition" "" $expected.metadata.name -}}
{{- if $existing -}}
  {{- range $spec := list $expected.spec $existing.spec -}}
    {{- $_ := set $spec.names "listKind" (default (printf "%sList" $spec.names.kind) $spec.names.listKind) -}}
    {{- $_ := set $spec "conversion" (default (dict "strategy" "None") $spec.conversion) -}}
    {{- $_ := set $spec "preserveUnknownFields" (default false $spec.preserveUnknownFields) -}}
  {{- end -}}
  {{- if ne (toJson $expected.spec) (toJson $existing.spec) -}}
    {{- fail "The shared access-request CRD differs from this reviewed chart. Ask the cluster administrator to review compatibility before changing it; this release will not adopt, upgrade, or delete the CRD." -}}
  {{- end -}}
{{- end -}}
{{- $secretName = required "remoteOperator.existingSecret.name is required when Remote Operator is enabled" $secretName -}}
{{- $expectedEncryptionKeySha256 = required "remoteOperator.existingSecret.encryptionKeySha256 is required when Remote Operator is enabled" $expectedEncryptionKeySha256 -}}
{{- $credentials := lookup "v1" "Secret" .Release.Namespace $secretName -}}
{{- if not $credentials -}}
  {{- fail (printf "Remote Operator credentials Secret %s/%s is missing. Create it through the setup flow before installing this release." .Release.Namespace $secretName) -}}
{{- end -}}
{{- if not (and (hasKey $credentials.data "sync-token") (hasKey $credentials.data "encryption-key")) -}}
  {{- fail "The Remote Operator credentials Secret must contain sync-token and encryption-key." -}}
{{- end -}}
{{- $syncToken := index $credentials.data "sync-token" | b64dec -}}
{{- $encryptionKey := index $credentials.data "encryption-key" | b64dec -}}
{{- if or (empty $syncToken) (empty $encryptionKey) -}}
  {{- fail "The Remote Operator credentials Secret must contain non-empty sync-token and encryption-key values." -}}
{{- end -}}
{{- $actualEncryptionKeySha256 := $encryptionKey | sha256sum -}}
{{- if ne $actualEncryptionKeySha256 $expectedEncryptionKeySha256 -}}
  {{- fail (printf "Remote Operator credentials Secret %s/%s has encryption-key SHA-256 %s, but setup recorded %s. Refusing identity replacement; restore the original encryption-key." .Release.Namespace $secretName $actualEncryptionKeySha256 $expectedEncryptionKeySha256) -}}
{{- end -}}
__COLLECTOR_CHECK__{{- end -}}
{{- if or .Release.IsInstall .Release.IsUpgrade -}}
{{- $identityRecordName := include "deployment.remoteOperatorIdentityRecordName" . -}}
{{- $identityInitializedName := include "deployment.remoteOperatorIdentityInitializedName" . -}}
{{- $identityCompletionName := include "deployment.remoteOperatorIdentityCompletionName" . -}}
{{- $lifecycleCapabilityName := include "deployment.remoteOperatorLifecycleCapabilityName" . -}}
{{- $identityRecord := lookup "v1" "ConfigMap" .Release.Namespace $identityRecordName -}}
{{- $identityInitialized := lookup "v1" "ConfigMap" .Release.Namespace $identityInitializedName -}}
{{- $identityCompletion := lookup "v1" "ConfigMap" .Release.Namespace $identityCompletionName -}}
{{- $lifecycleCapability := lookup "v1" "ConfigMap" .Release.Namespace $lifecycleCapabilityName -}}
{{- if and .Release.IsInstall (or $identityRecord $identityInitialized $identityCompletion $lifecycleCapability) -}}
  {{- fail "Retained Remote Operator lifecycle records already exist for this release name. Refusing reinstall; complete the explicit cleanup lifecycle before reusing the name." -}}
{{- end -}}
{{- if $lifecycleCapability -}}
  {{- $capabilityAnnotations := default dict $lifecycleCapability.metadata.annotations -}}
  {{- $capabilityLabels := default dict $lifecycleCapability.metadata.labels -}}
  {{- $capabilityData := default dict $lifecycleCapability.data -}}
  {{- if or (ne (index $capabilityAnnotations "meta.helm.sh/release-name") .Release.Name) (ne (index $capabilityAnnotations "meta.helm.sh/release-namespace") .Release.Namespace) (ne (index $capabilityLabels "app.kubernetes.io/managed-by") .Release.Service) (ne (index $capabilityLabels "app.kubernetes.io/instance") .Release.Name) (ne (index $capabilityLabels "alien.dev/remote-operator-lifecycle-capability") "v2") (ne (index $capabilityLabels "alien.dev/remote-operator-release-id") (include "deployment.remoteOperatorReleaseIdentity" .)) (not (default false $lifecycleCapability.immutable)) (ne (len $capabilityData) 2) (ne (index $capabilityData "version") "2") (not (hasKey $capabilityData "firstGuardRevision")) -}}
    {{- fail (printf "ConfigMap %s/%s does not match the immutable lifecycle-capability contract owned by this exact Helm release. Refusing adoption." .Release.Namespace $lifecycleCapabilityName) -}}
  {{- end -}}
  {{- $firstGuardRevision := int (index $capabilityData "firstGuardRevision") -}}
  {{- if or (lt $firstGuardRevision 1) (gt $firstGuardRevision (int .Release.Revision)) -}}
    {{- fail (printf "ConfigMap %s/%s records invalid first guard revision %d. Refusing adoption." .Release.Namespace $lifecycleCapabilityName $firstGuardRevision) -}}
  {{- end -}}
  {{- if and .Release.IsUpgrade .Values.remoteOperator.enabled -}}
    {{- $historyState := dict "records" 0 -}}
    {{- $historyStore := dict -}}
    {{- if eq .Values.remoteOperator.helmHistoryBackend "secret" -}}
      {{- $historyStore = lookup "v1" "Secret" .Release.Namespace "" -}}
    {{- else -}}
      {{- $historyStore = lookup "v1" "ConfigMap" .Release.Namespace "" -}}
    {{- end -}}
    {{- range $historyRecord := default (list) (get $historyStore "items") -}}
      {{- $historyLabels := default dict $historyRecord.metadata.labels -}}
      {{- if and (eq (default "" (index $historyLabels "owner")) "helm") (eq (default "" (index $historyLabels "name")) $.Release.Name) -}}
        {{- $_ := set $historyState "records" (add1 (int (get $historyState "records"))) -}}
        {{- $historyRevision := int (default "0" (index $historyLabels "version")) -}}
        {{- if lt $historyRevision $firstGuardRevision -}}
          {{- fail (printf "Helm revision %d predates the Remote Operator rollback guard introduced at revision %d. Remove every older stored revision (for example, perform a disabled bridge upgrade with --history-max 1) before enabling Remote Operator." $historyRevision $firstGuardRevision) -}}
        {{- end -}}
      {{- end -}}
    {{- end -}}
    {{- if eq (int (get $historyState "records")) 0 -}}
      {{- fail (printf "Remote Operator lifecycle protection found no records for this release in the configured %s Helm history backend. Ensure remoteOperator.helmHistoryBackend matches HELM_DRIVER. The Helm SQL storage backend is unsupported; migrate to a Kubernetes storage backend and prune every pre-guard revision before enabling." .Values.remoteOperator.helmHistoryBackend) -}}
    {{- end -}}
  {{- end -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled (not $lifecycleCapability) -}}
  {{- fail "This release predates the Remote Operator rollback guard. Upgrade once with remoteOperator.enabled=false before first enable so the previous revision can reject an unsafe rollback." -}}
{{- end -}}
{{- if $identityRecord -}}
  {{- $recordAnnotations := default dict $identityRecord.metadata.annotations -}}
  {{- $recordLabels := default dict $identityRecord.metadata.labels -}}
  {{- $recordData := default dict $identityRecord.data -}}
  {{- if or (ne (index $recordAnnotations "meta.helm.sh/release-name") .Release.Name) (ne (index $recordAnnotations "meta.helm.sh/release-namespace") .Release.Namespace) (ne (index $recordAnnotations "helm.sh/hook") "pre-install,pre-upgrade") (ne (index $recordAnnotations "helm.sh/hook-weight") "-100") (ne (index $recordAnnotations "helm.sh/resource-policy") "keep") (ne (index $recordLabels "app.kubernetes.io/managed-by") .Release.Service) (ne (index $recordLabels "app.kubernetes.io/instance") .Release.Name) (ne (index $recordLabels "alien.dev/remote-operator-identity-record") "true") (ne (index $recordLabels "alien.dev/remote-operator-identity-phase") "prepared") (ne (index $recordLabels "alien.dev/remote-operator-release-id") (include "deployment.remoteOperatorReleaseIdentity" .)) (not (default false $identityRecord.immutable)) (ne (len $recordData) 3) (ne (index $recordData "version") "3") (not (hasKey $recordData "credentialsSecretName")) (not (hasKey $recordData "encryptionKeySha256")) -}}
    {{- fail (printf "ConfigMap %s/%s does not match the immutable prepared identity-record contract owned by this exact Helm release. Refusing adoption." .Release.Namespace $identityRecordName) -}}
  {{- end -}}
  {{- if .Values.remoteOperator.enabled -}}
    {{- $recordedSecretName := default "" (index $recordData "credentialsSecretName") -}}
    {{- $recordedEncryptionKeySha256 := default "" (index $recordData "encryptionKeySha256") -}}
    {{- if ne $recordedSecretName $secretName -}}
      {{- fail (printf "Remote Operator identity record %s/%s pins credentials Secret %s, not %s. Refusing identity replacement; sync-token rotation must keep the original Secret name." .Release.Namespace $identityRecordName $recordedSecretName $secretName) -}}
    {{- end -}}
    {{- if ne $recordedEncryptionKeySha256 $expectedEncryptionKeySha256 -}}
      {{- fail (printf "Remote Operator identity record %s/%s pins encryption-key SHA-256 %s, not %s. Refusing identity replacement; sync-token rotation must keep the original encryption-key." .Release.Namespace $identityRecordName $recordedEncryptionKeySha256 $expectedEncryptionKeySha256) -}}
    {{- end -}}
  {{- end -}}
{{- end -}}
{{- if $identityInitialized -}}
  {{- $initializedAnnotations := default dict $identityInitialized.metadata.annotations -}}
  {{- $initializedLabels := default dict $identityInitialized.metadata.labels -}}
  {{- $initializedData := default dict $identityInitialized.data -}}
  {{- $initializedPhase := default "" (index $initializedLabels "alien.dev/remote-operator-identity-phase") -}}
  {{- $initializedImmutable := default false $identityInitialized.immutable -}}
  {{- $initializedPhaseValid := or (and (eq $initializedPhase "pending") (not $initializedImmutable)) (and (eq $initializedPhase "initialized") $initializedImmutable) -}}
  {{- if or (ne (index $initializedAnnotations "meta.helm.sh/release-name") .Release.Name) (ne (index $initializedAnnotations "meta.helm.sh/release-namespace") .Release.Namespace) (ne (index $initializedAnnotations "helm.sh/resource-policy") "keep") (ne (index $initializedLabels "app.kubernetes.io/managed-by") .Release.Service) (ne (index $initializedLabels "app.kubernetes.io/instance") .Release.Name) (not $initializedPhaseValid) (ne (index $initializedLabels "alien.dev/remote-operator-release-id") (include "deployment.remoteOperatorReleaseIdentity" .)) (ne (len $initializedData) 2) (ne (index $initializedData "version") "1") (not (hasKey $initializedData "identityRecordName")) -}}
    {{- fail (printf "ConfigMap %s/%s does not match the pending-or-initialized identity contract owned by this exact Helm release. Refusing adoption." .Release.Namespace $identityInitializedName) -}}
  {{- end -}}
  {{- if not $identityRecord -}}
    {{- fail (printf "Remote Operator initialization record %s/%s exists without identity record %s. Restore the retained identity record before retrying." .Release.Namespace $identityInitializedName $identityRecordName) -}}
  {{- end -}}
  {{- if ne (default "" (index $initializedData "identityRecordName")) $identityRecordName -}}
    {{- fail (printf "Remote Operator initialization record %s/%s does not reference identity record %s. Refusing adoption." .Release.Namespace $identityInitializedName $identityRecordName) -}}
  {{- end -}}
{{- end -}}
{{- if $identityCompletion -}}
  {{- if .Release.IsInstall -}}
    {{- fail (printf "ConfigMap %s/%s records a completed Remote Operator identity from an earlier release. Refusing reinstall even when stale Helm ownership metadata names this release." .Release.Namespace $identityCompletionName) -}}
  {{- end -}}
  {{- $completionAnnotations := default dict $identityCompletion.metadata.annotations -}}
  {{- $completionLabels := default dict $identityCompletion.metadata.labels -}}
  {{- $completionData := default dict $identityCompletion.data -}}
  {{- if or (ne (index $completionAnnotations "meta.helm.sh/release-name") .Release.Name) (ne (index $completionAnnotations "meta.helm.sh/release-namespace") .Release.Namespace) (ne (index $completionAnnotations "helm.sh/hook") "post-install,post-upgrade") (ne (index $completionAnnotations "helm.sh/hook-weight") "100") (ne (index $completionAnnotations "helm.sh/resource-policy") "keep") (ne (index $completionLabels "app.kubernetes.io/managed-by") .Release.Service) (ne (index $completionLabels "app.kubernetes.io/instance") .Release.Name) (ne (index $completionLabels "alien.dev/remote-operator-identity-phase") "complete") (ne (index $completionLabels "alien.dev/remote-operator-release-id") (include "deployment.remoteOperatorReleaseIdentity" .)) (not (default false $identityCompletion.immutable)) (ne (len $completionData) 2) (ne (index $completionData "version") "1") (not (hasKey $completionData "identityRecordName")) -}}
    {{- fail (printf "ConfigMap %s/%s does not match the immutable completion-record contract owned by this exact Helm release. Refusing adoption." .Release.Namespace $identityCompletionName) -}}
  {{- end -}}
  {{- if not $identityRecord -}}
    {{- fail (printf "Remote Operator completion record %s/%s exists without identity record %s. Restore the retained identity record before retrying." .Release.Namespace $identityCompletionName $identityRecordName) -}}
  {{- end -}}
  {{- if not $identityInitialized -}}
    {{- fail (printf "Remote Operator completion record %s/%s exists without initialization record %s. Restore the retained initialization record before retrying." .Release.Namespace $identityCompletionName $identityInitializedName) -}}
  {{- end -}}
  {{- if ne (default "" (index (default dict $identityInitialized.metadata.labels) "alien.dev/remote-operator-identity-phase")) "initialized" -}}
    {{- fail (printf "Remote Operator completion record %s/%s requires a successfully initialized identity record %s." .Release.Namespace $identityCompletionName $identityInitializedName) -}}
  {{- end -}}
  {{- if ne (default "" (index $completionData "identityRecordName")) $identityRecordName -}}
    {{- fail (printf "Remote Operator completion record %s/%s does not reference identity record %s. Refusing adoption." .Release.Namespace $identityCompletionName $identityRecordName) -}}
  {{- end -}}
{{- end -}}
{{- $preparedIdentity := and $identityRecord (not $identityCompletion) -}}
{{- $preparedRetry := and .Values.remoteOperator.enabled $preparedIdentity -}}
{{- $identityState := dict "managedResourceExists" false "otherManagedResourceExists" false "identityMissing" false -}}
{{- if or .Values.remoteOperator.enabled $identityRecord $identityInitialized $identityCompletion -}}
{{- range $document := splitList "\n---\n" (include "deployment.remoteOperatorResources" .) -}}
  {{- $resource := fromYaml $document -}}
  {{- if and $resource $resource.kind $resource.metadata.name -}}
    {{- $lookupNamespace := default "" $resource.metadata.namespace -}}
    {{- $current := lookup $resource.apiVersion $resource.kind $lookupNamespace $resource.metadata.name -}}
    {{- if $current -}}
      {{- if or $.Values.remoteOperator.enabled $.Release.IsUpgrade -}}
      {{- $annotations := default dict $current.metadata.annotations -}}
      {{- $labels := default dict $current.metadata.labels -}}
      {{- if or (ne (index $annotations "meta.helm.sh/release-name") $.Release.Name) (ne (index $annotations "meta.helm.sh/release-namespace") $.Release.Namespace) (ne (index $labels "app.kubernetes.io/managed-by") $.Release.Service) -}}
        {{- fail (printf "%s %s/%s is not owned by this exact Helm release. Refusing adoption." $resource.kind $lookupNamespace $resource.metadata.name) -}}
      {{- end -}}
      {{- $_ := set $identityState "managedResourceExists" true -}}
      {{- if ne $resource.kind "PersistentVolumeClaim" -}}
        {{- $_ := set $identityState "otherManagedResourceExists" true -}}
      {{- end -}}
      {{- end -}}
    {{- else if and $.Values.remoteOperator.enabled (eq $resource.kind "PersistentVolumeClaim") -}}
      {{- $_ := set $identityState "identityMissing" true -}}
    {{- end -}}
  {{- end -}}
{{- end -}}
{{- end -}}

{{- if and .Release.IsInstall .Values.remoteOperator.enabled (get $identityState "managedResourceExists") (not $preparedRetry) -}}
  {{- fail "Remote Operator managed resources already exist before install. Refusing adoption without an exact prepared identity retry." -}}
{{- end -}}
{{- if and $preparedRetry (get $identityState "otherManagedResourceExists") -}}
  {{- fail "A prepared Remote Operator retry may reuse only its exact-release owned retained identity PVC. Refusing adoption of another managed resource." -}}
{{- end -}}
{{- $safePreparedRollback := and (not .Values.remoteOperator.enabled) $preparedIdentity -}}
{{- if and .Release.IsUpgrade (not .Values.remoteOperator.enabled) (or $identityRecord $identityInitialized $identityCompletion (get $identityState "managedResourceExists")) (not $safePreparedRollback) (not (include "deployment.remoteOperatorRemovalConfirmed" .)) -}}
  {{- fail (printf "Disabling Remote Operator on an existing release deletes its workload and permissions. To remove only the Remote Operator and keep its identity volume, upgrade with --set remoteOperator.enabled=false --set-string remoteOperator.confirmRemoval=%s" .Release.Name) -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled (get $identityState "managedResourceExists") (not $identityRecord) -}}
  {{- fail "Remote Operator managed resources exist without the retained identity record. Refusing adoption; restore the original identity record before retrying." -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled .Values.remoteOperator.bootstrapIdentity $identityCompletion -}}
  {{- fail "remoteOperator.bootstrapIdentity has already been consumed by this release. Set it to false; the retained completion record prevents replacement after total workload deletion." -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled (get $identityState "identityMissing") (or $identityInitialized $identityCompletion) -}}
  {{- fail "The Remote Operator identity volume is missing from a partial installation. Restore it; an upgrade must not bootstrap a replacement identity." -}}
{{- end -}}
{{- if and .Release.IsUpgrade .Values.remoteOperator.enabled (get $identityState "identityMissing") (not $preparedRetry) (not .Values.remoteOperator.bootstrapIdentity) -}}
  {{- fail "The Remote Operator identity volume is missing. Use the explicit first-enable bootstrap flow; an ordinary upgrade must not create a replacement identity." -}}
{{- end -}}
{{- end -}}
"#
    .replace("__COLLECTOR_CHECK__", collector_check)
}

pub fn generate_operator_manifest(options: OperatorManifestOptions<'_>) -> Result<String> {
    generate_operator_manifest_inner(options, None, None, None, None, None, None)
}

/// Generate a standalone Operator manifest carrying an exact image receipt.
pub fn generate_operator_manifest_with_image_identity(
    options: OperatorManifestOptions<'_>,
    image_identity: OperatorImageIdentityOptions<'_>,
) -> Result<String> {
    generate_operator_manifest_inner(options, None, None, None, None, None, Some(image_identity))
}

/// Render a Remote Operator for inclusion in a product Helm chart.
pub fn generate_product_operator_manifest(
    options: ProductOperatorManifestOptions<'_>,
) -> Result<String> {
    generate_product_operator_manifest_with_identity_marker(options, None, None, None)
}

/// Render a product Remote Operator manifest carrying an exact image receipt.
pub fn generate_product_operator_manifest_with_image_identity(
    options: ProductOperatorManifestOptions<'_>,
    image_identity: OperatorImageIdentityOptions<'_>,
) -> Result<String> {
    generate_product_operator_manifest_with_identity_marker(
        options,
        None,
        None,
        Some(image_identity),
    )
}

fn generate_product_operator_manifest_with_identity_marker(
    options: ProductOperatorManifestOptions<'_>,
    identity_initialized_config_map: Option<&str>,
    log_collector_name: Option<&str>,
    image_identity: Option<OperatorImageIdentityOptions<'_>>,
) -> Result<String> {
    generate_operator_manifest_inner(
        options.manifest,
        Some(options.credentials_secret_name),
        Some(options.credentials_encryption_key_sha256),
        options.resource_name,
        identity_initialized_config_map,
        log_collector_name,
        image_identity,
    )
}

fn generate_operator_manifest_inner(
    options: OperatorManifestOptions<'_>,
    credentials_secret_name: Option<&str>,
    credentials_encryption_key_sha256: Option<&str>,
    resource_name: Option<&str>,
    identity_initialized_config_map: Option<&str>,
    log_collector_name: Option<&str>,
    image_identity: Option<OperatorImageIdentityOptions<'_>>,
) -> Result<String> {
    if options.format == OperatorOutputFormat::RawManifest && credentials_secret_name.is_none() {
        validate_runtime_encryption_key(options.encryption_key)?;
    }
    validate_operator_options(&options)?;
    validate_product_operator_options(
        credentials_secret_name,
        credentials_encryption_key_sha256,
        resource_name,
    )?;
    let operator_image_report = image_identity
        .map(|identity| build_operator_image_report(options.image, identity))
        .transpose()?;

    let stack_settings_json = options
        .stack_settings
        .filter(|settings| **settings != StackSettings::default())
        .map(to_stable_json)
        .transpose()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize operator stack settings".to_string(),
        })?;

    let base_name = sanitize_chart_name(options.project_name);
    let operator_name = resource_name
        .map(str::to_string)
        .unwrap_or_else(|| format!("{base_name}-operator"));
    let identity_pvc_name = format!("{operator_name}-identity");
    let log_collector_name = log_collector_name
        .map(str::to_string)
        .unwrap_or_else(|| format!("{operator_name}-whitelabeled-log-collector"));
    // Product charts pin an Operator image built with the readiness server.
    // The standalone generator accepts arbitrary released images, including
    // versions that predate that endpoint, so it must not require the probe.
    let supports_readiness = credentials_secret_name.is_some();
    let creates_credentials_secret = credentials_secret_name.is_none();
    let credentials_secret_name = credentials_secret_name.unwrap_or(operator_name.as_str());

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
        OperatorOutputFormat::HelmTemplate => "{{ .Release.Name }}".to_string(),
        OperatorOutputFormat::RawManifest => options
            .environment_name
            .expect("validated by validate_operator_options")
            .to_string(),
    };

    let label_instance = resource_name.unwrap_or(base_name.as_str());
    let labels = operator_labels(label_instance, options.format);
    let cluster_wide = options.scope.is_cluster_wide();

    // White-labeled access-request CRD names, derived from the branded domain.
    // The operator carries the same domain at runtime, so both agree on the CRD.
    let crd_names = alien_core::access_request_crd::access_request_crd_names(options.label_domain);

    let mut docs = Vec::new();
    // The access-request CRD is cluster-scoped and must exist before any CR is
    // created. Register it up front, regardless of namespace/cluster scope.
    docs.push(access_request_crd_doc(&crd_names));
    docs.push(operator_service_account_doc(
        namespace,
        &operator_name,
        &labels,
        options.format,
    ));
    // Cluster-wide (label) scope needs cluster-scoped read RBAC; namespace scope
    // stays a namespaced Role. Both grant read-only + the access-request CRD.
    if cluster_wide {
        docs.push(operator_clusterrole_doc(
            &operator_name,
            &labels,
            &crd_names,
            options.kubernetes_operations_enabled,
            options.permission,
            options.custom_operation_permissions,
        ));
        docs.push(operator_clusterrolebinding_doc(
            namespace,
            &operator_name,
            &labels,
        ));
    } else {
        docs.push(operator_role_doc(
            namespace,
            &operator_name,
            &labels,
            &crd_names,
            options.kubernetes_operations_enabled,
            options.permission,
            options.custom_operation_permissions,
        ));
        docs.push(operator_rolebinding_doc(namespace, &operator_name, &labels));
    }
    if creates_credentials_secret {
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
    }
    docs.push(operator_identity_pvc_doc(
        namespace,
        &identity_pvc_name,
        &labels,
        resource_name.is_some(),
    ));
    docs.push(operator_deployment_doc(
        namespace,
        &operator_name,
        &identity_pvc_name,
        credentials_secret_name,
        supports_readiness,
        identity_initialized_config_map,
        &options,
        namespace,
        &environment_name_expr,
        options.label_selector,
        &labels,
        stack_settings_json.as_deref(),
        operator_image_report.as_ref(),
    ));
    if let Some(log_collector) = options.log_collector.as_ref() {
        // Product charts manage workloads under the chart's runtime scope, not
        // under the separately named Remote Operator Deployment.
        let default_collector_scope = if identity_initialized_config_map.is_some()
            && options.format == OperatorOutputFormat::HelmTemplate
        {
            (
                "{{ .Values.logCollector.scope.deploymentLabelKey }}".to_string(),
                "{{ default (include \"deployment.fullname\" .) .Values.logCollector.scope.deploymentLabelValue | regexQuoteMeta }}".to_string(),
            )
        } else {
            (
                branded_tag_key(
                    alien_core::access_request_crd::current_kubernetes_label_domain(
                        options
                            .label_domain
                            .unwrap_or(alien_core::DEFAULT_ALIEN_LABEL_DOMAIN),
                    ),
                    ALIEN_STACK_TAG_KEY,
                ),
                operator_name.clone(),
            )
        };
        let mut collector_labels = labels.clone();
        collector_labels.insert(
            "app.kubernetes.io/component".to_string(),
            "whitelabeled-log-collector".to_string(),
        );
        collector_labels.insert(
            "alien.dev/log-collector-exclude".to_string(),
            "true".to_string(),
        );
        docs.push(operator_service_doc(namespace, &operator_name, &labels));
        docs.push(operator_log_collector_service_account_doc(
            namespace,
            &log_collector_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_doc(
            namespace,
            &log_collector_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_role_binding_doc(
            namespace,
            &log_collector_name,
            &collector_labels,
        ));
        docs.push(operator_log_collector_configmap_doc(
            namespace,
            &operator_name,
            &log_collector_name,
            namespace,
            &collector_labels,
            log_collector,
            (&default_collector_scope.0, &default_collector_scope.1),
            options.format == OperatorOutputFormat::HelmTemplate,
        ));
        docs.push(operator_log_collector_daemonset_doc(
            namespace,
            &log_collector_name,
            credentials_secret_name,
            log_collector.image,
            &collector_labels,
            options.format == OperatorOutputFormat::HelmTemplate,
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

fn build_operator_image_report(
    image: &str,
    identity: OperatorImageIdentityOptions<'_>,
) -> Result<OperatorImageReport> {
    let digest = image
        .rsplit_once('@')
        .map(|(_, digest)| digest.to_string())
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "operator image identity requires repository@sha256:digest form"
                    .to_string(),
            })
        })?;
    let (source, package_id, package_version) = match identity {
        OperatorImageIdentityOptions::Configured => (OperatorImageSource::Configured, None, None),
        OperatorImageIdentityOptions::Package {
            package_id,
            package_version,
        } => (
            OperatorImageSource::Package,
            Some(package_id.to_string()),
            Some(package_version.to_string()),
        ),
    };
    let report = OperatorImageReport {
        source,
        package_id,
        package_version,
        image: image.to_string(),
        digest,
    };
    report.validate().map_err(|message| {
        AlienError::new(ErrorData::GenericError {
            message: message.to_string(),
        })
    })?;
    Ok(report)
}

fn validate_operator_options(options: &OperatorManifestOptions<'_>) -> Result<()> {
    for operation in options.custom_operation_permissions {
        operation.validate().context(ErrorData::GenericError {
            message: format!(
                "invalid Kubernetes permissions for {}/{}",
                operation.plugin, operation.operation
            ),
        })?;
    }
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

    if let Some(collector) = &options.log_collector {
        match (collector.pod_label_key, collector.pod_label_value) {
            (None, None) => {}
            (Some(key), Some(value))
                if valid_kubernetes_pod_label_key(key) && valid_kubernetes_label_name(value) => {}
            _ => {
                return invalid(
                    "log collector Pod label key and value must be set together and be valid Kubernetes labels",
                );
            }
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

fn valid_kubernetes_label_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn valid_kubernetes_pod_label_key(value: &str) -> bool {
    match value.split_once('/') {
        Some((domain, name)) => {
            alien_core::access_request_crd::is_valid_kubernetes_label_domain(domain)
                && valid_kubernetes_label_name(name)
        }
        None => valid_kubernetes_label_name(value),
    }
}

fn validate_product_operator_options(
    credentials_secret_name: Option<&str>,
    credentials_encryption_key_sha256: Option<&str>,
    resource_name: Option<&str>,
) -> Result<()> {
    let invalid = |message: &str| {
        Err(AlienError::new(ErrorData::GenericError {
            message: message.to_string(),
        }))
    };
    if credentials_secret_name.is_some_and(|name| name.trim().is_empty()) {
        return invalid("operator credentials Secret name must not be empty");
    }
    if credentials_encryption_key_sha256.is_some_and(|fingerprint| fingerprint.trim().is_empty()) {
        return invalid("operator credentials encryption-key SHA-256 must not be empty");
    }
    if credentials_secret_name.is_some() != credentials_encryption_key_sha256.is_some() {
        return invalid(
            "operator product credentials require both a Secret name and encryption-key SHA-256",
        );
    }
    if resource_name.is_some_and(|name| name.trim().is_empty()) {
        return invalid("operator resource name must not be empty");
    }
    Ok(())
}

fn operator_labels(instance_name: &str, format: OperatorOutputFormat) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), "operator".to_string()),
        (
            "app.kubernetes.io/instance".to_string(),
            instance_name.to_string(),
        ),
        (
            "app.kubernetes.io/component".to_string(),
            "operator".to_string(),
        ),
        (
            "app.kubernetes.io/managed-by".to_string(),
            match format {
                OperatorOutputFormat::RawManifest => "kubectl",
                OperatorOutputFormat::HelmTemplate => "{{ .Release.Service }}",
            }
            .to_string(),
        ),
    ])
}

fn operator_service_account_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
    format: OperatorOutputFormat,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, operator_name, labels);
    if format == OperatorOutputFormat::HelmTemplate {
        yaml.push_str("  {{- with .Values.remoteOperator.serviceAccountAnnotations }}\n");
        yaml.push_str("  annotations:\n");
        yaml.push_str("    {{- toYaml . | nindent 4 }}\n");
        yaml.push_str("  {{- end }}\n");
    }
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_role_doc(
    namespace: &str,
    operator_name: &str,
    labels: &BTreeMap<String, String>,
    crd_names: &AccessRequestCrdNames,
    kubernetes_operations_enabled: bool,
    permission: OperatorPermission,
    custom_operations: &[KubernetesOperationPermissions],
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        operator_name,
        labels,
    );
    yaml.push_str(&operator_rules(
        crd_names,
        kubernetes_operations_enabled,
        permission,
        custom_operations,
    ));
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

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct OperatorRuleScope {
    api_group: String,
    resource: String,
    resource_names: BTreeSet<String>,
}

#[derive(Default)]
struct OperatorRuleGrant {
    verbs: BTreeSet<String>,
    reasons: BTreeSet<String>,
}

/// Kubernetes operation rules shared by the namespaced `Role` and cluster-wide
/// `ClusterRole`.
///
/// Base access is read-only inventory (`get/list/watch`; never `secrets`). The
/// Kubernetes operations plugin adds `pods/log` access. When that plugin is
/// enabled and the permission ceiling is `Remediation`, it additionally grants
/// exactly what the initial mutating operations require:
///   - `pods` `delete` — `restart-pod` (the controller reschedules the pod)
///   - workload `scale` `patch` — `scale` (the `scale` subresource)
///
/// No operation-specific rule is emitted when the plugin is disabled.
///
/// Plus the access-request custom resource, which the operator creates
/// (materializing a control-plane access request the customer must authorize)
/// and updates the `status` of (recording the approval window read back from
/// the customer). The operator never executes commands from the CR — the
/// customer's approval is reported back to the control plane, which dispatches
/// the commands through the normal commands queue.
///
/// The access-request `apiGroups`/`resources` are white-labeled from `names` so
/// a vendor build grants access to *their* CRD, matching the CRD doc below.
fn operator_rules(
    names: &AccessRequestCrdNames,
    kubernetes_operations_enabled: bool,
    permission: OperatorPermission,
    custom_operations: &[KubernetesOperationPermissions],
) -> String {
    // Normalize builtin and custom grants together. Names are part of the key:
    // sharing a grant never widens a named requirement to all resources.
    let mut rules = BTreeMap::<OperatorRuleScope, OperatorRuleGrant>::new();
    let mut add_rule = |group: &str, resource: &str, resource_names, verbs, reason| {
        let grant = rules
            .entry(OperatorRuleScope {
                api_group: group.to_owned(),
                resource: resource.to_owned(),
                resource_names,
            })
            .or_default();
        grant.verbs.extend(verbs);
        grant.reasons.insert(reason);
    };
    let status_resource = format!("{}/status", names.plural);
    let baseline: &[(&str, &[&str], &[&str], &str)] = &[
        (
            "",
            &[
                "pods",
                "services",
                "configmaps",
                "persistentvolumeclaims",
                "events",
                "endpoints",
            ],
            &["get", "list", "watch"],
            "baseline resource inventory.",
        ),
        (
            "apps",
            &["deployments", "statefulsets", "daemonsets", "replicasets"],
            &["get", "list", "watch"],
            "baseline workload inventory.",
        ),
        (
            "batch",
            &["jobs", "cronjobs"],
            &["get", "list", "watch"],
            "baseline job inventory.",
        ),
        (
            "metrics.k8s.io",
            &["pods"],
            &["get", "list", "watch"],
            "baseline pod metrics.",
        ),
        (
            &names.group,
            &[&names.plural],
            &["get", "list", "watch", "create", "update", "patch"],
            "access-request materialization and approval observation.",
        ),
        (
            &names.group,
            &[&status_resource],
            &["get", "update", "patch"],
            "access-request status reporting.",
        ),
    ];
    for (group, resources, verbs, reason) in baseline {
        for resource in *resources {
            add_rule(
                group,
                resource,
                BTreeSet::new(),
                verbs
                    .iter()
                    .map(|verb| (*verb).to_owned())
                    .collect::<BTreeSet<_>>(),
                (*reason).to_owned(),
            );
        }
    }
    if kubernetes_operations_enabled {
        add_rule(
            "",
            "pods/log",
            BTreeSet::new(),
            BTreeSet::from(["get".to_owned()]),
            "the kubernetes/logs operation.".to_owned(),
        );
    }
    if kubernetes_operations_enabled && permission == OperatorPermission::Remediation {
        add_rule(
            "",
            "pods",
            BTreeSet::new(),
            BTreeSet::from(["delete".to_owned()]),
            "the kubernetes/restart-pod operation.".to_owned(),
        );
        for resource in [
            "deployments/scale",
            "statefulsets/scale",
            "replicasets/scale",
        ] {
            add_rule(
                "apps",
                resource,
                BTreeSet::new(),
                BTreeSet::from(["patch".to_owned()]),
                "the kubernetes/scale operation.".to_owned(),
            );
        }
    }
    for operation in custom_operations {
        for rule in &operation.permissions.rules {
            let verbs: BTreeSet<_> = rule
                .verbs
                .iter()
                .filter(|verb| {
                    permission == OperatorPermission::Remediation
                        || matches!(verb.as_str(), "get" | "list" | "watch")
                })
                .cloned()
                .collect();
            if !verbs.is_empty() {
                add_rule(
                    &rule.api_group,
                    &rule.resource,
                    rule.resource_names.iter().cloned().collect(),
                    verbs,
                    format!(
                        "{}/{}: {}",
                        operation.plugin, operation.operation, rule.reason
                    ),
                );
            }
        }
    }
    let mut yaml = "rules:\n".to_owned();
    for (scope, grant) in rules {
        for reason in grant.reasons {
            yaml.push_str(&format!("  # Required by {reason}\n"));
        }
        yaml.push_str(&format!(
            "  - apiGroups: [{}]\n    resources: [{}]\n    verbs: [{}]\n",
            yaml_string(&scope.api_group),
            yaml_string(&scope.resource),
            grant
                .verbs
                .iter()
                .map(|verb| yaml_string(verb))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !scope.resource_names.is_empty() {
            yaml.push_str(&format!(
                "    resourceNames: [{}]\n",
                scope
                    .resource_names
                    .iter()
                    .map(|name| yaml_string(name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    yaml
}

/// The access-request `CustomResourceDefinition`, white-labeled from `names`.
///
/// One CR is a **time-boxed grant**: a control-plane access request (usually a
/// remediation plan) proposing a set of commands the customer must authorize.
/// The customer reviews the commands and approves by patching a duration —
/// `kubectl patch … --type=merge -p '{"spec":{"approvedForMinutes":360}}'` — to
/// authorize the grant for that many minutes. The operator records the resulting
/// window in `status.approvedUntil` and reports the approval back to the control
/// plane, which then dispatches the commands one by one through the normal
/// commands queue. The operator does **not** execute commands from the CR.
///
/// Printer columns show STATE/COMMANDS/APPROVED-UNTIL/AGE.
///
/// Appended to the operator manifest so `kubectl apply`/Helm registers the kind
/// before any CR is created.
fn access_request_crd_doc(names: &AccessRequestCrdNames) -> String {
    format!(
        r#"apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: {crd_name}
spec:
  group: {group}
  scope: Namespaced
  names:
    plural: {plural}
    singular: {singular}
    kind: {kind}
    shortNames: ["{short_name}"]
  versions:
    - name: {version}
      served: true
      storage: true
      subresources:
        status: {{}}
      additionalPrinterColumns:
        - name: State
          type: string
          jsonPath: .status.state
        - name: Commands
          type: integer
          jsonPath: .status.commandCount
        - name: Approved-Until
          type: date
          jsonPath: .status.approvedUntil
        - name: Age
          type: date
          jsonPath: .metadata.creationTimestamp
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              required: ["requestId", "commands"]
              properties:
                requestId:
                  type: string
                  description: Control-plane access-request id this CR mirrors.
                title:
                  type: string
                  description: Human-readable title (e.g. the remediation plan title).
                reason:
                  type: string
                  description: Why access is being requested.
                commands:
                  type: array
                  description: The commands this grant covers.
                  items:
                    type: object
                    required: ["command"]
                    properties:
                      command:
                        type: string
                        description: Command name, <plugin>/<operation>.
                      summary:
                        type: string
                        description: One-line human summary for display.
                      params:
                        type: object
                        x-kubernetes-preserve-unknown-fields: true
                        description: Exact operation parameters covered by the grant.
                approvedForMinutes:
                  type: integer
                  minimum: 1
                  description: >-
                    Set by the customer to approve the grant for this many
                    minutes. The operator computes status.approvedUntil from it.
            status:
              type: object
              properties:
                state:
                  type: string
                  description: PENDING_APPROVAL | APPROVED | EXPIRED
                commandCount:
                  type: integer
                  description: Number of commands this grant covers.
                approvedAt:
                  type: string
                  description: When the customer approved (RFC3339).
                approvedUntil:
                  type: string
                  description: >-
                    Instant the grant is authorized until (RFC3339), computed as
                    approvedAt + approvedForMinutes. After this the control plane
                    must not dispatch the grant's commands.
"#,
        crd_name = names.crd_name,
        group = names.group,
        plural = names.plural,
        singular = names.singular,
        kind = names.kind,
        short_name = names.short_name,
        version = alien_core::access_request_crd::ACCESS_REQUEST_CRD_VERSION,
    )
}

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

fn operator_clusterrole_doc(
    operator_name: &str,
    labels: &BTreeMap<String, String>,
    crd_names: &AccessRequestCrdNames,
    kubernetes_operations_enabled: bool,
    permission: OperatorPermission,
    custom_operations: &[KubernetesOperationPermissions],
) -> String {
    let mut yaml = operator_cluster_metadata_doc("ClusterRole", operator_name, labels);
    yaml.push_str(&operator_rules(
        crd_names,
        kubernetes_operations_enabled,
        permission,
        custom_operations,
    ));
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
    retain_on_helm_uninstall: bool,
) -> String {
    let mut yaml =
        operator_metadata_doc("v1", "PersistentVolumeClaim", namespace, pvc_name, labels);
    if retain_on_helm_uninstall {
        yaml.push_str("  annotations:\n");
        yaml.push_str("    helm.sh/resource-policy: keep\n");
    }
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
    credentials_secret_name: &str,
    supports_readiness: bool,
    identity_initialized_config_map: Option<&str>,
    options: &OperatorManifestOptions<'_>,
    observed_namespace: &str,
    environment_name: &str,
    label_selector: Option<&str>,
    labels: &BTreeMap<String, String>,
    stack_settings_json: Option<&str>,
    operator_image_report: Option<&OperatorImageReport>,
) -> String {
    let mut yaml = operator_metadata_doc("apps/v1", "Deployment", namespace, operator_name, labels);
    yaml.push_str("spec:\n");
    yaml.push_str("  replicas: 1\n");
    // Only one process can hold the persisted identity lock at a time.
    yaml.push_str("  strategy:\n    type: Recreate\n    rollingUpdate: null\n");
    yaml.push_str("  selector:\n");
    yaml.push_str("    matchLabels:\n");
    append_operator_selector_labels(&mut yaml, labels, 6);
    yaml.push_str("  template:\n");
    yaml.push_str("    metadata:\n");
    yaml.push_str("      labels:\n");
    append_operator_labels(&mut yaml, labels, 8);
    if options.log_collector.is_some() {
        yaml.push_str("        alien.dev/log-collector-exclude: 'true'\n");
    }
    if options.format == OperatorOutputFormat::HelmTemplate {
        yaml.push_str("        {{- with .Values.remoteOperator.podLabels }}\n");
        yaml.push_str("        {{- toYaml . | nindent 8 }}\n");
        yaml.push_str("        {{- end }}\n");
    }
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
    if let Some(report) = operator_image_report {
        append_env_value(
            &mut yaml,
            "ALIEN_OPERATOR_IMAGE_SOURCE",
            match report.source {
                OperatorImageSource::Package => "package",
                OperatorImageSource::Configured => "configured",
            },
        );
        append_env_value(&mut yaml, "ALIEN_OPERATOR_IMAGE_RECEIPT", &report.image);
        append_env_value(&mut yaml, "ALIEN_OPERATOR_IMAGE_DIGEST", &report.digest);
        if let Some(package_id) = report.package_id.as_deref() {
            append_env_value(&mut yaml, "ALIEN_OPERATOR_IMAGE_PACKAGE_ID", package_id);
        }
        if let Some(package_version) = report.package_version.as_deref() {
            append_env_value(
                &mut yaml,
                "ALIEN_OPERATOR_IMAGE_PACKAGE_VERSION",
                package_version,
            );
        }
    }
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
    // Use the host chart's standard appVersion so the pasted template does not
    // require a vendor-specific values key. Raw manifests omit it.
    if options.format == OperatorOutputFormat::HelmTemplate {
        append_env_value(
            &mut yaml,
            "OPERATOR_RELEASE_VERSION",
            "{{ .Chart.AppVersion }}",
        );
    }
    append_env_value(
        &mut yaml,
        "OPERATOR_PERMISSION",
        options.permission.as_str(),
    );
    append_env_value(&mut yaml, "OPERATOR_INITIAL_DESIRED_RELEASE", "none");
    append_env_value(&mut yaml, "OPERATOR_SETUP_METHOD", "manual");
    append_env_value(&mut yaml, "DATA_DIR", "/var/lib/operator");
    if supports_readiness {
        append_env_value(&mut yaml, "OPERATOR_READINESS_PORT", "8081");
        if let Some(config_map_name) = identity_initialized_config_map {
            append_env_value(
                &mut yaml,
                "OPERATOR_IDENTITY_INITIALIZED_CONFIGMAP",
                config_map_name,
            );
        }
    }
    if options.log_collector.is_some() {
        append_env_value(&mut yaml, "OTLP_HOST", "0.0.0.0");
        append_env_value(&mut yaml, "OTLP_PORT", "8080");
        append_env_value(
            &mut yaml,
            "COLLECTOR_TOKEN_FILE",
            "/etc/operator/secrets/collector-token",
        );
    }
    if let Some(stack_settings_json) = stack_settings_json {
        append_env_value(&mut yaml, "STACK_SETTINGS", stack_settings_json);
    }
    append_env_value(
        &mut yaml,
        "SYNC_TOKEN_FILE",
        "/etc/operator/secrets/sync-token",
    );
    if options.format == OperatorOutputFormat::HelmTemplate {
        yaml.push_str("            - name: SYNC_TOKEN_REVISION\n");
        yaml.push_str("              value: {{ default 0 .Values.remoteOperator.syncTokenRevision | quote }}\n");
    }
    append_env_value(
        &mut yaml,
        "OPERATOR_ENCRYPTION_KEY_FILE",
        "/etc/operator/secrets/encryption-key",
    );
    append_env_value(&mut yaml, "SYNC_INTERVAL", "30");
    if supports_readiness || options.log_collector.is_some() {
        yaml.push_str("          ports:\n");
    }
    if supports_readiness {
        yaml.push_str("            - name: readiness\n");
        yaml.push_str("              containerPort: 8081\n");
    }
    if options.log_collector.is_some() {
        yaml.push_str("            - name: http\n");
        yaml.push_str("              containerPort: 8080\n");
    }
    if supports_readiness {
        yaml.push_str("          readinessProbe:\n");
        yaml.push_str("            httpGet:\n");
        yaml.push_str("              path: /ready\n");
        yaml.push_str("              port: readiness\n");
        yaml.push_str("            periodSeconds: 2\n");
        yaml.push_str("            failureThreshold: 150\n");
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
        yaml_string(credentials_secret_name)
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
    // The collector shares the release labels but cannot receive Operator logs.
    yaml.push_str("    app.kubernetes.io/component: operator\n");
    yaml.push_str("  ports:\n");
    yaml.push_str("    - name: http\n");
    yaml.push_str("      port: 8080\n");
    yaml.push_str("      targetPort: http\n");
    yaml
}

fn operator_log_collector_service_account_doc(
    namespace: &str,
    collector_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "ServiceAccount", namespace, collector_name, labels);
    yaml.push_str("automountServiceAccountToken: true\n");
    yaml
}

fn operator_log_collector_role_doc(
    namespace: &str,
    collector_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "Role",
        namespace,
        collector_name,
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
    collector_name: &str,
    labels: &BTreeMap<String, String>,
) -> String {
    let mut yaml = operator_metadata_doc(
        "rbac.authorization.k8s.io/v1",
        "RoleBinding",
        namespace,
        collector_name,
        labels,
    );
    yaml.push_str("roleRef:\n");
    yaml.push_str("  apiGroup: rbac.authorization.k8s.io\n");
    yaml.push_str("  kind: Role\n");
    yaml.push_str(&format!("  name: {}\n", yaml_string(collector_name)));
    yaml.push_str("subjects:\n");
    yaml.push_str("  - kind: ServiceAccount\n");
    yaml.push_str(&format!("    name: {}\n", yaml_string(collector_name)));
    yaml.push_str(&format!("    namespace: {}\n", yaml_string(namespace)));
    yaml
}

fn operator_log_collector_configmap_doc(
    namespace: &str,
    operator_name: &str,
    collector_name: &str,
    observed_namespace: &str,
    labels: &BTreeMap<String, String>,
    collector: &OperatorLogCollectorOptions<'_>,
    default_scope: (&str, &str),
    helm_template: bool,
) -> String {
    let mut yaml = operator_metadata_doc("v1", "ConfigMap", namespace, collector_name, labels);
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
    // The Kubernetes filter's default tag parser reads the symlink filename in
    // /var/log/containers. Files under /var/log/pods cannot supply its metadata.
    yaml.push_str(&format!(
        "        Path              /var/log/containers/*_{}_*.log\n",
        observed_namespace
    ));
    yaml.push_str("        Path_Key          filename\n");
    // Built-in multiline parsers auto-detect the runtime log format: `cri` for
    // containerd (EKS/GKE/AKS, k8s >=1.24) and `docker` for the docker-json format
    // used by Docker-runtime clusters (Docker Desktop, OrbStack, legacy on-prem).
    yaml.push_str("        multiline.parser  docker, cri\n");
    yaml.push_str("        Tag               kube.*\n");
    yaml.push_str(&format!(
        "        DB                /buffers/{collector_name}.db\n"
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
    yaml.push_str("    [FILTER]\n");
    yaml.push_str("        Name                grep\n");
    yaml.push_str("        Match               kube.*\n");
    yaml.push_str(
        "        Exclude             $kubernetes['labels']['alien.dev/log-collector-exclude'] ^true$\n\n",
    );
    let label_key = collector.pod_label_key.unwrap_or(default_scope.0);
    let label_value = collector.pod_label_value.unwrap_or(default_scope.1);
    let label_pattern = if helm_template && collector.pod_label_value.is_none() {
        label_value.to_string()
    } else {
        label_value.replace('.', "\\.")
    };
    yaml.push_str("    [FILTER]\n");
    yaml.push_str("        Name                grep\n");
    yaml.push_str("        Match               kube.*\n");
    yaml.push_str(&format!(
        "        Regex               $kubernetes['labels']['{label_key}'] ^{label_pattern}$\n\n"
    ));
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
    collector_name: &str,
    credentials_secret_name: &str,
    image: &str,
    labels: &BTreeMap<String, String>,
    include_credential_revision: bool,
) -> String {
    let mut yaml = operator_metadata_doc("apps/v1", "DaemonSet", namespace, collector_name, labels);
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
        yaml_string(collector_name)
    ));
    yaml.push_str("      tolerations:\n");
    yaml.push_str("        - operator: Exists\n");
    yaml.push_str("      securityContext:\n");
    yaml.push_str("        seccompProfile:\n");
    yaml.push_str("          type: RuntimeDefault\n");
    yaml.push_str("      containers:\n");
    yaml.push_str("        - name: collector\n");
    yaml.push_str(&format!("          image: {}\n", yaml_string(image)));
    yaml.push_str("          imagePullPolicy: IfNotPresent\n");
    yaml.push_str("          securityContext:\n");
    yaml.push_str("            allowPrivilegeEscalation: false\n");
    yaml.push_str("            readOnlyRootFilesystem: true\n");
    yaml.push_str("            capabilities:\n");
    yaml.push_str("              drop: [ALL]\n");
    yaml.push_str("          args: [\"-c\", \"/collector/etc/collector.conf\"]\n");
    yaml.push_str("          env:\n");
    yaml.push_str("            - name: COLLECTOR_TOKEN\n");
    yaml.push_str("              valueFrom:\n");
    yaml.push_str("                secretKeyRef:\n");
    yaml.push_str(&format!(
        "                  name: {}\n",
        yaml_string(credentials_secret_name)
    ));
    yaml.push_str("                  key: collector-token\n");
    if include_credential_revision {
        yaml.push_str("            - name: COLLECTOR_TOKEN_REVISION\n");
        yaml.push_str(
            "              value: {{ default \"\" .Values.remoteOperator.collectorTokenRevision | quote }}\n",
        );
    }
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
    yaml.push_str(&format!(
        "            name: {}\n",
        yaml_string(collector_name)
    ));
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
                targets_kubernetes: true,
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

fn to_stable_json<T: Serialize>(value: &T) -> alien_error::Result<String> {
    let value = serde_json::to_value(value).into_alien_error()?;
    serde_json::to_string(&sort_json_object_keys(value)).into_alien_error()
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

inputValues: {}

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
      helmHistoryBackend: secret
      image:
        repository: alpine/k8s
        tag: "1.32.0"
        pullPolicy: IfNotPresent

logCollector:
  enabled: false
  # Sends selected workload Pod logs. Requires read-only node-log hostPath mounts;
  # namespaces enforcing Baseline or Restricted Pod Security reject those mounts.
  # Generated on first install and retained on upgrade when empty.
  token: ""
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
    legacyDeploymentLabelKey: ""
    deploymentLabelValue: ""
    # For an observed workload that lacks the deployment label, set both fields.
    podLabelKey: ""
    podLabelValue: ""

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

fn values_schema_json(stack: &Stack) -> Result<String> {
    let base = r##"{
  "$schema": "https://json-schema.org/draft-07/schema#",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "nameOverride": { "type": "string" },
    "fullnameOverride": { "type": "string" },
    "inputValues": {
      "type": "object",
      "additionalProperties": {
        "anyOf": [
          { "type": "string" },
          { "type": "number" },
          { "type": "boolean" },
          { "type": "array", "items": { "type": "string" } }
        ]
      }
    },
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
                "helmHistoryBackend": { "type": "string", "enum": ["secret", "configmap"] },
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
            "deploymentLabelKey": {
              "type": "string",
              "minLength": 1,
              "maxLength": 264,
              "pattern": "^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/deployment$"
            },
            "legacyDeploymentLabelKey": {
              "type": "string",
              "maxLength": 264,
              "pattern": "^$|^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/deployment$"
            },
            "deploymentLabelValue": { "type": "string" },
            "podLabelKey": {
              "type": "string",
              "maxLength": 317,
              "pattern": "^$|^([a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/)?[A-Za-z0-9]([-A-Za-z0-9_.]{0,61}[A-Za-z0-9])?$"
            },
            "podLabelValue": {
              "type": "string",
              "maxLength": 63,
              "pattern": "^$|^[A-Za-z0-9]([-A-Za-z0-9_.]{0,61}[A-Za-z0-9])?$"
            }
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
    "serviceAccountPrefix": {
      "type": "string",
      "maxLength": 63,
      "pattern": "^$|^[A-Za-z0-9]([-A-Za-z0-9_.]*[A-Za-z0-9])?$"
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
      "title": "initialize path",
      "required": ["management"],
      "properties": {
        "management": {
          "properties": {
            "deploymentId": { "type": "null" }
          }
        },
        "stackSettings": { "type": ["object", "null"] },
        "infrastructure": { "type": ["object", "null"] }
      }
    }
  ]
}
"##;

    let deployer_inputs = stack
        .inputs()
        .iter()
        .filter(|input| {
            input
                .provided_by
                .contains(&alien_core::StackInputProvider::Deployer)
        })
        .collect::<Vec<_>>();
    if deployer_inputs.is_empty() {
        return Ok(base.to_string());
    }

    let mut schema: serde_json::Value = serde_json::from_str(base).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "failed to parse built-in Helm values schema".to_string(),
        },
    )?;
    let input_schema = &mut schema["properties"]["inputValues"];
    input_schema["additionalProperties"] = serde_json::Value::Bool(false);
    let mut properties = serde_json::Map::new();
    for input in deployer_inputs {
        use alien_core::StackInputKind;
        let kind = match input.kind {
            StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => "string",
            StackInputKind::Number => "number",
            StackInputKind::Integer => "integer",
            StackInputKind::Boolean => "boolean",
            StackInputKind::StringList => "array",
        };
        let mut field = serde_json::json!({ "type": kind });
        if matches!(input.kind, StackInputKind::StringList) {
            field["items"] = serde_json::json!({ "type": "string" });
        }
        if let Some(validation) = &input.validation {
            if let Some(min_length) = validation.min_length {
                field["minLength"] = serde_json::json!(min_length);
            }
            if let Some(max_length) = validation.max_length {
                field["maxLength"] = serde_json::json!(max_length);
            }
            if let Some(pattern) = &validation.pattern {
                field["pattern"] = serde_json::json!(pattern);
            }
            if let Some(values) = &validation.values {
                field["enum"] = serde_json::json!(values);
            }
            if let Some(min_items) = validation.min_items {
                field["minItems"] = serde_json::json!(min_items);
            }
            if let Some(max_items) = validation.max_items {
                field["maxItems"] = serde_json::json!(max_items);
            }
            if matches!(input.kind, StackInputKind::Number | StackInputKind::Integer) {
                for (bound, key) in [
                    (validation.min.as_deref(), "minimum"),
                    (validation.max.as_deref(), "maximum"),
                ] {
                    if let Some(bound) = bound {
                        field[key] =
                            serde_json::Value::Number(bound.parse().into_alien_error().context(
                                ErrorData::JsonSerializationFailed {
                                    reason: format!(
                                        "invalid numeric bound for input '{}'",
                                        input.id
                                    ),
                                },
                            )?);
                    }
                }
            }
            if validation.format.as_deref() == Some("url") {
                field["format"] = serde_json::json!("uri");
            }
        }
        properties.insert(input.id.clone(), field);
    }
    input_schema["properties"] = serde_json::Value::Object(properties);
    serde_json::to_string_pretty(&schema)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "failed to serialize Helm input values schema".to_string(),
        })
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

{{- define "deployment.releaseScopedName" -}}
{{- $suffix := .suffix -}}
{{- $identity := printf "%s/%s" .root.Release.Namespace .root.Release.Name -}}
{{- $hash := sha256sum $identity | trunc 8 -}}
{{- $maxBaseLength := sub 54 (len $suffix) -}}
{{- $rawBase := regexReplaceAll "[^a-z0-9-]" (lower .root.Release.Name) "-" | trimAll "-" -}}
{{- $base := default "release" $rawBase | trunc (int $maxBaseLength) | trimSuffix "-" -}}
{{- printf "%s-%s%s" $base $hash $suffix -}}
{{- end -}}

{{- define "deployment.logCollectorName" -}}
{{- printf "%s-logs" ((include "deployment.fullname" .) | trunc 58 | trimSuffix "-") -}}
{{- end -}}

{{- define "deployment.labels" -}}
app.kubernetes.io/name: {{ include "deployment.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "deployment.managerServiceAccountName" -}}
{{- $prefix := include "deployment.serviceAccountPrefix" . -}}
{{- $raw := printf "%s-manager-sa" $prefix | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.serviceAccountPrefix" -}}
{{- $source := default (include "deployment.fullname" .) .Values.serviceAccountPrefix -}}
{{- $normalized := regexReplaceAll "-+" (regexReplaceAll "[^a-z0-9-]" (lower $source) "-") "-" | trimAll "-" -}}
{{- if not (regexMatch "^[a-z]" $normalized) -}}
{{- $normalized = printf "r-%s" $normalized -}}
{{- end -}}
{{- if or (ne $source $normalized) (gt (len $normalized) 40) -}}
{{- printf "%s-%s" ($normalized | trunc 31 | trimSuffix "-") (sha256sum $source | trunc 8) -}}
{{- else -}}
{{- $normalized -}}
{{- end -}}
{{- end -}}

{{- define "deployment.serviceAccountName" -}}
{{- $prefix := include "deployment.serviceAccountPrefix" .root -}}
{{- $raw := printf "%s-%s-sa" $prefix .name | lower -}}
{{- regexReplaceAll "[^a-z0-9-]" $raw "-" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "deployment.resourceName" -}}
{{- $raw := .name | lower -}}
{{- $normalized := regexReplaceAll "[^a-z0-9-]" $raw "-" -}}
{{- $collapsed := regexReplaceAll "-+" $normalized "-" | trimAll "-" -}}
{{- default "alien" $collapsed | trunc 63 | trimSuffix "-" -}}
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
    r#"{{- $runtimeLabelKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}
{{- $runtimeLabelValue := default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue -}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "deployment.managerServiceAccountName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    {{ $runtimeLabelKey }}: {{ $runtimeLabelValue | quote }}
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
    {{ $runtimeLabelKey }}: {{ $runtimeLabelValue | quote }}
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
    r#"apiVersion: rbac.authorization.k8s.io/v1
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
    resources: ["serviceaccounts"]
    verbs: ["get"]
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
  - apiGroups: ["networking.gke.io"]
    resources: ["healthcheckpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
  - apiGroups: ["alb.networking.azure.io"]
    resources: ["healthcheckpolicies"]
    verbs: ["get", "list", "watch", "create", "update", "patch", "delete"]
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
{{- if or $createManagementSecret $createEncryptionSecret .Values.infrastructure .Values.logCollector.enabled .Values.inputValues }}
{{- $collectorToken := .Values.logCollector.token -}}
{{- if and .Values.logCollector.enabled (empty $collectorToken) -}}
  {{- $existing := lookup "v1" "Secret" .Release.Namespace (include "deployment.fullname" .) -}}
  {{- if and $existing (hasKey (default dict $existing.data) "collector-token") -}}
    {{- $collectorToken = index $existing.data "collector-token" | b64dec -}}
    {{- if empty $collectorToken -}}{{ fail "Existing release Secret has an empty collector-token" }}{{- end -}}
  {{- else -}}
    {{- $collectorToken = randAlphaNum 48 -}}
  {{- end -}}
{{- end -}}
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
  {{- if .Values.logCollector.enabled }}
  collector-token: {{ $collectorToken | quote }}
  {{- end }}
  {{- if .Values.inputValues }}
  input-values.json: {{ toJson .Values.inputValues | quote }}
  {{- end }}
{{- end }}
"#
    .to_string()
}

fn configmap_tpl() -> String {
    r#"{{- $defaultStackSettings := dict "deploymentModel" "pull" "updates" .Values.management.updates "telemetry" .Values.management.telemetry "heartbeats" .Values.management.healthChecks -}}
{{- $stackSettings := deepCopy (default $defaultStackSettings .Values.stackSettings) -}}
{{- $kubernetes := deepCopy (default dict (get $stackSettings "kubernetes")) -}}
{{- $cluster := deepCopy (default dict (get $kubernetes "cluster")) -}}
{{- $requestedNamespace := default .Release.Namespace (get $cluster "namespace") -}}
{{- if ne $requestedNamespace .Release.Namespace -}}
  {{- fail "The Kubernetes workload namespace must match the Helm release namespace so the agent can use its chart-owned Secret and ServiceAccount." -}}
{{- end -}}
{{- $_ := set $cluster "namespace" .Release.Namespace -}}
{{- if not (get $cluster "ownership") -}}
  {{- $_ := set $cluster "ownership" "external" -}}
{{- end -}}
{{- $_ := set $kubernetes "cluster" $cluster -}}
{{- $_ := set $stackSettings "kubernetes" $kubernetes -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.fullname" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
data:
  stack.json: |-
{{ .Files.Get "files/stack.json" | indent 4 }}
  stack-settings.json: {{ toJson $stackSettings | quote }}
  services.json: {{ toJson .Values.services | quote }}
  public-endpoints.json: {{ toJson (default dict .Values.publicEndpoints) | quote }}
"#
    .to_string()
}

fn runtime_cleanup_scope_tpl() -> String {
    r#"{{- $scopeName := include "deployment.releaseScopedName" (dict "root" . "suffix" "-runtime-scope") -}}
{{- $requestedLabelKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}
{{- $requestedLegacyLabelKey := default "" .Values.logCollector.scope.legacyDeploymentLabelKey -}}
{{- $requestedLabelValue := default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue -}}
{{- if not (hasSuffix "/deployment" $requestedLabelKey) -}}
  {{- fail "The runtime cleanup deployment label key must end in '/deployment' because runtime ownership labels are domain-qualified." -}}
{{- end -}}
{{- $requestedResourceLabelKey := printf "%s/resource" (trimSuffix "/deployment" $requestedLabelKey) -}}
{{- if or (gt (len $requestedLabelKey) 264) (not (regexMatch "^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/deployment$" $requestedLabelKey)) -}}
  {{- fail "The runtime cleanup deployment label key is not a valid Kubernetes label key." -}}
{{- end -}}
{{- if and $requestedLegacyLabelKey (or (eq $requestedLegacyLabelKey $requestedLabelKey) (gt (len $requestedLegacyLabelKey) 264) (not (regexMatch "^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/deployment$" $requestedLegacyLabelKey))) -}}
  {{- fail "The legacy runtime cleanup deployment label key must be empty or a distinct valid Kubernetes label key." -}}
{{- end -}}
{{- if or (gt (len $requestedLabelValue) 63) (not (regexMatch "^[A-Za-z0-9]([-A-Za-z0-9_.]*[A-Za-z0-9])?$" $requestedLabelValue)) -}}
  {{- fail "The runtime cleanup deployment label value is not a valid non-empty Kubernetes label value." -}}
{{- end -}}
{{- $existing := lookup "v1" "ConfigMap" .Release.Namespace $scopeName -}}
{{- $firstSafeRevision := .Release.Revision -}}
{{- if $existing -}}
  {{- $annotations := default dict $existing.metadata.annotations -}}
  {{- $labels := default dict $existing.metadata.labels -}}
  {{- $data := default dict $existing.data -}}
  {{- if or (ne (index $annotations "meta.helm.sh/release-name") .Release.Name) (ne (index $annotations "meta.helm.sh/release-namespace") .Release.Namespace) (ne (index $labels "app.kubernetes.io/managed-by") .Release.Service) (ne (index $labels "app.kubernetes.io/instance") .Release.Name) (ne (index $labels "alien.dev/runtime-cleanup-scope") "v2") (not (default false $existing.immutable)) (ne (len $data) 6) (ne (index $data "version") "2") (not (hasKey $data "labelKey")) (not (hasKey $data "legacyLabelKey")) (not (hasKey $data "labelValue")) (not (hasKey $data "resourceLabelKey")) (not (hasKey $data "firstSafeRevision")) -}}
    {{- fail (printf "ConfigMap %s/%s does not match the immutable runtime cleanup scope owned by this exact Helm release. Refusing adoption." .Release.Namespace $scopeName) -}}
  {{- end -}}
  {{- if or (ne (index $data "labelKey") $requestedLabelKey) (ne (index $data "legacyLabelKey") $requestedLegacyLabelKey) (ne (index $data "labelValue") $requestedLabelValue) (ne (index $data "resourceLabelKey") $requestedResourceLabelKey) -}}
    {{- fail (printf "Runtime cleanup scope is pinned to %s=%s; refusing to change it to %s=%s because uninstall cleanup authority must remain bound to the original deployment." (index $data "labelKey") (index $data "labelValue") $requestedLabelKey $requestedLabelValue) -}}
  {{- end -}}
  {{- $firstSafeRevision = index $data "firstSafeRevision" -}}
{{- end }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ $scopeName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    meta.helm.sh/release-name: {{ .Release.Name | quote }}
    meta.helm.sh/release-namespace: {{ .Release.Namespace | quote }}
  labels:
    app.kubernetes.io/managed-by: {{ .Release.Service | quote }}
    app.kubernetes.io/instance: {{ .Release.Name | quote }}
    alien.dev/runtime-cleanup-scope: "v2"
immutable: true
data:
  version: "2"
  labelKey: {{ $requestedLabelKey | quote }}
  legacyLabelKey: {{ $requestedLegacyLabelKey | quote }}
  labelValue: {{ $requestedLabelValue | quote }}
  resourceLabelKey: {{ $requestedResourceLabelKey | quote }}
  firstSafeRevision: {{ $firstSafeRevision | quote }}
"#
    .to_string()
}

fn runtime_cleanup_history_prune_tpl() -> String {
    r#"{{- $cleanup := dig "cleanup" "onUninstall" dict .Values.runtime -}}
{{- $stack := .Files.Get "files/stack.json" | fromJson -}}
{{- $runtimeRoots := list -}}
{{- $runtimeBuilds := list -}}
{{- range $entry := values (default dict $stack.resources) -}}
  {{- if has (default "" $entry.config.type) (list "container" "worker" "daemon") -}}
    {{- $runtimeRootName := include "deployment.resourceName" (dict "root" $ "name" $entry.config.id) -}}
    {{- $runtimeRoots = append $runtimeRoots (dict "name" $runtimeRootName "resourceId" $entry.config.id) -}}
  {{- end -}}
  {{- if eq (default "" $entry.config.type) "build" -}}
    {{- $runtimeBuildName := include "deployment.resourceName" (dict "root" $ "name" (printf "build-%s" $entry.config.id)) -}}
    {{- $runtimeBuilds = append $runtimeBuilds (dict "name" $runtimeBuildName "resourceId" $entry.config.id) -}}
  {{- end -}}
{{- end -}}
{{- if .Release.IsUpgrade -}}
{{- $backend := dig "helmHistoryBackend" "secret" $cleanup -}}
{{- $pruneName := include "deployment.releaseScopedName" (dict "root" . "suffix" "-history-prune") -}}
{{- $scopeName := include "deployment.releaseScopedName" (dict "root" . "suffix" "-runtime-scope") -}}
{{- $existingScope := lookup "v1" "ConfigMap" .Release.Namespace $scopeName -}}
{{- $firstSafeRevision := .Release.Revision -}}
{{- $migrationLabelKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}
{{- $migrationLegacyLabelKey := default "" .Values.logCollector.scope.legacyDeploymentLabelKey -}}
{{- $migrationLabelValue := default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue -}}
{{- if $existingScope -}}
  {{- $scopeData := default dict $existingScope.data -}}
  {{- $firstSafeRevision = index $scopeData "firstSafeRevision" -}}
  {{- $migrationLabelKey = index $scopeData "labelKey" -}}
  {{- $migrationLegacyLabelKey = index $scopeData "legacyLabelKey" -}}
  {{- $migrationLabelValue = index $scopeData "labelValue" -}}
{{- end -}}
{{- if and (gt (int $firstSafeRevision) 1) (gt (int .Release.Revision) (int $firstSafeRevision)) -}}
{{- $historyRecordName := printf "sh.helm.release.v1.%s.v%d" .Release.Name (.Release.Revision | int) -}}
{{- $historyProof := randAlphaNum 32 -}}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ $pruneName }}
  namespace: {{ .Release.Namespace }}
  annotations:
    "helm.sh/hook": post-upgrade
    "helm.sh/hook-weight": "124"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded,hook-failed
spec:
  backoffLimit: 0
  template:
    spec:
      serviceAccountName: {{ include "deployment.managerServiceAccountName" . }}
      restartPolicy: Never
      volumes:
        - name: current-history
          {{- if eq $backend "secret" }}
          secret:
            secretName: {{ $historyRecordName | quote }}
            optional: true
          {{- else }}
          configMap:
            name: {{ $historyRecordName | quote }}
            optional: true
          {{- end }}
      containers:
        - name: prune-unsafe-history
          image: "{{ dig "image" "repository" "alpine/k8s" $cleanup }}:{{ dig "image" "tag" "1.32.0" $cleanup }}"
          imagePullPolicy: {{ dig "image" "pullPolicy" "IfNotPresent" $cleanup }}
          command:
            - /bin/sh
            - -ec
            - |
              history_kind={{ $backend | quote }}
              namespace={{ .Release.Namespace | quote }}
              release_name={{ .Release.Name | quote }}
              first_safe_revision={{ $firstSafeRevision | quote }}
              history_proof={{ $historyProof | quote }}
              migration_label_key={{ $migrationLabelKey | quote }}
              migration_legacy_label_key={{ $migrationLegacyLabelKey | quote }}
              migration_label_value={{ $migrationLabelValue | quote }}
              migration_resource_label_key="${migration_label_key%/deployment}/resource"
              migration_legacy_resource_label_key=""
              if [ -n "$migration_legacy_label_key" ]; then
                migration_legacy_resource_label_key="${migration_legacy_label_key%/deployment}/resource"
              fi
              migration_label_go_template='{{ "{{" }} with index .metadata.labels "'$migration_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              migration_legacy_label_go_template='{{ "{{" }} with index .metadata.labels "'$migration_legacy_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              migration_resource_go_template='{{ "{{" }} with index .metadata.labels "'$migration_resource_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              migration_legacy_resource_go_template='{{ "{{" }} with index .metadata.labels "'$migration_legacy_resource_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              if [ "$first_safe_revision" -lt 1 ] || [ "$first_safe_revision" -gt {{ .Release.Revision }} ]; then
                echo "The runtime cleanup scope has an invalid first safe Helm revision; refusing history pruning." >&2
                exit 1
              fi
              if ! base64 -d /history/release | gzip -d | grep -F -q "$history_proof"; then
                echo "The configured Helm history backend does not contain this exact pending upgrade; refusing history pruning." >&2
                exit 1
              fi
              migration_complete=true
              if ! kubectl -n "$namespace" get deployments.apps,statefulsets.apps,daemonsets.apps,jobs.batch -l managed-by=runtime -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.metadata.labels}{"\n"}{end}' > /tmp/runtime-roots 2>/tmp/migration-error; then
                cat /tmp/migration-error >&2
                echo "Cannot list runtime workloads; refusing history pruning." >&2
                exit 1
              fi
              while IFS= read -r root_line; do
                [ -n "$root_line" ] || continue
                if ! printf '%s' "$root_line" | grep -F -q '/deployment:'; then
                  migration_complete=false
                fi
              done < /tmp/runtime-roots
              {{- range $runtimeRoot := $runtimeRoots }}
              migration_root={{ $runtimeRoot.name | quote }}
              migration_resource_id={{ $runtimeRoot.resourceId | quote }}
              for migration_kind in deployment.apps statefulset.apps daemonset.apps; do
                migration_error=/tmp/migration-error
                if root_managed_by="$(kubectl -n "$namespace" get "$migration_kind" "$migration_root" -o jsonpath='{.metadata.labels.managed-by}' 2>"$migration_error")"; then
                  if [ "$root_managed_by" = runtime ]; then
                    root_scope="$(kubectl -n "$namespace" get "$migration_kind" "$migration_root" -o "go-template=$migration_label_go_template")"
                    root_resource="$(kubectl -n "$namespace" get "$migration_kind" "$migration_root" -o "go-template=$migration_resource_go_template")"
                    root_legacy_scope=""
                    root_legacy_resource=""
                    if [ -n "$migration_legacy_label_key" ]; then
                      root_legacy_scope="$(kubectl -n "$namespace" get "$migration_kind" "$migration_root" -o "go-template=$migration_legacy_label_go_template")"
                      root_legacy_resource="$(kubectl -n "$namespace" get "$migration_kind" "$migration_root" -o "go-template=$migration_legacy_resource_go_template")"
                    fi
                    if [ "$root_scope" = "$migration_label_value" ] && [ "$root_resource" = "$migration_resource_id" ]; then
                      :
                    elif [ -n "$migration_legacy_label_key" ] && [ "$root_legacy_scope" = "$migration_label_value" ] && [ "$root_legacy_resource" = "$migration_resource_id" ]; then
                      :
                    else
                      migration_complete=false
                    fi
                  fi
                elif ! grep -q '(NotFound)' "$migration_error"; then
                  cat "$migration_error" >&2
                  echo "Cannot verify runtime workload ownership migration; refusing history pruning." >&2
                  exit 1
                fi
              done
              {{- end }}
              {{- range $runtimeBuild := $runtimeBuilds }}
              migration_root={{ $runtimeBuild.name | quote }}
              migration_resource_id={{ $runtimeBuild.resourceId | quote }}
              migration_error=/tmp/migration-error
              if root_managed_by="$(kubectl -n "$namespace" get job.batch "$migration_root" -o jsonpath='{.metadata.labels.managed-by}' 2>"$migration_error")"; then
                root_component="$(kubectl -n "$namespace" get job.batch "$migration_root" -o jsonpath='{.metadata.labels.component}')"
                root_app="$(kubectl -n "$namespace" get job.batch "$migration_root" -o jsonpath='{.metadata.labels.app}')"
                root_scope="$(kubectl -n "$namespace" get job.batch "$migration_root" -o "go-template=$migration_label_go_template")"
                root_resource="$(kubectl -n "$namespace" get job.batch "$migration_root" -o "go-template=$migration_resource_go_template")"
                root_legacy_scope=""
                root_legacy_resource=""
                if [ -n "$migration_legacy_label_key" ]; then
                  root_legacy_scope="$(kubectl -n "$namespace" get job.batch "$migration_root" -o "go-template=$migration_legacy_label_go_template")"
                  root_legacy_resource="$(kubectl -n "$namespace" get job.batch "$migration_root" -o "go-template=$migration_legacy_resource_go_template")"
                fi
                if [ "$root_managed_by" != runtime ] || [ "$root_component" != build ] || [ "$root_app" != "$migration_root" ]; then
                  migration_complete=false
                elif [ "$root_scope" = "$migration_label_value" ] && [ "$root_resource" = "$migration_resource_id" ]; then
                  :
                elif [ -n "$migration_legacy_label_key" ] && [ "$root_legacy_scope" = "$migration_label_value" ] && [ "$root_legacy_resource" = "$migration_resource_id" ]; then
                  :
                else
                  migration_complete=false
                fi
              elif ! grep -q '(NotFound)' "$migration_error"; then
                cat "$migration_error" >&2
                echo "Cannot verify Build Job ownership migration; refusing history pruning." >&2
                exit 1
              fi
              {{- end }}
              if [ "$migration_complete" != true ]; then
                echo "Runtime workload ownership migration is incomplete; retaining the legacy Helm rollback revision."
                exit 0
              fi
              scratch=/tmp/validated-history
              : > "$scratch"
              revision=1
              while [ "$revision" -lt "$first_safe_revision" ]; do
                record="sh.helm.release.v1.$release_name.v$revision"
                error_file="/tmp/history-$revision.err"
                if ! kubectl -n "$namespace" get "$history_kind" "$record" -o name >/dev/null 2>"$error_file"; then
                  if grep -q '(NotFound)' "$error_file"; then
                    revision=$((revision + 1))
                    continue
                  fi
                  cat "$error_file" >&2
                  exit 1
                fi
                owner="$(kubectl -n "$namespace" get "$history_kind" "$record" -o jsonpath='{.metadata.labels.owner}')"
                record_release="$(kubectl -n "$namespace" get "$history_kind" "$record" -o jsonpath='{.metadata.labels.name}')"
                record_revision="$(kubectl -n "$namespace" get "$history_kind" "$record" -o jsonpath='{.metadata.labels.version}')"
                record_kind="$(kubectl -n "$namespace" get "$history_kind" "$record" -o jsonpath='{.kind}')"
                if [ "$history_kind" = secret ]; then
                  record_type="$(kubectl -n "$namespace" get secret "$record" -o jsonpath='{.type}')"
                  expected_kind=Secret
                  expected_type=helm.sh/release.v1
                else
                  record_type=configmap
                  expected_kind=ConfigMap
                  expected_type=configmap
                fi
                if [ "$owner" != helm ] || [ "$record_release" != "$release_name" ] || [ "$record_revision" != "$revision" ] || [ "$record_kind" != "$expected_kind" ] || [ "$record_type" != "$expected_type" ]; then
                  echo "Helm history record $namespace/$record has foreign or malformed ownership; refusing history pruning." >&2
                  exit 1
                fi
                printf '%s\n' "$revision" >> "$scratch"
                revision=$((revision + 1))
              done
              prior_revision=$((first_safe_revision - 1))
              while IFS= read -r revision; do
                [ -n "$revision" ] || continue
                [ "$revision" -ne "$prior_revision" ] || continue
                record="sh.helm.release.v1.$release_name.v$revision"
                kubectl -n "$namespace" delete "$history_kind" "$record" --wait=false
              done < "$scratch"
              if grep -qx "$prior_revision" "$scratch"; then
                record="sh.helm.release.v1.$release_name.v$prior_revision"
                kubectl -n "$namespace" delete "$history_kind" "$record" --wait=false
              fi
          volumeMounts:
            - name: current-history
              mountPath: /history
              readOnly: true
{{- end }}
{{- end }}
"#
    .to_string()
}

fn cleanup_job_tpl() -> String {
    r#"{{- $cleanup := dig "cleanup" "onUninstall" dict .Values.runtime -}}
{{- $cleanupLabelKey := default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey -}}
{{- $legacyCleanupLabelKey := default "" .Values.logCollector.scope.legacyDeploymentLabelKey -}}
{{- $cleanupLabelValue := default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue -}}
{{- $cleanupResourceLabelKey := printf "%s/resource" (trimSuffix "/deployment" $cleanupLabelKey) -}}
{{- $legacyCleanupResourceLabelKey := "" -}}
{{- if $legacyCleanupLabelKey -}}
  {{- $legacyCleanupResourceLabelKey = printf "%s/resource" (trimSuffix "/deployment" $legacyCleanupLabelKey) -}}
{{- end -}}
{{- $stack := .Files.Get "files/stack.json" | fromJson -}}
{{- $sandboxIds := list -}}
{{- range $entry := values (default dict $stack.resources) -}}
  {{- if eq (default "" $entry.config.type) "sandbox" -}}
    {{- $sandboxIds = append $sandboxIds $entry.config.id -}}
  {{- end -}}
{{- end -}}
{{- if dig "enabled" true $cleanup }}
apiVersion: batch/v1
kind: Job
metadata:
  name: {{ include "deployment.releaseScopedName" (dict "root" . "suffix" "-cleanup") }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
  annotations:
    "helm.sh/hook": pre-delete
    "helm.sh/hook-weight": "-10"
    "helm.sh/hook-delete-policy": before-hook-creation,hook-succeeded,hook-failed
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
              namespace={{ .Release.Namespace | quote }}
              scope_name={{ include "deployment.releaseScopedName" (dict "root" . "suffix" "-runtime-scope") | quote }}
              expected_label_key="$(printf '%s' {{ $cleanupLabelKey | b64enc | quote }} | base64 -d)"
              expected_legacy_label_key="$(printf '%s' {{ $legacyCleanupLabelKey | b64enc | quote }} | base64 -d)"
              expected_label_value="$(printf '%s' {{ $cleanupLabelValue | b64enc | quote }} | base64 -d)"
              expected_resource_label_key="$(printf '%s' {{ $cleanupResourceLabelKey | b64enc | quote }} | base64 -d)"
              expected_legacy_resource_label_key="$(printf '%s' {{ $legacyCleanupResourceLabelKey | b64enc | quote }} | base64 -d)"
              expected_release_name="$(printf '%s' {{ .Release.Name | b64enc | quote }} | base64 -d)"
              expected_release_namespace="$(printf '%s' {{ .Release.Namespace | b64enc | quote }} | base64 -d)"
              expected_release_service="$(printf '%s' {{ .Release.Service | b64enc | quote }} | base64 -d)"
              # One field per line: BusyBox ash `read` collapses empty IFS fields,
              # and the optional legacy label key is empty on default-domain charts.
              scope_contract="$(kubectl -n "$namespace" get configmap "$scope_name" -o go-template='{{ "{{" }} printf "%t\n%d\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n%s\n" .immutable (len .data) (index .data "version") (index .data "labelKey") (index .data "legacyLabelKey") (index .data "labelValue") (index .data "resourceLabelKey") (index .data "firstSafeRevision") (index .metadata.annotations "meta.helm.sh/release-name") (index .metadata.annotations "meta.helm.sh/release-namespace") (index .metadata.labels "app.kubernetes.io/managed-by") (index .metadata.labels "app.kubernetes.io/instance") (index .metadata.labels "alien.dev/runtime-cleanup-scope") {{ "}}" }}')"
              {
                read -r scope_immutable
                read -r scope_data_count
                read -r scope_version
                read -r deployment_label_key
                read -r legacy_deployment_label_key
                read -r deployment_label_value
                read -r resource_label_key
                read -r first_safe_revision
                read -r scope_release_name
                read -r scope_release_namespace
                read -r scope_release_service
                read -r scope_release_instance
                read -r scope_marker
              } <<EOF
              $scope_contract
              EOF
              if [ "$scope_immutable" != true ] || [ "$scope_data_count" != 6 ] || [ "$scope_version" != 2 ] || [ "$deployment_label_key" != "$expected_label_key" ] || [ "$legacy_deployment_label_key" != "$expected_legacy_label_key" ] || [ "$deployment_label_value" != "$expected_label_value" ] || [ "$resource_label_key" != "$expected_resource_label_key" ] || [ "$scope_release_name" != "$expected_release_name" ] || [ "$scope_release_namespace" != "$expected_release_namespace" ] || [ "$scope_release_service" != "$expected_release_service" ] || [ "$scope_release_instance" != "$expected_release_name" ] || [ "$scope_marker" != v2 ]; then
                echo "Runtime cleanup scope contract does not match this Helm release; refusing namespace cleanup." >&2
                exit 1
              fi
              case "$first_safe_revision" in
                ''|*[!0-9]*) echo "Runtime cleanup scope carries an invalid first safe revision; refusing namespace cleanup." >&2; exit 1 ;;
              esac
              if [ "$first_safe_revision" -lt 1 ] || [ "$first_safe_revision" -gt {{ .Release.Revision }} ]; then
                echo "Runtime cleanup scope carries an impossible first safe revision; refusing namespace cleanup." >&2
                exit 1
              fi
              if [ "${#deployment_label_key}" -gt 264 ] || ! printf '%s' "$deployment_label_key" | grep -Eq '^[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?(\.[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?)*/deployment$'; then
                echo "Runtime cleanup scope contains an invalid Kubernetes label key; refusing namespace cleanup." >&2
                exit 1
              fi
              if [ "${#deployment_label_value}" -gt 63 ] || ! printf '%s' "$deployment_label_value" | grep -Eq '^[A-Za-z0-9]([-A-Za-z0-9_.]*[A-Za-z0-9])?$'; then
                echo "Runtime cleanup scope contains an invalid Kubernetes label value; refusing namespace cleanup." >&2
                exit 1
              fi
              deployment_label_go_template='{{ "{{" }} with index .metadata.labels "'$deployment_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              legacy_deployment_label_go_template='{{ "{{" }} with index .metadata.labels "'$legacy_deployment_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              resource_label_go_template='{{ "{{" }} with index .metadata.labels "'$resource_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              legacy_resource_label_go_template='{{ "{{" }} with index .metadata.labels "'$expected_legacy_resource_label_key'" {{ "}}" }}{{ "{{" }} . {{ "}}" }}{{ "{{" }} end {{ "}}" }}'
              selector="managed-by=runtime,$deployment_label_key=$deployment_label_value"
              selectors="$selector"
              if [ -n "$legacy_deployment_label_key" ]; then
                selectors="$selectors managed-by=runtime,$legacy_deployment_label_key=$deployment_label_value"
              fi
              scratch=/tmp/alien-runtime-cleanup
              mkdir -p "$scratch"
              : > "$scratch/env-secrets"
              : > "$scratch/registry-secrets"
              : > "$scratch/pvcs"
              : > "$scratch/pending-deletions"
              resource_exists() {
                object="$1"
                error_file="$scratch/get-error"
                if kubectl -n "$namespace" get "$object" -o name >/dev/null 2>"$error_file"; then
                  return 0
                fi
                if grep -q '(NotFound)' "$error_file"; then
                  return 1
                fi
                cat "$error_file" >&2
                echo "Refusing cleanup: cannot determine whether $object still exists." >&2
                exit 1
              }

              # Stop the exact Helm-owned manager before taking the cleanup
              # snapshot. Otherwise its reconciliation loop could recreate a
              # scoped dependent after the Job has already listed it.
              manager_name={{ include "deployment.fullname" . | quote }}
              if resource_exists "deployment/$manager_name"; then
                manager_contract="$(kubectl -n "$namespace" get deployment "$manager_name" -o go-template='{{ "{{" }} printf "%s\t%s\t%s\t%s" (index .metadata.annotations "meta.helm.sh/release-name") (index .metadata.annotations "meta.helm.sh/release-namespace") (index .metadata.labels "app.kubernetes.io/managed-by") (index .metadata.labels "app.kubernetes.io/instance") {{ "}}" }}')"
                IFS="$(printf '\t')" read -r manager_release_name manager_release_namespace manager_release_service manager_release_instance <<EOF
              $manager_contract
              EOF
                if [ "$manager_release_name" != "$expected_release_name" ] || [ "$manager_release_namespace" != "$expected_release_namespace" ] || [ "$manager_release_service" != "$expected_release_service" ] || [ "$manager_release_instance" != "$expected_release_name" ]; then
                  echo "Manager Deployment $namespace/$manager_name is not owned by this exact Helm release; refusing cleanup." >&2
                  exit 1
                fi
                kubectl -n "$namespace" delete deployment "$manager_name" --cascade=foreground --wait=false
                manager_delete_seconds_remaining=75
                while resource_exists "deployment/$manager_name"; do
                  if [ "$manager_delete_seconds_remaining" -le 0 ]; then
                    echo "Timed out waiting for manager Deployment $namespace/$manager_name to stop before runtime cleanup." >&2
                    exit 1
                  fi
                  sleep 1
                  manager_delete_seconds_remaining=$((manager_delete_seconds_remaining - 1))
                done
              fi

              # Legacy sandbox objects are ambiguous in a shared namespace:
              # their names and labels did not include deployment identity.
              # Refuse a successful uninstall instead of adopting or deleting
              # an object that can belong to a sibling release.
              {{- range $sandboxId := $sandboxIds }}
              sandbox_id="$(printf '%s' {{ $sandboxId | b64enc | quote }} | base64 -d)"
              sandbox_selector="alien.dev/sandbox=$sandbox_id"
              sandbox_pods="$scratch/sandbox-pods"
              if ! kubectl -n "$namespace" get pods -l "$sandbox_selector" -o name >"$sandbox_pods" 2>"$scratch/list-error"; then
                cat "$scratch/list-error" >&2
                echo "Refusing cleanup: cannot list sandbox Pods." >&2
                exit 1
              fi
              while IFS= read -r sandbox_pod; do
                [ -n "$sandbox_pod" ] || continue
                pod_scope="$(kubectl -n "$namespace" get "$sandbox_pod" -o "go-template=$deployment_label_go_template")"
                if [ -z "$pod_scope" ] && [ -n "$legacy_deployment_label_key" ]; then
                  pod_scope="$(kubectl -n "$namespace" get "$sandbox_pod" -o "go-template=$legacy_deployment_label_go_template")"
                fi
                if [ -z "$pod_scope" ]; then
                  echo "Sandbox Pod $namespace/$sandbox_pod has ambiguous legacy ownership; label it for this deployment after verifying ownership before uninstall." >&2
                  exit 1
                fi
              done < "$sandbox_pods"
              capability_secret="alien-sandbox-$sandbox_id-capability"
              if resource_exists "secret/$capability_secret"; then
                capability_owner="$(kubectl -n "$namespace" get secret "$capability_secret" -o jsonpath='{.metadata.labels.alien\.dev/sandbox}')"
                capability_scope="$(kubectl -n "$namespace" get secret "$capability_secret" -o "go-template=$deployment_label_go_template")"
                if [ -z "$capability_scope" ] && [ -n "$legacy_deployment_label_key" ]; then
                  capability_scope="$(kubectl -n "$namespace" get secret "$capability_secret" -o "go-template=$legacy_deployment_label_go_template")"
                fi
                if [ "$capability_owner" != "$sandbox_id" ] || [ "$capability_scope" != "$deployment_label_value" ]; then
                  echo "Sandbox capability Secret $namespace/$capability_secret has ambiguous legacy ownership; label it for this deployment after verifying ownership before uninstall." >&2
                  exit 1
                fi
              fi
              {{- end }}

              # Record the exact dependency graph before deleting its scoped
              # workload roots. This migrates objects created by older
              # Operators that did not yet stamp the deployment scope label.
              for cleanup_selector in $selectors; do
              workloads="$scratch/workloads"
              if ! kubectl -n "$namespace" get deployments.apps,statefulsets.apps,daemonsets.apps -l "$cleanup_selector" -o name >"$workloads" 2>"$scratch/list-error"; then
                cat "$scratch/list-error" >&2
                echo "Refusing cleanup: cannot list runtime workloads." >&2
                exit 1
              fi
              while IFS= read -r workload; do
                [ -n "$workload" ] || continue
                workload_name="${workload#*/}"
                resource_id="$(kubectl -n "$namespace" get "$workload" -o "go-template=$resource_label_go_template")"
                if [ -z "$resource_id" ] && [ -n "$legacy_deployment_label_key" ]; then
                  resource_id="$(kubectl -n "$namespace" get "$workload" -o "go-template=$legacy_resource_label_go_template")"
                fi
                if [ -n "$resource_id" ]; then
                  kubectl -n "$namespace" get "$workload" -o jsonpath='{range .spec.template.spec.containers[*].env[*]}{.valueFrom.secretKeyRef.name}{"\n"}{end}{range .spec.template.spec.initContainers[*].env[*]}{.valueFrom.secretKeyRef.name}{"\n"}{end}' > "$scratch/workload-env-secrets"
                  while IFS= read -r secret_name; do
                    [ -n "$secret_name" ] && printf '%s\t%s\n' "$secret_name" "$resource_id" >> "$scratch/env-secrets"
                  done < "$scratch/workload-env-secrets"
                fi
                kubectl -n "$namespace" get "$workload" -o jsonpath='{range .spec.template.spec.imagePullSecrets[*]}{.name}{"\n"}{end}' > "$scratch/workload-registry-secrets"
                while IFS= read -r secret_name; do
                  [ -n "$secret_name" ] && printf '%s\t%s\n' "$secret_name" "$workload_name" >> "$scratch/registry-secrets"
                done < "$scratch/workload-registry-secrets"
                case "$workload" in
                  statefulset.apps/*)
                    kubectl -n "$namespace" get "$workload" -o jsonpath='{range .spec.volumeClaimTemplates[*]}{.metadata.name}{"\n"}{end}' > "$scratch/workload-pvcs"
                    while IFS= read -r claim_name; do
                      [ -n "$claim_name" ] && printf '%s\t%s\n' "$claim_name" "$workload_name" >> "$scratch/pvcs"
                    done < "$scratch/workload-pvcs"
                    ;;
                esac
              done < "$workloads"
              done

              for cleanup_selector in $selectors; do
              kubectl -n "$namespace" get deployments.apps,statefulsets.apps,daemonsets.apps,replicasets.apps,jobs.batch,pods,services,configmaps,secrets,networkpolicies.networking.k8s.io,ingresses.networking.k8s.io -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
              kubectl -n "$namespace" delete deployments.apps,statefulsets.apps,daemonsets.apps -l "$cleanup_selector" --ignore-not-found=true --cascade=foreground --wait=false
              kubectl -n "$namespace" delete replicasets.apps,jobs.batch,pods -l "$cleanup_selector" --ignore-not-found=true --wait=false
              kubectl -n "$namespace" delete services,configmaps,secrets,networkpolicies.networking.k8s.io,ingresses.networking.k8s.io -l "$cleanup_selector" --ignore-not-found=true --wait=false
              if ! gateway_resources="$(kubectl api-resources --api-group gateway.networking.k8s.io --no-headers 2>"$scratch/discovery-error")"; then
                cat "$scratch/discovery-error" >&2
                echo "Refusing cleanup: Gateway API discovery failed." >&2
                exit 1
              fi
              if printf '%s\n' "$gateway_resources" | awk '{print $1}' | grep -qx 'httproutes'; then
                kubectl -n "$namespace" get httproutes.gateway.networking.k8s.io -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
                kubectl -n "$namespace" delete httproutes.gateway.networking.k8s.io -l "$cleanup_selector" --ignore-not-found=true --wait=false
              fi
              if printf '%s\n' "$gateway_resources" | awk '{print $1}' | grep -qx 'gateways'; then
                kubectl -n "$namespace" get gateways.gateway.networking.k8s.io -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
                kubectl -n "$namespace" delete gateways.gateway.networking.k8s.io -l "$cleanup_selector" --ignore-not-found=true --wait=false
              fi
              if ! gke_resources="$(kubectl api-resources --api-group networking.gke.io --no-headers 2>"$scratch/discovery-error")"; then
                cat "$scratch/discovery-error" >&2
                echo "Refusing cleanup: GKE networking API discovery failed." >&2
                exit 1
              fi
              if printf '%s\n' "$gke_resources" | awk '{print $1}' | grep -qx 'healthcheckpolicies'; then
                kubectl -n "$namespace" get healthcheckpolicies.networking.gke.io -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
                kubectl -n "$namespace" delete healthcheckpolicies.networking.gke.io -l "$cleanup_selector" --ignore-not-found=true --wait=false
              fi
              if ! azure_resources="$(kubectl api-resources --api-group alb.networking.azure.io --no-headers 2>"$scratch/discovery-error")"; then
                cat "$scratch/discovery-error" >&2
                echo "Refusing cleanup: Azure networking API discovery failed." >&2
                exit 1
              fi
              if printf '%s\n' "$azure_resources" | awk '{print $1}' | grep -qx 'healthcheckpolicies'; then
                kubectl -n "$namespace" get healthcheckpolicies.alb.networking.azure.io -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
                kubectl -n "$namespace" delete healthcheckpolicies.alb.networking.azure.io -l "$cleanup_selector" --ignore-not-found=true --wait=false
              fi
              done

              sort -u "$scratch/env-secrets" -o "$scratch/env-secrets"
              while IFS="$(printf '\t')" read -r secret_name resource_id; do
                [ -n "$secret_name" ] || continue
                if ! resource_exists "secret/$secret_name"; then
                  continue
                fi
                secret_managed_by="$(kubectl -n "$namespace" get secret "$secret_name" -o jsonpath='{.metadata.labels.managed-by}')"
                secret_resource_id="$(kubectl -n "$namespace" get secret "$secret_name" -o jsonpath='{.metadata.labels.resource-id}')"
                if [ "$secret_managed_by" = runtime ] && [ "$secret_resource_id" = "$resource_id" ]; then
                  printf 'secret/%s\n' "$secret_name" >> "$scratch/pending-deletions"
                  kubectl -n "$namespace" delete secret "$secret_name" --ignore-not-found=true --wait=false
                fi
              done < "$scratch/env-secrets"

              sort -u "$scratch/registry-secrets" -o "$scratch/registry-secrets"
              while IFS="$(printf '\t')" read -r secret_name workload_name; do
                [ -n "$secret_name" ] || continue
                if ! resource_exists "secret/$secret_name"; then
                  continue
                fi
                secret_type="$(kubectl -n "$namespace" get secret "$secret_name" -o jsonpath='{.type}')"
                if [ "$secret_name" = "$workload_name-registry" ] && [ "$secret_type" = kubernetes.io/dockerconfigjson ]; then
                  printf 'secret/%s\n' "$secret_name" >> "$scratch/pending-deletions"
                  kubectl -n "$namespace" delete secret "$secret_name" --ignore-not-found=true --wait=false
                fi
              done < "$scratch/registry-secrets"

              {{- if dig "deletePersistentVolumeClaims" false $cleanup }}
              for cleanup_selector in $selectors; do
                kubectl -n "$namespace" get persistentvolumeclaims -l "$cleanup_selector" -o name >> "$scratch/pending-deletions"
                kubectl -n "$namespace" delete persistentvolumeclaims -l "$cleanup_selector" --ignore-not-found=true --wait=false
              done
              sort -u "$scratch/pvcs" -o "$scratch/pvcs"
              if ! kubectl -n "$namespace" get persistentvolumeclaims -o name >"$scratch/all-pvcs" 2>"$scratch/list-error"; then
                cat "$scratch/list-error" >&2
                echo "Refusing cleanup: cannot list PersistentVolumeClaims." >&2
                exit 1
              fi
              while IFS="$(printf '\t')" read -r claim_name workload_name; do
                [ -n "$claim_name" ] || continue
                while IFS= read -r pvc; do
                  [ -n "$pvc" ] || continue
                  pvc_name="${pvc#*/}"
                  prefix="$claim_name-$workload_name-"
                  case "$pvc_name" in
                    "$prefix"*)
                      ordinal="${pvc_name#"$prefix"}"
                      case "$ordinal" in
                        ''|*[!0-9]*) ;;
                        *)
                          printf '%s\n' "$pvc" >> "$scratch/pending-deletions"
                          kubectl -n "$namespace" delete "$pvc" --ignore-not-found=true --wait=false
                          ;;
                      esac
                      ;;
                  esac
                done < "$scratch/all-pvcs"
              done < "$scratch/pvcs"
              {{- else }}
              echo "Preserving runtime PersistentVolumeClaims. Set runtime.cleanup.onUninstall.deletePersistentVolumeClaims=true to delete them."
              {{- end }}
              sort -u "$scratch/pending-deletions" -o "$scratch/pending-deletions"
              attempts=0
              while true; do
                remaining=false
                while IFS= read -r object; do
                  [ -n "$object" ] || continue
                  if resource_exists "$object"; then
                    remaining=true
                  fi
                done < "$scratch/pending-deletions"
                [ "$remaining" = false ] && break
                attempts=$((attempts + 1))
                if [ "$attempts" -ge 75 ]; then
                  echo "Runtime Pods or PersistentVolumeClaims were not deleted within 75 seconds." >&2
                  exit 1
                fi
                sleep 1
              done
              trap - EXIT
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
        alien.dev/log-collector-exclude: "true"
        {{- with .Values.runtime.podLabels }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      annotations:
        checksum/input-values: {{ toJson .Values.inputValues | sha256sum | quote }}
        checksum/management-credential: {{ toJson (dict "token" .Values.management.token "existingSecret" .Values.management.existingSecret) | sha256sum | quote }}
        checksum/collector-credential: {{ toJson .Values.logCollector.token | sha256sum | quote }}
        {{- with .Values.runtime.podAnnotations }}
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
            - name: SYNC_URL
              value: {{ .Values.management.url | quote }}
            - name: OPERATOR_NAME
              value: {{ .Values.management.name | quote }}
            - name: OPERATOR_RESOURCE_PREFIX
              value: {{ include "deployment.serviceAccountPrefix" . | quote }}
            {{- if .Values.management.deploymentId }}
            - name: DEPLOYMENT_ID
              value: {{ .Values.management.deploymentId | quote }}
            {{- end }}
            - name: ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY
              value: {{ default "alien.dev/deployment" .Values.logCollector.scope.deploymentLabelKey | quote }}
            - name: ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE
              value: {{ default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue | quote }}
            {{- if .Values.logCollector.scope.legacyDeploymentLabelKey }}
            - name: ALIEN_RUNTIME_LEGACY_DEPLOYMENT_LABEL_KEY
              value: {{ .Values.logCollector.scope.legacyDeploymentLabelKey | quote }}
            {{- end }}
            - name: KUBERNETES_NAMESPACE
              value: {{ .Release.Namespace | quote }}
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
            {{- if .Values.inputValues }}
            - name: STACK_INPUT_VALUES_FILE
              value: /etc/deployment/input-values/input-values.json
            {{- end }}
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
            {{- if .Values.inputValues }}
            - name: input-values
              mountPath: /etc/deployment/input-values
              readOnly: true
            {{- end }}
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
        {{- if .Values.inputValues }}
        - name: input-values
          secret:
            secretName: {{ include "deployment.fullname" . }}
            items:
              - key: input-values.json
                path: input-values.json
            defaultMode: 384
        {{- end }}
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
  name: {{ include "deployment.logCollectorName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: log-collector
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
  name: {{ include "deployment.logCollectorName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: log-collector
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
  name: {{ include "deployment.logCollectorName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: log-collector
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: Role
  name: {{ include "deployment.logCollectorName" . }}
subjects:
  - kind: ServiceAccount
    name: {{ include "deployment.logCollectorName" . }}
    namespace: {{ .Release.Namespace }}
{{- end }}
"#
    .to_string()
}

fn whitelabeled_log_collector_configmap_tpl() -> String {
    r#"{{- if .Values.logCollector.enabled }}
{{- $podLabelKey := .Values.logCollector.scope.podLabelKey -}}
{{- $podLabelValue := .Values.logCollector.scope.podLabelValue -}}
{{- if ne (empty $podLabelKey) (empty $podLabelValue) -}}
  {{- fail "logCollector.scope.podLabelKey and podLabelValue must be set together" -}}
{{- end -}}
{{- $logLabelKey := default .Values.logCollector.scope.deploymentLabelKey $podLabelKey -}}
{{- $logLabelValue := default (default (include "deployment.fullname" .) .Values.logCollector.scope.deploymentLabelValue) $podLabelValue -}}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "deployment.logCollectorName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: log-collector
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
        # The Kubernetes filter parses the /var/log/containers symlink filename.
        Path              /var/log/containers/*_{{ .Release.Namespace }}_*.log
        Path_Key          filename
        multiline.parser  docker, cri
        Tag               kube.*
        DB                /buffers/{{ include "deployment.logCollectorName" . }}.db
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

    [FILTER]
        Name                grep
        Match               kube.*
        Exclude             $kubernetes['labels']['alien.dev/log-collector-exclude'] ^true$

    [FILTER]
        Name                grep
        Match               kube.*
        Regex               $kubernetes['labels']['{{ $logLabelKey }}'] ^{{ $logLabelValue | regexQuoteMeta }}$

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
  name: {{ include "deployment.logCollectorName" . }}
  labels:
    {{- include "deployment.labels" . | nindent 4 }}
    app.kubernetes.io/component: log-collector
spec:
  selector:
    matchLabels:
      app.kubernetes.io/name: {{ include "deployment.name" . }}
      app.kubernetes.io/instance: {{ .Release.Name }}
      app.kubernetes.io/component: log-collector
  template:
    metadata:
      labels:
        {{- include "deployment.labels" . | nindent 8 }}
        app.kubernetes.io/component: log-collector
        alien.dev/log-collector-exclude: "true"
      annotations:
        checksum/collector-credential: {{ toJson .Values.logCollector.token | sha256sum | quote }}
    spec:
      serviceAccountName: {{ include "deployment.logCollectorName" . }}
      tolerations:
        - operator: Exists
      securityContext:
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: collector
          image: "{{ .Values.logCollector.image.repository }}:{{ .Values.logCollector.image.tag }}"
          imagePullPolicy: {{ .Values.logCollector.image.pullPolicy }}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: [ALL]
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
            name: {{ include "deployment.logCollectorName" . }}
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
    managed-by: runtime
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

fn readme_md(chart_name: &str) -> String {
    format!(
        "# {chart_name}\n\nFrom this chart directory, set the required values and install into the chosen namespace (`default` shown here):\n\n```bash\nhelm install {chart_name} . --namespace default --values values.yaml\n```\n\nFor a managed package, use its generated install command and values. See `examples/<target>.yaml` for EKS, GKE, AKS, and on-premises values.\n\nTo collect selected workload Pod logs, set `logCollector.enabled: true` in values.yaml. The collector mounts node log directories read-only; namespaces enforcing Baseline or Restricted Pod Security reject those mounts. When `logCollector.token` is empty, Helm generates a per-installation token and retains it across upgrades.\n"
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
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        })
        .expect("operator manifest should render")
    }

    fn operator_test_manifest_with_log_collector() -> String {
        generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            log_collector: Some(OperatorLogCollectorOptions {
                image: "fluent/fluent-bit:3.2",
                token: "collector-secret",
                pod_label_key: None,
                pod_label_value: None,
            }),
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        })
        .expect("operator manifest should render")
    }

    fn operator_test_manifest_with_stack_settings() -> String {
        let stack_settings = StackSettings {
            updates: alien_core::UpdatesMode::ApprovalRequired,
            ..Default::default()
        };
        generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: Some(&stack_settings),
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
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
                // The access-request CRD is cluster-scoped and leads the manifest
                // so the kind is registered before any namespaced object.
                "CustomResourceDefinition",
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
            // The CRD is a cluster-scoped object; only namespaced docs carry a
            // metadata.namespace.
            if yaml_str(&doc, "kind") == Some("CustomResourceDefinition") {
                assert!(
                    yaml_path(&doc, &["metadata", "namespace"]).is_none(),
                    "the CRD is cluster-scoped and must not be namespaced"
                );
                continue;
            }
            assert_eq!(
                yaml_path(&doc, &["metadata", "namespace"]).and_then(YamlValue::as_str),
                Some("demo"),
                "every namespaced operator document should be namespaced"
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
            let api_groups = rule
                .get("apiGroups")
                .and_then(YamlValue::as_sequence)
                .expect("rule should include apiGroups")
                .iter()
                .map(|g| g.as_str().expect("apiGroup should be string"))
                .collect::<Vec<_>>();
            let verbs = rule
                .get("verbs")
                .and_then(YamlValue::as_sequence)
                .expect("rule should include verbs")
                .iter()
                .map(|verb| verb.as_str().expect("verb should be string"))
                .collect::<Vec<_>>();

            let resources = rule
                .get("resources")
                .and_then(YamlValue::as_sequence)
                .expect("rule should include resources")
                .iter()
                .map(|resource| resource.as_str().expect("resource should be string"))
                .collect::<Vec<_>>();

            // Secrets are never accessible, on any rule.
            assert!(
                !resources.contains(&"secrets"),
                "operator must not access customer Secrets"
            );

            // The access-request CRD is the operator's own control resource: it
            // materializes access requests and records the approval window in
            // status, but never deletes them.
            if api_groups == vec!["accessrequests.alien"] {
                assert!(
                    verbs.iter().all(|v| matches!(
                        *v,
                        "get" | "list" | "watch" | "create" | "update" | "patch"
                    )),
                    "access-request rule verbs must stay within the materialize/status set: {verbs:?}"
                );
                assert!(
                    !verbs.contains(&"delete") && !verbs.contains(&"deletecollection"),
                    "operator must not delete access-request CRs"
                );
                continue;
            }

            let allowed = ["get", "list", "watch"];
            for v in &verbs {
                assert!(
                    allowed.contains(v),
                    "diagnostic RBAC must be read-only; found '{v}' on {resources:?}"
                );
            }
        }
    }

    #[test]
    fn operator_remediation_adds_only_restart_and_scale_writes() {
        let manifest = generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("test"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Remediation,
            format: OperatorOutputFormat::RawManifest,
        })
        .expect("remediation manifest should render");
        let docs = parse_manifest_docs(&manifest);
        let role = docs_by_kind(&docs, "Role").remove(0);
        let rules = role["rules"]
            .as_sequence()
            .expect("Role should include rules");

        let mut writes = BTreeSet::new();
        for rule in rules {
            for group in rule["apiGroups"].as_sequence().unwrap() {
                for resource in rule["resources"].as_sequence().unwrap() {
                    for verb in rule["verbs"].as_sequence().unwrap() {
                        let verb = verb.as_str().unwrap();
                        if !matches!(verb, "get" | "list" | "watch") {
                            writes.insert((
                                group.as_str().unwrap(),
                                resource.as_str().unwrap(),
                                verb,
                            ));
                        }
                    }
                }
            }
        }
        assert_eq!(
            writes,
            BTreeSet::from([
                ("", "pods", "delete"),
                ("apps", "deployments/scale", "patch"),
                ("apps", "statefulsets/scale", "patch"),
                ("apps", "replicasets/scale", "patch"),
                ("accessrequests.alien", "alienaccessrequests", "create"),
                ("accessrequests.alien", "alienaccessrequests", "update"),
                ("accessrequests.alien", "alienaccessrequests", "patch"),
                (
                    "accessrequests.alien",
                    "alienaccessrequests/status",
                    "update"
                ),
                (
                    "accessrequests.alien",
                    "alienaccessrequests/status",
                    "patch"
                ),
            ])
        );
    }

    #[test]
    fn access_request_crd_is_white_labeled_from_the_brand_domain() {
        // A vendor branded acme.dev gets AcmeAccessRequest, not
        // AlienAccessRequest — the CRD, RBAC, and (elsewhere) the operator
        // runtime all derive from the same brand slug. The brand's DNS shape
        // is never required — it's slugified into the CRD group, never
        // resolved.
        let manifest = generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod"),
            install_namespace: Some("demo"),
            label_domain: Some("acme.dev"),
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        })
        .expect("branded operator manifest should render");
        let docs = parse_manifest_docs(&manifest);

        let crd = docs_by_kind(&docs, "CustomResourceDefinition")
            .into_iter()
            .next()
            .expect("manifest should include the access-request CRD");
        assert_eq!(
            yaml_path(&crd, &["metadata", "name"]).and_then(YamlValue::as_str),
            Some("acmeaccessrequests.accessrequests.acme")
        );
        assert_eq!(
            yaml_path(&crd, &["spec", "group"]).and_then(YamlValue::as_str),
            Some("accessrequests.acme")
        );
        assert_eq!(
            yaml_path(&crd, &["spec", "names", "kind"]).and_then(YamlValue::as_str),
            Some("AcmeAccessRequest")
        );
        assert_eq!(
            yaml_path(&crd, &["spec", "names", "plural"]).and_then(YamlValue::as_str),
            Some("acmeaccessrequests")
        );
        assert_eq!(
            crd["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
                ["properties"]["commands"]["items"]["properties"]["params"]
                ["x-kubernetes-preserve-unknown-fields"]
                .as_bool(),
            Some(true),
            "the CRD must retain exact operation parameters for customer review"
        );
        // Not the Alien defaults.
        let text = &manifest;
        assert!(
            !text.contains("alienaccessrequests"),
            "no alien-named resource in a branded build"
        );
        assert!(
            !text.contains("accessrequests.alien"),
            "no alien group in a branded build"
        );

        // RBAC targets the branded group/resource.
        let role = docs_by_kind(&docs, "Role").into_iter().next().unwrap();
        let has_branded_rule = role
            .get("rules")
            .and_then(YamlValue::as_sequence)
            .unwrap()
            .iter()
            .any(|r| {
                r.get("apiGroups")
                    .and_then(YamlValue::as_sequence)
                    .map(|g| g.iter().any(|x| x.as_str() == Some("accessrequests.acme")))
                    .unwrap_or(false)
            });
        assert!(
            has_branded_rule,
            "RBAC must grant the branded access-request group"
        );
    }

    #[test]
    fn operator_manifest_deployment_uses_group_token_and_persistent_identity() {
        let docs = parse_manifest_docs(&operator_test_manifest());
        let deployment = docs_by_kind(&docs, "Deployment")
            .into_iter()
            .next()
            .expect("operator manifest should include Deployment");
        assert_eq!(
            deployment["spec"]["strategy"]["type"].as_str(),
            Some("Recreate"),
            "the replacement must wait for the persisted identity lock owner to stop"
        );
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
        assert!(!env_names.contains(&"OPERATOR_READINESS_PORT"));
        assert!(
            !env_names.contains(&"DEPLOYMENT_ID"),
            "first boot must self-register and then persist deployment identity"
        );
        let container = &deployment["spec"]["template"]["spec"]["containers"][0];
        assert!(
            container.get("readinessProbe").is_none(),
            "standalone manifests accept older pinned Operator images without /ready"
        );
        assert!(
            container.get("ports").is_none(),
            "standalone manifests without a collector expose no container ports"
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
        assert!(manifest.contains("/var/log/containers/*_demo_*.log"));
        assert!(manifest.contains("/internal/logs"));
        assert!(manifest.contains("COLLECTOR_TOKEN_FILE"));
        assert!(manifest.contains("collector-token"));
        assert!(manifest.contains("fluent/fluent-bit:3.2"));
        let deployment = docs_by_kind(&docs, "Deployment")
            .into_iter()
            .next()
            .expect("operator manifest should include Deployment");
        assert_eq!(operator_env_value(&deployment, "STACK_SETTINGS"), None);
        // The log collector tails pod log FILES on the node, not the API — but
        // the operator's own RBAC does grant `pods/log` for the on-demand `logs`
        // operation, so `pods/log` legitimately appears in the operator Role.
        assert!(manifest.contains("pods/log"));
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
        let pod_spec = &daemonset["spec"]["template"]["spec"];
        assert_eq!(
            pod_spec["securityContext"]["seccompProfile"]["type"],
            "RuntimeDefault"
        );
        let container_security = &pod_spec["containers"][0]["securityContext"];
        assert_eq!(container_security["allowPrivilegeEscalation"], false);
        assert_eq!(container_security["readOnlyRootFilesystem"], true);
        assert_eq!(container_security["capabilities"]["drop"][0], "ALL");
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
    fn helm_collector_rolls_when_external_credentials_change() {
        let labels =
            BTreeMap::from([("app.kubernetes.io/name".to_string(), "operator".to_string())]);
        let daemonset = operator_log_collector_daemonset_doc(
            "{{ .Release.Namespace }}",
            "operator-whitelabeled-log-collector",
            "operator-credentials",
            "fluent/fluent-bit:3.2",
            &labels,
            true,
        );

        let files = indexmap::IndexMap::from([
            (
                "Chart.yaml".to_string(),
                "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n".to_string(),
            ),
            (
                "values.yaml".to_string(),
                "remoteOperator:\n  collectorTokenRevision: \"\"\n".to_string(),
            ),
            ("templates/collector.yaml".to_string(), daemonset),
        ]);

        let revisions = ["a".repeat(64), "b".repeat(64)];
        let rendered = revisions.map(|revision| {
            let values = format!("remoteOperator:\n  collectorTokenRevision: {revision}\n");
            let output = crate::test_utils::helm_template(&files, Some(&values));
            output.assert_ok("collector token rotation");
            let docs = parse_manifest_docs(&output.stdout);
            let daemonset = docs_by_kind(&docs, "DaemonSet")
                .into_iter()
                .next()
                .expect("rendered collector DaemonSet");
            assert_eq!(
                operator_env_value(&daemonset, "COLLECTOR_TOKEN_REVISION"),
                Some(revision.as_str())
            );
            daemonset["spec"]["template"].clone()
        });
        assert_ne!(
            rendered[0], rendered[1],
            "rotating only the collector token marker must change the pod template"
        );
    }

    #[test]
    fn operator_manifest_serializes_stack_settings_without_log_collector() {
        let manifest = operator_test_manifest_with_stack_settings();
        let docs = parse_manifest_docs(&manifest);
        let deployment = docs_by_kind(&docs, "Deployment")
            .into_iter()
            .next()
            .expect("operator manifest should include Deployment");

        assert_eq!(
            operator_env_value(&deployment, "STACK_SETTINGS"),
            Some(r#"{"updates":"approval-required"}"#)
        );
        assert!(docs_by_kind(&docs, "DaemonSet").is_empty());
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
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme-prod-eu"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Cluster,
            label_selector: Some("app.kubernetes.io/part-of=my-saas"),
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
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
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            // Ignored for Helm output — the value comes from .Values / .Release per install.
            environment_name: None,
            install_namespace: None,
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::HelmTemplate,
        })
        .expect("helm template should render");

        assert!(manifest.contains("namespace: '{{ .Release.Namespace }}'"));
        assert!(manifest.contains("value: '{{ .Release.Name }}'"));
        assert!(manifest.contains("name: 'my-saas-operator'"));
        assert!(
            manifest.contains("{{- with .Values.remoteOperator.serviceAccountAnnotations }}"),
            "Helm installs must be able to attach AWS, GCP, or Azure workload identity"
        );
        assert!(
            manifest.contains("{{- with .Values.remoteOperator.podLabels }}"),
            "AKS workload identity requires a pod label in addition to the ServiceAccount"
        );
        let files = indexmap::IndexMap::from([
            (
                "Chart.yaml".to_string(),
                "apiVersion: v2\nname: operator-test\nversion: 0.1.0\n".to_string(),
            ),
            (
                "values.yaml".to_string(),
                "remoteOperator: {}\n".to_string(),
            ),
            ("templates/operator.yaml".to_string(), manifest),
        ]);
        for (values, revision) in [
            (None, "0"),
            (Some("remoteOperator:\n  syncTokenRevision: 7\n"), "7"),
        ] {
            let rendered = crate::test_utils::helm_template(&files, values);
            rendered.assert_ok("credential revision and single identity owner");
            let docs = parse_manifest_docs(&rendered.stdout);
            let deployment = docs_by_kind(&docs, "Deployment")
                .into_iter()
                .next()
                .expect("rendered Deployment");
            assert_eq!(
                operator_env_value(&deployment, "SYNC_TOKEN_REVISION"),
                Some(revision)
            );
            assert_eq!(
                deployment["spec"]["strategy"]["type"].as_str(),
                Some("Recreate")
            );
            assert_eq!(
                deployment["spec"]["strategy"].get("rollingUpdate"),
                Some(&YamlValue::Null),
                "clear the previous strategy when upgrading an existing Deployment"
            );
        }
    }

    #[test]
    fn raw_manifest_requires_install_namespace_and_environment_name() {
        let missing_namespace = generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme"),
            install_namespace: None,
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        });
        assert!(
            missing_namespace.is_err(),
            "raw output needs an install namespace"
        );

        let missing_env = generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: None,
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Namespace,
            label_selector: None,
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
            format: OperatorOutputFormat::RawManifest,
        });
        assert!(missing_env.is_err(), "raw output needs an environment name");

        // Label scope must carry a non-empty selector.
        let empty_label = generate_operator_manifest(OperatorManifestOptions {
            custom_operation_permissions: &[],
            manager_url: "https://manager.example.com",
            group_token: "ax_dg_test",
            encryption_key: TEST_RUNTIME_ENCRYPTION_KEY,
            image: "registry.example.com/operator:test",
            log_collector: None,
            stack_settings: None,
            project_name: "my-saas",
            environment_name: Some("acme"),
            install_namespace: Some("demo"),
            label_domain: None,
            scope: OperatorScope::Cluster,
            label_selector: Some("   "),
            kubernetes_operations_enabled: true,
            permission: OperatorPermission::Diagnostics,
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

    fn sample_product_chart() -> HelmChart {
        sample_product_chart_with_collector(false)
    }

    fn sample_product_chart_with_collector(include_collector: bool) -> HelmChart {
        sample_product_chart_with_collector_and_label_domain(include_collector, None)
    }

    fn sample_product_chart_with_collector_and_label_domain(
        include_collector: bool,
        label_domain: Option<&str>,
    ) -> HelmChart {
        let registry = HelmRegistry::built_in();
        generate_product_helm_chart(
            &sample_stack(),
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: "sample-stack".to_string(),
            },
            ProductOperatorManifestOptions {
                manifest: OperatorManifestOptions {
                    manager_url: "{{ .Values.management.url }}",
                    group_token: "",
                    encryption_key: "",
                    image: "registry.example.com/operator@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    log_collector: include_collector.then_some(OperatorLogCollectorOptions {
                        image: "fluent/fluent-bit:3.2",
                        token: "",
                        pod_label_key: None,
                        pod_label_value: None,
                    }),
                    stack_settings: None,
                    project_name: "remote-sample-stack",
                    environment_name: None,
                    install_namespace: None,
                    label_domain,
                    scope: OperatorScope::Namespace,
                    label_selector: None,
                    kubernetes_operations_enabled: true,
                    custom_operation_permissions: &[],
                    permission: OperatorPermission::Remediation,
                    format: OperatorOutputFormat::HelmTemplate,
                },
                credentials_secret_name: "{{ .Values.remoteOperator.existingSecret.name }}",
                credentials_encryption_key_sha256:
                    "{{ .Values.remoteOperator.existingSecret.encryptionKeySha256 }}",
                resource_name: Some(
                    "{{ printf \"%s-remote-operator\" (include \"deployment.fullname\" .) | trunc 63 | trimSuffix \"-\" }}",
                ),
            },
        )
        .expect("product chart should render")
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
    fn helm_setup_inputs_reach_operator_through_a_secret() {
        let chart = sample_product_chart();
        let values = r#"
management:
  token: ax_dg_example
  name: prod
  url: https://manager.example.test
  deploymentId: null
runtime:
  encryption:
    key: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
inputValues:
  ingestUrl: https://ingest.example.test
  enabled: true
  namespaces:
    - production
"#;
        let rendered = crate::test_utils::helm_template(&chart.files, Some(values));
        rendered.assert_ok("Helm setup inputs render");
        let documents = serde_yaml::Deserializer::from_str(&rendered.stdout)
            .map(|document| YamlValue::deserialize(document).expect("valid Kubernetes YAML"))
            .collect::<Vec<_>>();
        let secret = documents
            .iter()
            .find(|document| document["kind"] == "Secret")
            .expect("setup Secret");
        let input_values: serde_json::Value = serde_json::from_str(
            secret["stringData"]["input-values.json"]
                .as_str()
                .expect("input values are in the Secret"),
        )
        .expect("JSON input values");
        assert_eq!(
            input_values,
            serde_json::json!({
                "ingestUrl": "https://ingest.example.test",
                "enabled": true,
                "namespaces": ["production"]
            })
        );
        let configmap = documents
            .iter()
            .find(|document| document["kind"] == "ConfigMap")
            .expect("runtime ConfigMap");
        assert!(configmap["data"].as_mapping().is_some_and(|data| {
            data.keys()
                .all(|key| key.as_str() != Some("input-values.json"))
        }));
        let operator = documents
            .iter()
            .find(|document| document["kind"] == "Deployment")
            .expect("Operator Deployment");
        let checksum = operator["spec"]["template"]["metadata"]["annotations"]
            ["checksum/input-values"]
            .as_str()
            .expect("input values checksum");
        let credential_checksum = operator["spec"]["template"]["metadata"]["annotations"]
            ["checksum/management-credential"]
            .as_str()
            .expect("management credential checksum");
        let changed_values = values.replace(
            "https://ingest.example.test",
            "https://ingest-updated.example.test",
        );
        let changed = crate::test_utils::helm_template(&chart.files, Some(&changed_values));
        changed.assert_ok("Helm input change renders");
        let changed_operator = serde_yaml::Deserializer::from_str(&changed.stdout)
            .map(|document| YamlValue::deserialize(document).expect("valid Kubernetes YAML"))
            .find(|document| document["kind"] == "Deployment")
            .expect("updated Operator Deployment");
        assert_ne!(
            checksum,
            changed_operator["spec"]["template"]["metadata"]["annotations"]
                ["checksum/input-values"]
                .as_str()
                .expect("updated input values checksum"),
            "changing Helm input values must roll the Operator"
        );
        let rotated_values = values.replace("ax_dg_example", "ax_dg_rotated");
        let rotated = crate::test_utils::helm_template(&chart.files, Some(&rotated_values));
        rotated.assert_ok("Helm credential rotation renders");
        let rotated_operator = serde_yaml::Deserializer::from_str(&rotated.stdout)
            .map(|document| YamlValue::deserialize(document).expect("valid Kubernetes YAML"))
            .find(|document| document["kind"] == "Deployment")
            .expect("rotated Operator Deployment");
        assert_ne!(
            credential_checksum,
            rotated_operator["spec"]["template"]["metadata"]["annotations"]
                ["checksum/management-credential"]
                .as_str()
                .expect("rotated management credential checksum"),
            "changing the Helm management credential must roll the Operator"
        );
        assert_eq!(
            checksum,
            rotated_operator["spec"]["template"]["metadata"]["annotations"]
                ["checksum/input-values"]
                .as_str()
                .expect("unchanged input values checksum")
        );
        let container = &operator["spec"]["template"]["spec"]["containers"][0];
        assert!(container["env"].as_sequence().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["name"] == "STACK_INPUT_VALUES_FILE"
                    && entry["value"] == "/etc/deployment/input-values/input-values.json"
            })
        }));
        assert!(container["volumeMounts"]
            .as_sequence()
            .is_some_and(|mounts| {
                mounts.iter().any(|mount| {
                    mount["name"] == "input-values"
                        && mount["mountPath"] == "/etc/deployment/input-values"
                        && mount["readOnly"].as_bool() == Some(true)
                })
            }));
    }

    #[test]
    fn helm_rejects_unknown_or_wrongly_typed_deployer_inputs() {
        let stack = Stack::new("input-stack".to_string())
            .inputs(vec![alien_core::StackInputDefinition {
                id: "ingestUrl".to_string(),
                kind: alien_core::StackInputKind::String,
                provided_by: vec![alien_core::StackInputProvider::Deployer],
                required: true,
                label: "Ingest URL".to_string(),
                description: "Ingest endpoint".to_string(),
                placeholder: None,
                default: None,
                platforms: Some(vec![Platform::Kubernetes]),
                validation: Some(alien_core::StackInputValidation {
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    format: Some("url".to_string()),
                    min: None,
                    max: None,
                    values: None,
                    min_items: None,
                    max_items: None,
                }),
                env: Vec::new(),
            }])
            .build();
        let registry = HelmRegistry::built_in();
        let chart = generate_helm_chart(
            &stack,
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: "input-stack".to_string(),
            },
        )
        .expect("chart");
        crate::test_utils::helm_lint(&chart.files).assert_ok("lint chart defaults");
        let values = "management:\n  token: ax_test\n  name: test\n  url: https://manager.example.test\n  deploymentId: null\nruntime:\n  encryption:\n    key: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\ninputValues:\n  ingestUrl: https://ingest.example.test\n";
        crate::test_utils::helm_template(&chart.files, Some(values))
            .assert_ok("valid input values");
        for invalid in [
            values.replace("ingestUrl: https://ingest.example.test", "ingestUrl: 42"),
            values.replace(
                "ingestUrl: https://ingest.example.test",
                "ingestUrl: not-a-url",
            ),
            values.replace("ingestUrl:", "ingestUrll:"),
        ] {
            let rendered = crate::test_utils::helm_template(&chart.files, Some(&invalid));
            assert!(!rendered.is_ok(), "Helm accepted invalid input values");
            assert!(
                rendered.stderr.contains("inputValues"),
                "{}",
                rendered.stderr
            );
        }
    }

    #[test]
    fn runtime_cleanup_uses_a_pinned_scope_and_nonblocking_deletes() {
        let registry = HelmRegistry::built_in();
        let chart = generate_helm_chart(
            &sample_stack(),
            HelmOptions {
                registry: &registry,
                stack_settings: StackSettings::default(),
                chart_name: "sample-stack".to_string(),
            },
        )
        .expect("chart");

        let scope = &chart.files["templates/runtime-cleanup-scope.yaml"];
        assert!(scope.contains("immutable: true"));
        assert!(scope.contains("resourceLabelKey"));
        assert!(scope.contains("Runtime cleanup scope is pinned"));

        assert!(!chart.files.contains_key("templates/cleanup-rbac.yaml"));
        let rbac = &chart.files["templates/role.yaml"];
        assert!(rbac.contains(
            "resources: [\"configmaps\", \"secrets\", \"services\", \"pods\", \"pods/log\", \"persistentvolumeclaims\"]"
        ));
        assert!(rbac.contains("resources: [\"jobs\"]"));
        assert!(rbac.contains("apiGroups: [\"networking.gke.io\"]"));
        assert!(rbac.contains("apiGroups: [\"alb.networking.azure.io\"]"));

        let cleanup = &chart.files["templates/cleanup-job.yaml"];
        assert!(cleanup.contains("scope_contract="));
        assert!(
            cleanup.contains("read -r legacy_deployment_label_key"),
            "BusyBox ash collapses empty IFS fields, so parse the optional legacy key as its own line"
        );
        assert!(cleanup.contains("scope_data_count\" != 6"));
        assert!(cleanup.contains("ambiguous legacy ownership"));
        assert!(cleanup.contains("jobs.batch,pods"));
        assert!(cleanup.contains("pending-deletions"));
        for line in cleanup
            .lines()
            .filter(|line| line.contains("kubectl ") && line.contains(" delete "))
        {
            assert!(
                line.contains("--wait=false"),
                "cleanup deletion must not require watch permission: {line}"
            );
        }

        let values = format!(
            "{}\nruntime:\n  cleanup:\n    onUninstall:\n      deletePersistentVolumeClaims: true\n",
            chart.files["examples/onprem.yaml"]
        );
        let validated = crate::test_utils::helm_template_and_validate(&chart.files, Some(&values));
        validated.assert_ok("PVC-enabled cleanup template");
        let rendered = crate::test_utils::helm_template(&chart.files, Some(&values));
        rendered.assert_ok("PVC-enabled cleanup manifest extraction");
        let docs = parse_manifest_docs(&rendered.stdout);
        let cleanup_job = docs_by_kind(&docs, "Job")
            .into_iter()
            .find(|job| {
                job.get("metadata")
                    .and_then(|metadata| metadata.get("name"))
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| name.ends_with("-cleanup"))
            })
            .expect("rendered cleanup Job");
        let script = cleanup_job
            .get("spec")
            .and_then(|spec| spec.get("template"))
            .and_then(|template| template.get("spec"))
            .and_then(|spec| spec.get("containers"))
            .and_then(YamlValue::as_sequence)
            .and_then(|containers| containers.first())
            .and_then(|container| container.get("command"))
            .and_then(YamlValue::as_sequence)
            .and_then(|command| command.get(2))
            .and_then(YamlValue::as_str)
            .expect("cleanup shell script");
        assert!(script.contains("resource_label_go_template="));
        assert!(script.contains("-o \"go-template=$resource_label_go_template\""));
        let mut child = std::process::Command::new("sh")
            .arg("-n")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("start shell syntax check");
        use std::io::Write as _;
        child
            .stdin
            .take()
            .expect("shell stdin")
            .write_all(script.as_bytes())
            .expect("write cleanup script");
        assert!(
            child.wait().expect("shell syntax result").success(),
            "cleanup Job must contain valid POSIX shell"
        );
    }

    #[test]
    fn generated_product_chart_embeds_a_distinct_remote_operator() {
        let chart = sample_product_chart();
        let branded_chart =
            sample_product_chart_with_collector_and_label_domain(false, Some("acme.dev"));
        let default_domain_chart =
            sample_product_chart_with_collector_and_label_domain(false, Some("alien.dev"));
        let display_name_chart =
            sample_product_chart_with_collector_and_label_domain(false, Some("My Cool App"));

        assert!(!chart.files.contains_key("crds/alien-access-requests.yaml"));
        assert!(
            branded_chart.files["values.yaml"].contains("deploymentLabelKey: 'acme/deployment'")
        );
        assert!(
            default_domain_chart.files["values.yaml"]
                .contains("deploymentLabelKey: 'alien.dev/deployment'"),
            "the explicit default domain must keep the runtime's qualified current label"
        );
        assert!(
            default_domain_chart.files["values.yaml"].contains("legacyDeploymentLabelKey: \"\""),
            "the explicit default domain has no distinct legacy key"
        );
        assert!(
            display_name_chart.files["values.yaml"]
                .contains("deploymentLabelKey: 'mycoolapp/deployment'"),
            "free-form brands must use the runtime's DNS-safe current label"
        );
        assert!(
            display_name_chart.files["values.yaml"].contains("legacyDeploymentLabelKey: \"\""),
            "an invalid legacy domain must not become a Kubernetes label key"
        );
        let crd_template = &chart.files["templates/remote-operator-crd.yaml"];
        assert!(crd_template.contains("if .Values.remoteOperator.enabled"));
        assert!(crd_template.contains("helm.sh/resource-policy: keep"));
        assert!(crd_template.contains("if not $existing"));
        assert!(chart.files["README.md"]
            .contains("disabled by default and adds no cluster-scoped resources until enabled"));
        assert!(chart.files["README.md"].contains("retains that CRD on rollback and uninstall"));
        assert!(chart.files["README.md"].contains("deleting its exact retained identity records"));
        let remote_template = &chart.files["templates/remote-operator.yaml"];
        assert!(chart.files["Chart.yaml"].contains("alien.dev/remote-operator-lifecycle: \"v2\""));
        assert!(remote_template.contains(".Values.remoteOperator.enabled"));
        assert!(remote_template.contains("deployment.remoteOperatorResourceName"));
        assert!(!remote_template.contains("deployment.fullname"));
        assert!(!remote_template.contains("setup-owned"));
        assert!(!remote_template.contains("kind: 'Secret'"));
        let checks = &chart.files["templates/remote-operator-checks.yaml"];
        assert!(checks.contains("Refusing adoption"));
        assert!(checks.contains("managedResourceExists"));
        assert!(checks.contains("predates the Remote Operator rollback guard"));
        assert!(
            checks.contains("predates the Remote Operator rollback guard introduced at revision")
        );
        assert!(checks.contains("perform a disabled bridge upgrade with --history-max 1"));
        assert!(checks.contains("Helm SQL storage backend is unsupported"));
        assert!(chart.files["values.yaml"].contains("helmHistoryBackend: secret"));
        assert!(checks.contains("eq .Values.remoteOperator.helmHistoryBackend \"secret\""));
        assert!(checks.contains("lookup \"v1\" \"Secret\" .Release.Namespace \"\""));
        assert!(!checks.contains("$historyStores := list"));
        let lifecycle_capability =
            &chart.files["templates/remote-operator-lifecycle-capability.yaml"];
        assert!(lifecycle_capability.contains("remote-operator-lifecycle-capability: \"v2\""));
        assert!(!lifecycle_capability.contains("helm.sh/resource-policy: keep"));
        assert!(lifecycle_capability.contains("immutable: true"));
        assert!(lifecycle_capability.contains("$firstGuardRevision := .Release.Revision"));
        assert!(lifecycle_capability.contains("$lifecycleCapability.data"));
        assert!(
            lifecycle_capability.contains("firstGuardRevision: {{ $firstGuardRevision | quote }}")
        );
        let cleanup_rbac = &chart.files["templates/remote-operator-cleanup-rbac.yaml"];
        assert!(!cleanup_rbac.contains("if .Values.remoteOperator.enabled"));
        assert!(!cleanup_rbac.contains("helm.sh/hook"));
        assert!(!cleanup_rbac.contains("before-hook-creation"));
        assert!(cleanup_rbac.contains("meta.helm.sh/release-name"));
        assert!(cleanup_rbac.contains("meta.helm.sh/release-namespace"));
        assert!(cleanup_rbac.contains("resources: [\"persistentvolumeclaims\"]"));
        assert!(cleanup_rbac.contains("resources: [\"deployments\"]"));
        assert!(!cleanup_rbac.contains("resources: [\"serviceaccounts\"]"));
        assert!(!cleanup_rbac.contains("resources: [\"roles\"]"));
        assert!(!cleanup_rbac.contains("resources: [\"rolebindings\"]"));
        assert!(!cleanup_rbac.contains("remoteOperatorLifecycleCheckName"));
        assert!(!cleanup_rbac.contains("helm.sh/resource-policy: keep"));
        let cleanup = &chart.files["templates/remote-operator-cleanup-job.yaml"];
        assert!(cleanup.contains("helm.sh/hook\": pre-delete"));
        assert!(cleanup.contains(
            "serviceAccountName: {{ include \"deployment.remoteOperatorCleanupName\" . }}"
        ));
        assert!(cleanup.contains("$identityRecord := lookup \"v1\" \"ConfigMap\""));
        assert!(cleanup.contains("$lifecycleCapability := lookup \"v1\" \"ConfigMap\""));
        assert!(cleanup.contains("if or (not .Values.remoteOperator.enabled) $lifecycleCapability"));
        assert!(!cleanup.contains("delete_lifecycle_check_rbac"));
        assert!(!cleanup.contains("delete_cleanup_rbac"));
        assert!(!cleanup.contains("lifecycle_check_name"));
        assert!(
            cleanup.contains("owned_by_this_release"),
            "same-name foreign identity storage must not block uninstall"
        );
        assert!(cleanup.contains("--ignore-not-found -o name"));
        assert!(cleanup.contains("cannot determine whether $1 $namespace/$2 exists"));
        assert!(cleanup.contains(
            "completion record $namespace/$identity_completion exists without its identity record"
        ));
        assert!(cleanup.contains("require_field deployment"));
        assert!(cleanup.contains("require_field persistentvolumeclaim"));
        assert!(cleanup.contains("identity_initialized"));
        assert!(cleanup.contains("initialized record"));
        assert!(cleanup.contains("identity_initialized_phase"));
        assert!(cleanup.contains("unknown identity phase"));
        assert!(!cleanup.contains("operator_has_started"));
        assert!(!cleanup.contains("startedAt"));
        assert!(!cleanup.contains("Retaining the prepared Remote Operator identity"));
        assert!(!cleanup.contains("claimName:"));
        assert!(cleanup.contains(
            "delete deployment \"$resource_name\" --ignore-not-found=true --cascade=foreground --wait=false"
        ));
        assert!(cleanup.contains(
            "if resource_exists configmap \"$identity_completion\"; then\n                require_field configmap \"$identity_completion\""
        ));
        for assignment in [
            "resource_name={{ include \"deployment.remoteOperatorResourceName\" . | quote }}",
            "identity_record={{ $identityRecordName | quote }}",
            "identity_initialized={{ $identityInitializedName | quote }}",
            "identity_completion={{ $identityCompletionName | quote }}",
            "identity_pvc={{ printf \"%s-identity\" $identityRecordName | quote }}",
            "lifecycle_capability={{ $lifecycleCapabilityName | quote }}",
            "remote_operator_enabled={{ .Values.remoteOperator.enabled | quote }}",
        ] {
            assert!(
                cleanup.contains(assignment),
                "cleanup script must initialize {assignment}"
            );
        }
        assert!(cleanup.contains(
            "delete persistentvolumeclaim \"$identity_pvc\" --ignore-not-found=true --wait=false"
        ));
        assert!(cleanup.contains(
            "while resource_exists deployment \"$resource_name\" || resource_exists persistentvolumeclaim \"$identity_pvc\"; do"
        ));
        assert!(cleanup.contains("durable_delete_seconds_remaining=75"));
        assert!(!cleanup.contains("cleanup_name"));
        assert!(!cleanup.contains("delete serviceaccount"));
        assert!(!cleanup.contains("delete role \"$cleanup"));
        assert!(!cleanup.contains("delete rolebinding"));
        let pvc_delete = cleanup
            .rfind("delete persistentvolumeclaim \"$identity_pvc\"")
            .expect("identity PVC deletion");
        let completion_delete = cleanup
            .rfind("delete configmap \"$identity_completion\"")
            .expect("completion deletion");
        let initialized_delete = cleanup
            .rfind("delete configmap \"$identity_initialized\"")
            .expect("initialization deletion");
        let capability_delete = cleanup
            .rfind("delete configmap \"$lifecycle_capability\"")
            .expect("capability deletion");
        let record_delete = cleanup
            .rfind("delete configmap \"$identity_record\"")
            .expect("identity record deletion");
        assert!(pvc_delete < completion_delete);
        assert!(completion_delete < initialized_delete);
        assert!(initialized_delete < capability_delete);
        assert!(capability_delete < record_delete);
        let rollback_guard = &chart.files["templates/remote-operator-rollback-guard.yaml"];
        assert!(rollback_guard.contains("helm.sh/hook\": pre-rollback"));
        assert!(rollback_guard.contains("--ignore-not-found=true --output=name"));
        assert!(rollback_guard.contains("if [ -n \"$identity_completion_resource\" ]"));
        assert!(checks.contains("missing from a partial installation"));
        assert!(checks.contains("remoteOperator.bootstrapIdentity has already been consumed"));
        assert!(checks.contains("(not .Values.remoteOperator.bootstrapIdentity)"));
        assert!(checks.contains("$encryptionKey | sha256sum"));
        assert!(checks.contains("pins credentials Secret"));
        assert!(checks.contains("pins encryption-key SHA-256"));
        assert!(checks.contains("completed Remote Operator identity"));
        assert!(checks.contains("exact prepared identity retry"));
        assert!(checks.contains("prepared Remote Operator retry may reuse only"));
        assert!(checks.contains("managed resources exist without the retained identity record"));
        assert!(checks.contains("if or .Release.IsInstall .Release.IsUpgrade"));
        assert!(checks.contains("Retained Remote Operator lifecycle records already exist"));
        assert!(checks.contains("Remote Operator cleanup authority is absent or does not match"));
        assert!(checks.contains("Perform one successful disabled bridge upgrade"));
        assert!(checks.contains("$expectedCleanupRoleRules := list"));
        assert!(checks.contains("$cleanupRoleBinding := lookup"));
        assert!(remote_template.contains("helm.sh/resource-policy: keep"));
        let identity_record = &chart.files["templates/remote-operator-identity-record.yaml"];
        assert!(identity_record.contains("alien.dev/remote-operator-identity-record"));
        assert!(identity_record.contains("deployment.remoteOperatorIdentityRecordName"));
        assert!(identity_record.contains("helm.sh/hook: pre-install,pre-upgrade"));
        assert!(identity_record.contains("helm.sh/resource-policy: keep"));
        assert!(identity_record.contains("immutable: true"));
        assert!(identity_record.contains("credentialsSecretName"));
        assert!(identity_record.contains("encryptionKeySha256"));
        let identity_initialized =
            &chart.files["templates/remote-operator-identity-initialized.yaml"];
        assert!(identity_initialized.contains("remoteOperatorIdentityInitializedName"));
        assert!(identity_initialized.contains("alien.dev/remote-operator-identity-phase: pending"));
        assert!(identity_initialized.contains("helm.sh/resource-policy: keep"));
        assert!(identity_initialized.contains("immutable: false"));
        let identity_initialized_rbac =
            &chart.files["templates/remote-operator-identity-initialized-rbac.yaml"];
        assert!(identity_initialized_rbac.contains("resources: [\"configmaps\"]"));
        assert!(identity_initialized_rbac.contains("verbs: [\"get\", \"patch\", \"update\"]"));
        assert!(identity_initialized_rbac.contains(
            "resourceNames:\n      - {{ include \"deployment.remoteOperatorIdentityInitializedName\" . }}"
        ));
        assert!(!chart
            .files
            .contains_key("templates/remote-operator-lifecycle-check-rbac.yaml"));
        let manager_role = &chart.files["templates/role.yaml"];
        assert!(!manager_role.contains("deployment.remoteOperatorLifecycleCheckName"));
        assert!(!manager_role.contains("deployment.remoteOperatorCleanupName"));
        let history_backend_check =
            &chart.files["templates/remote-operator-history-backend-check.yaml"];
        assert!(history_backend_check.contains("helm.sh/hook\": pre-upgrade"));
        assert!(history_backend_check.contains("helm.sh/hook-weight\": \"-127\""));
        assert!(history_backend_check.contains("hook-succeeded,hook-failed"));
        assert!(history_backend_check.contains("sh.helm.release.v1.%s.v%d"));
        assert!(!history_backend_check.contains("kind: Role"));
        assert!(!history_backend_check.contains("kind: ServiceAccount"));
        assert!(history_backend_check.contains("automountServiceAccountToken: false"));
        assert!(history_backend_check.contains("secretName: {{ $historyRecordName | quote }}"));
        assert!(history_backend_check.contains("name: {{ $historyRecordName | quote }}"));
        assert!(history_backend_check.contains("history_proof={{ randAlphaNum 32 | quote }}"));
        assert!(history_backend_check.contains("base64 -d /history/release | gzip -d"));
        assert!(history_backend_check.contains("this exact pending upgrade"));
        let identity_completion = &chart.files["templates/remote-operator-identity-complete.yaml"];
        assert!(identity_completion.contains("helm.sh/hook: post-install,post-upgrade"));
        assert!(identity_completion.contains("alien.dev/remote-operator-identity-phase: complete"));
        assert!(!identity_completion.contains("alien.dev/remote-operator-identity-record"));
        let identity_gate = &chart.files["templates/remote-operator-identity-gate.yaml"];
        assert!(identity_gate.contains("helm.sh/hook-weight\": \"90"));
        assert!(identity_gate.contains("deployment_ready=false"));
        assert!(identity_gate.contains("Remote Operator became ready without recording"));
        assert!(identity_gate.contains("deployment.remoteOperatorResourceName"));
        assert!(identity_gate.contains(
            "serviceAccountName: {{ include \"deployment.managerServiceAccountName\" . }}"
        ));
        assert!(remote_template.contains("name: 'OPERATOR_READINESS_PORT'"));
        assert!(remote_template.contains("name: 'OPERATOR_IDENTITY_INITIALIZED_CONFIGMAP'"));
        assert!(remote_template.contains("path: /ready"));
        assert!(remote_template.contains("port: readiness"));
        for (name, template) in [
            ("identity record", identity_record),
            ("identity initialization", identity_initialized),
            ("identity completion", identity_completion),
        ] {
            let enabled_gate = template
                .find("if .Values.remoteOperator.enabled")
                .unwrap_or_else(|| panic!("{name} must gate live lookups on enablement"));
            let first_lookup = template
                .find("lookup \"")
                .unwrap_or_else(|| panic!("{name} must contain a live lookup"));
            assert!(
                enabled_gate < first_lookup,
                "{name} must not perform a live lookup before checking enablement"
            );
        }
        let lifecycle_gate = checks
            .find("if or .Release.IsInstall .Release.IsUpgrade")
            .expect("lifecycle checks must run on every install and upgrade");
        let first_lifecycle_lookup = checks
            .find("lookup \"v1\" \"ConfigMap\"")
            .expect("lifecycle checks must perform exact ConfigMap lookups");
        assert!(lifecycle_gate < first_lifecycle_lookup);
        let cleanup_authority_gate = checks
            .find("if and .Release.IsUpgrade .Values.remoteOperator.enabled")
            .expect("enabled upgrades must verify their disabled-baseline cleanup authority");
        let cleanup_authority_lookup = checks
            .find("$cleanupServiceAccount := lookup")
            .expect("enabled upgrades must look up the cleanup ServiceAccount");
        let identity_retention_lookup = checks
            .find("$identityRecord := lookup \"v1\" \"ConfigMap\"")
            .expect("identity checks must look up the retained identity record");
        assert!(cleanup_authority_gate < cleanup_authority_lookup);
        assert!(cleanup_authority_lookup < identity_retention_lookup);
        assert!(chart.files["values.yaml"].contains("bootstrapIdentity: false"));
        let schema: serde_json::Value =
            serde_json::from_str(&chart.files["values.schema.json"]).unwrap();
        assert_eq!(
            schema["properties"]["remoteOperator"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["properties"]["remoteOperator"]["properties"]["bootstrapIdentity"]["type"],
            "boolean"
        );
        assert_eq!(
            schema["properties"]["remoteOperator"]["properties"]["existingSecret"]["properties"]
                ["encryptionKeySha256"]["maxLength"],
            64
        );
        assert_eq!(
            schema["properties"]["remoteOperator"]["properties"]["podLabels"]["propertyNames"]
                ["not"]["enum"],
            serde_json::json!(["app.kubernetes.io/name", "app.kubernetes.io/instance"])
        );

        let reserved_pod_label = crate::test_utils::helm_template(
            &chart.files,
            Some(
                r#"
remoteOperator:
  podLabels:
    app.kubernetes.io/name: overridden
"#,
            ),
        );
        assert!(matches!(
            reserved_pod_label.status,
            crate::test_utils::LinterStatus::Failed(_)
        ));
        assert!(
            reserved_pod_label.stderr.contains("app.kubernetes.io/name"),
            "Helm must identify the reserved pod label: {}",
            reserved_pod_label.stderr
        );

        crate::test_utils::helm_lint(&chart.files)
            .assert_ok("product chart with disabled Remote Operator");
        let disabled = crate::test_utils::helm_template(&chart.files, None);
        disabled.assert_ok("product chart with disabled Remote Operator");
        assert!(
            !disabled.stdout.contains("kind: CustomResourceDefinition"),
            "a namespace-only product install must not submit the cluster-scoped CRD"
        );
        let disabled_documents = parse_manifest_docs(&disabled.stdout);
        let cleanup_service_account = docs_by_kind(&disabled_documents, "ServiceAccount")
            .into_iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| name.contains("-cleanup-"))
            })
            .expect("disabled baseline cleanup ServiceAccount");
        let cleanup_name = yaml_path(&cleanup_service_account, &["metadata", "name"])
            .and_then(YamlValue::as_str)
            .expect("cleanup ServiceAccount name");
        assert!(yaml_path(
            &cleanup_service_account,
            &["metadata", "annotations", "helm.sh/hook"]
        )
        .is_none());
        let cleanup_role = docs_by_kind(&disabled_documents, "Role")
            .into_iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some(cleanup_name)
            })
            .expect("disabled baseline cleanup Role");
        let cleanup_rules = yaml_path(&cleanup_role, &["rules"])
            .and_then(YamlValue::as_sequence)
            .expect("cleanup Role rules");
        assert_eq!(cleanup_rules.len(), 3);
        assert!(cleanup_rules.iter().all(|rule| {
            yaml_path(rule, &["resources"])
                .and_then(YamlValue::as_sequence)
                .into_iter()
                .flatten()
                .filter_map(YamlValue::as_str)
                .all(|resource| !matches!(resource, "serviceaccounts" | "roles" | "rolebindings"))
        }));
        let cleanup_role_binding = docs_by_kind(&disabled_documents, "RoleBinding")
            .into_iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some(cleanup_name)
            })
            .expect("disabled baseline cleanup RoleBinding");
        let cleanup_subject_name = yaml_path(&cleanup_role_binding, &["subjects"])
            .and_then(YamlValue::as_sequence)
            .and_then(|subjects| subjects.first())
            .and_then(|subject| yaml_path(subject, &["name"]))
            .and_then(YamlValue::as_str);
        assert_eq!(cleanup_subject_name, Some(cleanup_name));
        assert_eq!(
            yaml_path(&cleanup_role_binding, &["roleRef", "name"]).and_then(YamlValue::as_str),
            Some(cleanup_name)
        );
        let cleanup_job = docs_by_kind(&disabled_documents, "Job")
            .into_iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some(cleanup_name)
            })
            .expect("disabled baseline cleanup Job");
        assert_eq!(
            yaml_path(&cleanup_job, &["metadata", "annotations", "helm.sh/hook"])
                .and_then(YamlValue::as_str),
            Some("pre-delete")
        );

        // Client-only rendering cannot satisfy the live Secret and ownership
        // lookups. The Kind lifecycle test exercises this unmodified chart
        // against the Kubernetes API.
        let mut render_files = chart.files.clone();
        render_files.shift_remove("templates/remote-operator-checks.yaml");
        let rendered = crate::test_utils::helm_template(
            &render_files,
            Some(
                r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#,
            ),
        );
        rendered.assert_ok("enabled Remote Operator product chart");
        assert!(rendered.stdout.contains("secretName: 'setup-owned'"));
        assert!(rendered.stdout.contains(
            "registry.example.com/operator@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(rendered
            .stdout
            .contains("value: 'https://manager.example.com'"));
        let documents = parse_manifest_docs(&rendered.stdout);
        assert!(documents.iter().any(|document| {
            yaml_str(document, "kind") == Some("CustomResourceDefinition")
                && yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some("alienaccessrequests.accessrequests.alien")
        }));
        let identity_record = documents
            .iter()
            .find(|document| {
                yaml_str(document, "kind") == Some("ConfigMap")
                    && yaml_path(
                        document,
                        &[
                            "metadata",
                            "labels",
                            "alien.dev/remote-operator-identity-record",
                        ],
                    )
                    .and_then(YamlValue::as_str)
                        == Some("true")
            })
            .expect("retained Remote Operator identity record");
        assert_eq!(
            yaml_path(identity_record, &["immutable"]).and_then(YamlValue::as_bool),
            Some(true)
        );
        assert_eq!(
            yaml_path(identity_record, &["data", "credentialsSecretName"])
                .and_then(YamlValue::as_str),
            Some("setup-owned")
        );
        assert_eq!(
            yaml_path(identity_record, &["data", "encryptionKeySha256"])
                .and_then(YamlValue::as_str),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        let identity_completion = documents
            .iter()
            .find(|document| {
                yaml_str(document, "kind") == Some("ConfigMap")
                    && yaml_path(
                        document,
                        &[
                            "metadata",
                            "labels",
                            "alien.dev/remote-operator-identity-phase",
                        ],
                    )
                    .and_then(YamlValue::as_str)
                        == Some("complete")
            })
            .expect("retained Remote Operator completion record");
        assert_eq!(
            yaml_path(identity_completion, &["data", "identityRecordName"])
                .and_then(YamlValue::as_str),
            Some("test-release-remote-operator-ab7b1d5677627240")
        );
        assert!(
            yaml_path(
                identity_completion,
                &[
                    "metadata",
                    "labels",
                    "alien.dev/remote-operator-identity-record"
                ]
            )
            .is_none(),
            "the UI marker selector belongs only on the prepared identity pin"
        );
        let remote_deployment = documents
            .iter()
            .find(|document| {
                yaml_str(document, "kind") == Some("Deployment")
                    && yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                        == Some("test-release-remote-operator-ab7b1d5677627240")
            })
            .expect("release-specific Remote Operator Deployment");
        assert_eq!(
            yaml_path(
                remote_deployment,
                &["metadata", "labels", "app.kubernetes.io/managed-by"]
            )
            .and_then(YamlValue::as_str),
            Some("Helm")
        );
        assert_eq!(
            yaml_path(
                remote_deployment,
                &[
                    "spec",
                    "template",
                    "metadata",
                    "labels",
                    "app.kubernetes.io/instance"
                ]
            )
            .and_then(YamlValue::as_str),
            Some("test-release-remote-operator-ab7b1d5677627240")
        );
        assert!(documents.iter().any(|document| {
            yaml_str(document, "kind") == Some("ServiceAccount")
                && yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some("test-release-remote-operator-ab7b1d5677627240")
        }));
    }

    #[test]
    fn product_collector_follows_branded_runtime_scope() {
        let mut files =
            sample_product_chart_with_collector_and_label_domain(true, Some("acme.dev")).files;
        // Client-only rendering has no live Secret or Helm history for this guard to inspect.
        files.shift_remove("templates/remote-operator-checks.yaml");
        let values = r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#;
        let rendered =
            crate::test_utils::helm_template_for_release(&files, Some(values), "customer-one");
        rendered.assert_ok("branded product collector scope");
        let docs = parse_manifest_docs(&rendered.stdout);
        let runtime = docs_by_kind(&docs, "Deployment")
            .into_iter()
            .find(|deployment| {
                yaml_path(deployment, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some("customer-one")
            })
            .expect("runtime Deployment");
        let label_key = operator_env_value(&runtime, "ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY")
            .expect("runtime deployment label key");
        let label_value = operator_env_value(&runtime, "ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE")
            .expect("runtime deployment label value");
        let resource_prefix = operator_env_value(&runtime, "OPERATOR_RESOURCE_PREFIX")
            .expect("runtime resource prefix");
        assert_eq!(resource_prefix, "customer-one");
        assert_eq!(
            (label_key, label_value),
            ("acme/deployment", "customer-one")
        );

        let collector = docs_by_kind(&docs, "ConfigMap")
            .into_iter()
            .find(|config| yaml_path(config, &["data", "collector.conf"]).is_some())
            .expect("Remote Operator collector ConfigMap");
        let config = yaml_path(&collector, &["data", "collector.conf"])
            .and_then(YamlValue::as_str)
            .expect("Fluent Bit configuration");
        assert!(config.contains(&format!(
            "Regex               $kubernetes['labels']['{label_key}'] ^{label_value}$"
        )));
        assert!(!config.contains("$kubernetes['labels']['alien.dev/deployment']"));
    }

    #[test]
    fn product_chart_omits_cleanup_job_on_first_enabled_render_without_baseline_lifecycle_capability(
    ) {
        let mut files = sample_product_chart().files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let rendered = crate::test_utils::helm_template(
            &files,
            Some(
                r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#,
            ),
        );
        rendered.assert_ok("product chart without retained Remote Operator identity");
        let documents = parse_manifest_docs(&rendered.stdout);
        let cleanup_service_account = docs_by_kind(&documents, "ServiceAccount")
            .into_iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| name.contains("-cleanup-"))
            })
            .expect("ordinary cleanup ServiceAccount");
        let cleanup_name = yaml_path(&cleanup_service_account, &["metadata", "name"])
            .and_then(YamlValue::as_str)
            .expect("cleanup ServiceAccount name");
        assert!(yaml_path(
            &cleanup_service_account,
            &["metadata", "annotations", "helm.sh/hook"]
        )
        .is_none());
        assert!(
            !docs_by_kind(&documents, "Job").iter().any(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
                    == Some(cleanup_name)
            }),
            "the first enabled render must wait for the disabled baseline lifecycle capability before rendering the cleanup Job"
        );
        assert!(documents.iter().any(|document| {
            yaml_str(document, "kind") == Some("ConfigMap")
                && yaml_path(
                    document,
                    &[
                        "metadata",
                        "labels",
                        "alien.dev/remote-operator-identity-record",
                    ],
                )
                .and_then(YamlValue::as_str)
                    == Some("true")
        }));
    }

    #[test]
    fn product_chart_fullname_override_cannot_hide_completed_remote_identity() {
        let chart = sample_product_chart();
        let identity_record = &chart.files["templates/remote-operator-identity-record.yaml"];
        let checks = &chart.files["templates/remote-operator-checks.yaml"];

        assert!(identity_record.contains(".Release.Namespace .Release.Name | sha256sum"));
        assert!(identity_record.contains("deployment.remoteOperatorReleaseIdentity"));
        assert!(!chart.files["templates/remote-operator.yaml"].contains("deployment.fullname"));
        assert!(checks.contains("$identityCompletion := lookup"));
        assert!(checks.contains("identity volume is missing from a partial installation"));

        let render = |fullname_override: &str| {
            let mut files = chart.files.clone();
            files.shift_remove("templates/remote-operator-checks.yaml");
            let values = format!(
                r#"
fullnameOverride: {fullname_override}
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#
            );
            let rendered = crate::test_utils::helm_template(&files, Some(&values));
            rendered.assert_ok("Remote Operator name ignores product fullnameOverride");
            rendered.stdout
        };

        let before = render("first-product-name");
        let after = render("replacement-product-name");
        let stable_name = "test-release-remote-operator-ab7b1d5677627240";
        for manifest in [&before, &after] {
            assert!(manifest.contains(&format!("name: {stable_name}")));
            assert!(manifest.contains(&format!("identityRecordName: \"{stable_name}\"")));
            assert!(manifest.contains(&format!("name: {stable_name}-complete")));
        }
    }

    #[test]
    fn product_chart_normalizes_a_dotted_release_name_for_remote_operator_resources() {
        let mut files = sample_product_chart().files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let values = r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#;

        crate::test_utils::helm_template_and_validate_for_release(
            &files,
            Some(values),
            "leading.middle.trailing",
        )
        .assert_ok("product Remote Operator with a dotted Helm release name");

        let rendered = crate::test_utils::helm_template_for_release(
            &files,
            Some(values),
            "leading.middle.trailing",
        );
        rendered.assert_ok("render product Remote Operator with a dotted Helm release name");
        assert!(rendered
            .stdout
            .contains("name: leading-middle-traili-remote-operator-"));
        assert!(!rendered
            .stdout
            .contains("name: leading.middle.trailing-remote-operator-"));
    }

    #[test]
    fn product_chart_reserves_the_cleanup_suffix_for_long_release_names() {
        let mut files = sample_product_chart().files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let values = r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#;
        let release_name = "a".repeat(53);
        let rendered =
            crate::test_utils::helm_template_for_release(&files, Some(values), &release_name);
        rendered.assert_ok("product Remote Operator with a maximum-length Helm release name");

        let documents = parse_manifest_docs(&rendered.stdout);
        let service_accounts = docs_by_kind(&documents, "ServiceAccount");
        let operator_name = service_accounts
            .iter()
            .find(|document| {
                yaml_path(document, &["metadata", "annotations", "helm.sh/hook"]).is_none()
                    && yaml_path(document, &["metadata", "name"])
                        .and_then(YamlValue::as_str)
                        .is_some_and(|name| name.contains("-remote-operator-"))
            })
            .and_then(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
            })
            .expect("Remote Operator ServiceAccount");
        let cleanup_name = service_accounts
            .iter()
            .find(|document| {
                yaml_path(document, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| name.contains("-cleanup-"))
            })
            .and_then(|document| {
                yaml_path(document, &["metadata", "name"]).and_then(YamlValue::as_str)
            })
            .expect("ordinary Remote Operator cleanup ServiceAccount");

        assert_eq!(operator_name.len(), 54);
        assert_eq!(cleanup_name.len(), 55);
        assert!(cleanup_name.contains("-cleanup-"));
        assert_ne!(cleanup_name, operator_name);
    }

    #[test]
    fn product_chart_identity_initialization_rbac_names_keep_release_identity_hash() {
        let mut files = sample_product_chart().files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let values = r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#;
        let render_names = |release_name: &str| {
            let rendered =
                crate::test_utils::helm_template_for_release(&files, Some(values), release_name);
            rendered.assert_ok("product Remote Operator identity-initialization RBAC");
            let documents = parse_manifest_docs(&rendered.stdout);
            let role = docs_by_kind(&documents, "Role")
                .into_iter()
                .find(|document| {
                    yaml_path(document, &["metadata", "name"])
                        .and_then(YamlValue::as_str)
                        .is_some_and(|name| name.contains("-identity-init-"))
                })
                .expect("identity-initialization Role");
            let role_binding = docs_by_kind(&documents, "RoleBinding")
                .into_iter()
                .find(|document| {
                    yaml_path(document, &["metadata", "name"])
                        .and_then(YamlValue::as_str)
                        .is_some_and(|name| name.contains("-identity-init-"))
                })
                .expect("identity-initialization RoleBinding");
            (
                yaml_path(&role, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .expect("identity-initialization Role name")
                    .to_string(),
                yaml_path(&role_binding, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .expect("identity-initialization RoleBinding name")
                    .to_string(),
                yaml_path(&role_binding, &["roleRef", "name"])
                    .and_then(YamlValue::as_str)
                    .expect("identity-initialization roleRef name")
                    .to_string(),
            )
        };

        let shared_prefix = "a".repeat(51);
        let first = render_names(&format!("{shared_prefix}aa"));
        let second = render_names(&format!("{shared_prefix}ab"));
        assert_eq!(first.0, first.1);
        assert_eq!(first.0, first.2);
        assert_eq!(second.0, second.1);
        assert_eq!(second.0, second.2);
        assert!(first.0.len() <= 63);
        assert!(second.0.len() <= 63);
        assert_ne!(first.0, second.0);
    }

    #[test]
    fn product_chart_collector_names_fit_kubernetes_limits_and_keep_release_identity() {
        let mut files = sample_product_chart_with_collector(true).files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let values = r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#;
        let render_collector_name = |release_name: &str| {
            let rendered =
                crate::test_utils::helm_template_for_release(&files, Some(values), release_name);
            rendered.assert_ok("product Remote Operator log collector");
            let documents = parse_manifest_docs(&rendered.stdout);
            let collector_name = docs_by_kind(&documents, "DaemonSet")
                .into_iter()
                .find_map(|document| {
                    yaml_path(&document, &["metadata", "name"])
                        .and_then(YamlValue::as_str)
                        .filter(|name| name.contains("-log-collector-"))
                        .map(str::to_string)
                })
                .expect("Remote Operator log-collector DaemonSet");
            assert!(
                collector_name.len() <= 63,
                "rendered collector name is too long: {collector_name}"
            );
            collector_name
        };

        let shared_prefix = "a".repeat(51);
        let first = render_collector_name(&format!("{shared_prefix}aa"));
        let second = render_collector_name(&format!("{shared_prefix}ab"));
        assert_ne!(first, second);
    }

    #[test]
    fn product_chart_remote_operator_removal_requires_the_exact_release_name() {
        let chart = sample_product_chart();
        let render = |values: &str| {
            crate::test_utils::helm_template_for_release(&chart.files, Some(values), "shop")
        };
        let has_rollback_guard = |manifest: &str| {
            docs_by_kind(&parse_manifest_docs(manifest), "Job")
                .iter()
                .any(|job| {
                    yaml_path(job, &["metadata", "annotations", "helm.sh/hook"])
                        .and_then(YamlValue::as_str)
                        == Some("pre-rollback")
                })
        };

        let wrong_release = render("remoteOperator:\n  confirmRemoval: other-release\n");
        assert!(!wrong_release.is_ok(), "{wrong_release:?}");
        assert!(
            wrong_release.stderr.contains(
                r#"remoteOperator.confirmRemoval is "other-release", but this release is "shop""#
            ),
            "{}",
            wrong_release.stderr
        );

        let enabled_with_confirmation = render(
            r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  confirmRemoval: shop
  existingSecret:
    name: setup-owned
    encryptionKeySha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
"#,
        );
        assert!(!enabled_with_confirmation.is_ok());
        assert!(
            enabled_with_confirmation.stderr.contains(
                "remoteOperator.confirmRemoval must be empty when Remote Operator is enabled"
            ),
            "{}",
            enabled_with_confirmation.stderr
        );

        // A plain disabled revision refuses rollback over a completed identity.
        let disabled = render("remoteOperator:\n  enabled: false\n");
        disabled.assert_ok("disabled product chart");
        assert!(has_rollback_guard(&disabled.stdout));

        // A confirmation with no completed identity to remove (here, no
        // cluster at all) keeps the rollback guard, so an install or bridge
        // upgrade confirmed early never becomes a guard-free rollback target.
        // The Kind lifecycle test covers removal of a completed identity.
        let removed = render("remoteOperator:\n  enabled: false\n  confirmRemoval: shop\n");
        removed.assert_ok("product chart with the Remote Operator removed");
        let documents = parse_manifest_docs(&removed.stdout);
        assert!(has_rollback_guard(&removed.stdout));
        assert!(docs_by_kind(&documents, "Deployment")
            .iter()
            .all(|document| {
                yaml_path(document, &["metadata", "name"])
                    .and_then(YamlValue::as_str)
                    .is_some_and(|name| !name.contains("-remote-operator-"))
            }));
        assert!(docs_by_kind(&documents, "Job").iter().any(|job| {
            yaml_path(job, &["metadata", "annotations", "helm.sh/hook"]).and_then(YamlValue::as_str)
                == Some("pre-delete")
        }));
        assert_eq!(
            documents,
            parse_manifest_docs(&disabled.stdout),
            "removal must leave the product resources exactly as a disabled render"
        );
    }

    #[test]
    fn product_chart_rejects_invalid_encryption_key_fingerprint() {
        let mut files = sample_product_chart().files;
        files.shift_remove("templates/remote-operator-checks.yaml");
        let rendered = crate::test_utils::helm_template(
            &files,
            Some(
                r#"
management:
  url: https://manager.example.com
remoteOperator:
  enabled: true
  existingSecret:
    name: setup-owned
    encryptionKeySha256: not-a-sha256
"#,
            ),
        );

        assert!(
            !rendered.is_ok(),
            "enabled Remote Operator values must reject a malformed encryption-key fingerprint"
        );
        assert!(
            format!("{}\n{}", rendered.stdout, rendered.stderr).contains("encryptionKeySha256"),
            "schema diagnostic must identify the malformed fingerprint: {rendered:?}"
        );
    }

    #[test]
    fn product_chart_requires_non_empty_decoded_external_credentials() {
        let checks = remote_operator_checks_tpl(true);

        for required_check in [
            "$syncToken := index $credentials.data \"sync-token\" | b64dec",
            "$encryptionKey := index $credentials.data \"encryption-key\" | b64dec",
            "if or (empty $syncToken) (empty $encryptionKey)",
            "$collectorToken = index $credentials.data \"collector-token\" | b64dec",
            "if empty $collectorToken",
        ] {
            assert!(
                checks.contains(required_check),
                "server-side Secret validation must include: {required_check}"
            );
        }
        assert!(checks.contains("non-empty sync-token and encryption-key values"));
        assert!(checks.contains("non-empty collector-token"));
    }

    #[test]
    fn product_chart_allows_prepared_identity_rollback_to_clean_partial_resources() {
        let chart = sample_product_chart();
        let checks = &chart.files["templates/remote-operator-checks.yaml"];

        assert!(
            checks.contains(
                "$preparedIdentity := and $identityRecord (not $identityCompletion)"
            ) && checks.contains(
                "$safePreparedRollback := and (not .Values.remoteOperator.enabled) $preparedIdentity"
            ) && checks.contains("(not $safePreparedRollback)"),
            "disabled rollback must allow an exact-owned prepared installation to clean partial resources"
        );
    }

    #[test]
    fn product_chart_allows_prepared_retry_with_zero_resources_or_only_the_identity_pvc() {
        let chart = sample_product_chart();
        let checks = &chart.files["templates/remote-operator-checks.yaml"];

        assert!(checks
            .contains("$preparedRetry := and .Values.remoteOperator.enabled $preparedIdentity"));
        assert!(checks
            .contains("if and $preparedRetry (get $identityState \"otherManagedResourceExists\")"));
        assert!(checks.contains(
            "A prepared Remote Operator retry may reuse only its exact-release owned retained identity PVC"
        ));
        assert!(
            !checks.contains(
                "and $preparedRetry (not (get $identityState \"managedResourceExists\"))"
            ),
            "a prepared retry must remain live when the pre-hook succeeded before any managed resource was created"
        );
    }

    #[test]
    fn product_chart_uninstall_rejects_a_foreign_remote_operator_deployment() {
        let cleanup = remote_operator_cleanup_job_tpl();

        assert!(
            cleanup.contains(
                "if resource_exists deployment \"$resource_name\"; then\n                require_field deployment \"$resource_name\" '{.metadata.annotations.meta\\.helm\\.sh/release-name}' \"$release_name\" release-name"
            ) && cleanup.contains(
                "require_field deployment \"$resource_name\" '{.metadata.annotations.meta\\.helm\\.sh/release-namespace}' \"$namespace\" release-namespace"
            ) && cleanup.contains(
                "require_field deployment \"$resource_name\" '{.metadata.labels.app\\.kubernetes\\.io/managed-by}' \"$release_service\" managed-by"
            ) && cleanup.contains(
                "require_field deployment \"$resource_name\" '{.metadata.labels.app\\.kubernetes\\.io/instance}' \"$resource_name\" instance"
            ),
            "the retained identity path must reject a same-name foreign Deployment before Helm deletes it"
        );
    }

    #[test]
    fn product_chart_rejects_a_forged_mutable_prepared_marker() {
        let chart = sample_product_chart();
        let checks = &chart.files["templates/remote-operator-checks.yaml"];

        for required_contract in [
            "(not (default false $identityRecord.immutable))",
            "(ne (index $recordAnnotations \"helm.sh/hook\") \"pre-install,pre-upgrade\")",
            "(ne (index $recordAnnotations \"helm.sh/hook-weight\") \"-100\")",
            "(ne (index $recordAnnotations \"helm.sh/resource-policy\") \"keep\")",
            "(ne (index $recordLabels \"app.kubernetes.io/instance\") .Release.Name)",
            "(ne (len $recordData) 3)",
            "(ne (index $recordData \"version\") \"3\")",
        ] {
            assert!(
                checks.contains(required_contract),
                "prepared marker contract must reject a forged mutable record: {required_contract}"
            );
        }
        assert!(checks.contains("does not match the immutable prepared identity-record contract"));
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
        assert!(rendered
            .stdout
            .contains("app.kubernetes.io/component: log-collector"));
        let documents = parse_manifest_docs(&rendered.stdout);
        let generated_values = values.replace("  token: test-collector-token\n", "");
        let generated = crate::test_utils::helm_template(&files, Some(&generated_values));
        generated.assert_ok("Helm generates a collector credential on first install");
        let generated_documents = parse_manifest_docs(&generated.stdout);
        let generated_token = docs_by_kind(&generated_documents, "Secret")
            .into_iter()
            .find_map(|secret| {
                secret["stringData"]["collector-token"]
                    .as_str()
                    .map(str::to_owned)
            })
            .expect("generated collector credential");
        assert_eq!(generated_token.len(), 48);
        assert!(generated_token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric()));
        let credential_checksum = |docs: &[YamlValue], kind: &str| {
            docs_by_kind(docs, kind)
                .into_iter()
                .find_map(|document| {
                    document["spec"]["template"]["metadata"]["annotations"]
                        ["checksum/collector-credential"]
                        .as_str()
                        .map(str::to_owned)
                })
                .expect("collector credential Pod checksum")
        };
        for kind in ["Deployment", "DaemonSet"] {
            assert_ne!(
                credential_checksum(&documents, kind),
                credential_checksum(&generated_documents, kind),
                "an explicit collector credential must roll the {kind}"
            );
        }
        let collector_daemonset = docs_by_kind(&documents, "DaemonSet")
            .into_iter()
            .next()
            .expect("collector DaemonSet");
        let pod_spec = &collector_daemonset["spec"]["template"]["spec"];
        assert_eq!(
            pod_spec["securityContext"]["seccompProfile"]["type"],
            "RuntimeDefault"
        );
        let container_security = &pod_spec["containers"][0]["securityContext"];
        assert_eq!(container_security["allowPrivilegeEscalation"], false);
        assert_eq!(container_security["readOnlyRootFilesystem"], true);
        assert_eq!(container_security["capabilities"]["drop"][0], "ALL");
        let collector_config = docs_by_kind(&documents, "ConfigMap")
            .into_iter()
            .find(|document| {
                document["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-logs"))
            })
            .expect("collector ConfigMap");
        let fluent_bit_config = collector_config["data"]["collector.conf"]
            .as_str()
            .expect("Fluent Bit configuration");
        assert_eq!(
            fluent_bit_config
                .lines()
                .filter(|line| line.trim() == "[FILTER]")
                .count(),
            3,
            "collector filters must each start on their own line"
        );
        assert!(rendered.stdout.contains("COLLECTOR_TOKEN_FILE"));
        assert!(rendered
            .stdout
            .contains("/var/log/containers/*_default_*.log"));
        assert!(rendered.stdout.contains("fluent/fluent-bit:3.2"));
        assert!(rendered
            .stdout
            .contains("$kubernetes['labels']['alien.dev/deployment'] ^e2e123$"));
        assert!(rendered.stdout.contains("resources: [\"pods\"]"));
        assert!(rendered
            .stdout
            .contains("verbs: [\"get\", \"list\", \"watch\"]"));
        // The operator grants `pods/log` for the on-demand `logs` operation. In
        // the helm chart the operator Role folds it into the core `""`-group
        // rule (see `role_tpl`), so it appears bundled with the other pod-level
        // resources rather than as a standalone rule.
        assert!(rendered
            .stdout
            .contains("\"pods\", \"pods/log\", \"persistentvolumeclaims\""));
        assert!(!rendered.stdout.contains("void"));

        let long_release = format!("sample-{}", "a".repeat(46));
        let long_rendered =
            crate::test_utils::helm_template_for_release(&files, Some(values), &long_release);
        long_rendered.assert_ok("helm render log collector with long release name");
        let long_docs = parse_manifest_docs(&long_rendered.stdout);
        let collector_names = long_docs
            .iter()
            .filter(|document| {
                [
                    "ServiceAccount",
                    "Role",
                    "RoleBinding",
                    "ConfigMap",
                    "DaemonSet",
                ]
                .contains(&yaml_str(document, "kind").unwrap_or_default())
                    && document["metadata"]["labels"]["app.kubernetes.io/component"]
                        == "log-collector"
            })
            .map(|document| {
                document["metadata"]["name"]
                    .as_str()
                    .expect("collector resource name")
            })
            .collect::<Vec<_>>();
        assert_eq!(collector_names.len(), 5);
        assert!(collector_names.iter().all(|name| name.len() <= 63));
        assert!(collector_names
            .iter()
            .all(|name| *name == collector_names[0]));
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
                    legacy_remote_bindings_access: None,
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

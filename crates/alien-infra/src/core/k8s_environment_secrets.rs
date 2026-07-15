use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
use alien_core::EnvironmentVariableType;
use alien_core::{EnvironmentVariable, ENV_ALIEN_SECRETS};
use alien_error::{Context, ContextError};
use k8s_openapi::api::core::v1::{EnvVar, EnvVarSource, Secret, SecretKeySelector};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;
use serde::{Deserialize, Serialize};

use crate::core::environment_variables::applicable_secret_environment_variables;
use crate::core::k8s_secret_bindings::extract_binding_secrets;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

#[derive(Debug, Clone)]
pub struct KubernetesEnvSecretPlan {
    pub secret_name: String,
    pub checksum: String,
    pub keys: Vec<String>,
}

fn environment_secret_values(
    resource_id: &str,
    variables: &[EnvironmentVariable],
    additional_secrets: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut values = applicable_secret_environment_variables(resource_id, variables)
        .into_iter()
        .map(|var| (var.name.clone(), var.value.clone()))
        .collect::<BTreeMap<_, _>>();
    values.extend(additional_secrets.clone());
    values
}

fn secret_checksum(secret_values: &BTreeMap<String, String>) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for (name, value) in secret_values {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"\n");
    }

    format!("{:x}", hasher.finalize())
}

/// Pure derivation of the per-workload environment Secret plan from the
/// deployment env snapshot: the Secret name (`{workload}-env`), the checksum
/// that rolls pods (and drives update detection when only secret values
/// change), and the keys the workload manifest renders as secretKeyRefs.
///
/// Returns `None` when no Secret-typed env vars target the resource.
pub fn environment_secret_plan(
    resource_id: &str,
    workload_name: &str,
    variables: &[EnvironmentVariable],
) -> Option<KubernetesEnvSecretPlan> {
    environment_secret_plan_with_additional_secrets(
        resource_id,
        workload_name,
        variables,
        &BTreeMap::new(),
    )
}

/// Derives a workload Secret plan that also includes controller-owned values
/// read directly from `DeploymentConfig` at provisioning time.
pub fn environment_secret_plan_with_additional_secrets(
    resource_id: &str,
    workload_name: &str,
    variables: &[EnvironmentVariable],
    additional_secrets: &BTreeMap<String, String>,
) -> Option<KubernetesEnvSecretPlan> {
    let secret_values = environment_secret_values(resource_id, variables, additional_secrets);
    if secret_values.is_empty() {
        return None;
    }

    Some(KubernetesEnvSecretPlan {
        secret_name: format!("{workload_name}-env"),
        checksum: secret_checksum(&secret_values),
        keys: secret_values.keys().cloned().collect(),
    })
}

/// Builds the typed Kubernetes Secret manifest holding the plan's key/value
/// pairs, stamped with the plan checksum.
fn environment_secret_manifest(
    plan: &KubernetesEnvSecretPlan,
    resource_id: &str,
    namespace: &str,
    secret_values: &BTreeMap<String, String>,
) -> Secret {
    Secret {
        metadata: ObjectMeta {
            name: Some(plan.secret_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(BTreeMap::from([
                ("managed-by".to_string(), "runtime".to_string()),
                ("resource-id".to_string(), resource_id.to_string()),
            ])),
            annotations: Some(BTreeMap::from([(
                "env-secret-checksum".to_string(),
                plan.checksum.clone(),
            )])),
            ..Default::default()
        },
        type_: Some("Opaque".to_string()),
        data: Some(
            secret_values
                .iter()
                .map(|(name, value)| (name.clone(), ByteString(value.as_bytes().to_vec())))
                .collect(),
        ),
        ..Default::default()
    }
}

fn environment_secret_is_owned(secret: &Secret, resource_id: &str) -> bool {
    secret.metadata.labels.as_ref().is_some_and(|labels| {
        labels.get("managed-by").map(String::as_str) == Some("runtime")
            && labels.get("resource-id").map(String::as_str) == Some(resource_id)
    })
}

fn ensure_environment_secret_is_owned(
    secret: &Secret,
    secret_name: &str,
    resource_id: &str,
) -> Result<()> {
    if environment_secret_is_owned(secret, resource_id) {
        return Ok(());
    }

    Err(alien_error::AlienError::new(
        ErrorData::ResourceConfigInvalid {
            message: format!(
                "Refusing to mutate Kubernetes Secret '{secret_name}' because it is not owned by resource '{resource_id}'"
            ),
            resource_id: Some(resource_id.to_string()),
        },
    ))
}

/// Deletes the controller-owned per-workload environment Secret. Missing
/// Secrets are already deleted; a same-name Secret without our ownership
/// labels is never touched.
pub async fn delete_environment_secret(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    let secret_name = format!("{workload_name}-env");
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let secrets_client = ctx
        .service_provider
        .get_kubernetes_secrets_client(kubernetes_config)
        .await?;

    let existing = match secrets_client.get_secret(namespace, &secret_name).await {
        Ok(secret) => secret,
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(());
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read environment Secret before deleting {resource_kind} '{resource_id}'"
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }
    };
    if !environment_secret_is_owned(&existing, resource_id) {
        tracing::debug!(
            secret_name = %secret_name,
            resource_id = %resource_id,
            "Leaving same-name Kubernetes Secret untouched because it is not owned by this workload"
        );
        return Ok(());
    }

    match secrets_client.delete_secret(namespace, &secret_name).await {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to delete environment Secret for {resource_kind} '{resource_id}'"
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

pub async fn reconcile_environment_secret(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<KubernetesEnvSecretPlan>> {
    reconcile_environment_secret_with_additional_secrets(
        resource_kind,
        resource_id,
        workload_name,
        namespace,
        &BTreeMap::new(),
        ctx,
    )
    .await
}

pub async fn reconcile_environment_secret_with_additional_secrets(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    additional_secrets: &BTreeMap<String, String>,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<KubernetesEnvSecretPlan>> {
    let variables = &ctx.deployment_config.environment_variables.variables;
    let Some(plan) = environment_secret_plan_with_additional_secrets(
        resource_id,
        workload_name,
        variables,
        additional_secrets,
    ) else {
        delete_environment_secret(resource_kind, resource_id, workload_name, namespace, ctx)
            .await?;
        return Ok(None);
    };

    let secret_values = environment_secret_values(resource_id, variables, additional_secrets);
    let mut secret = environment_secret_manifest(&plan, resource_id, namespace, &secret_values);
    let secret_name = plan.secret_name.clone();

    let kubernetes_config = ctx.get_kubernetes_config()?;
    let secrets_client = ctx
        .service_provider
        .get_kubernetes_secrets_client(kubernetes_config)
        .await?;

    match secrets_client.create_secret(namespace, &secret).await {
        Ok(_) => {}
        Err(e) => {
            let err = format!("{e}");
            if err.contains("AlreadyExists") || err.contains("409") {
                let existing = secrets_client
                    .get_secret(namespace, &secret_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to read existing environment Secret for {resource_kind} '{resource_id}'",
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })?;
                ensure_environment_secret_is_owned(&existing, &secret_name, resource_id)?;
                secret.metadata.resource_version = existing.metadata.resource_version;
                secrets_client
                    .update_secret(namespace, &secret_name, &secret)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to update environment Secret for {resource_kind} '{resource_id}'",
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })?;
            } else {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create environment Secret for {resource_kind} '{resource_id}'",
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
        }
    }

    Ok(Some(plan))
}

/// Builds an `EnvVar` that resolves from a Kubernetes Secret key at pod start.
fn secret_key_ref_env_var(name: &str, secret_name: &str, secret_key: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: None,
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                name: secret_name.to_string(),
                key: secret_key.to_string(),
                optional: Some(false),
            }),
            ..Default::default()
        }),
    }
}

/// Projects a workload's resolved environment into the typed `EnvVar` list a
/// Kubernetes pod template carries. Shared by the Container, Daemon, and Worker
/// controllers so the projection rules live in exactly one place.
///
/// Three inputs are merged, in priority order:
/// 1. `plan` keys — Secret-typed env vars scoped to the workload, each rendered
///    as a `secretKeyRef` into the per-workload `{workload}-env` Secret.
/// 2. `bindings` — linked-resource binding JSON; any embedded `secretRef` is
///    projected as its own `secretKeyRef` and the binding is emitted as
///    `ALIEN_<NAME>_BINDING` with `$(VAR)` placeholders. Extraction failures
///    propagate (fail fast) rather than silently dropping the binding.
/// 3. `env_map` — the remaining plain env vars; a name already projected as a
///    secret above wins and is never overwritten with an inline value.
///
/// The legacy `ALIEN_SECRETS` vault-load pointer is dropped from `env_map`.
/// Kubernetes workloads project secrets natively via `secretKeyRef`, so the
/// pointer must never reach a pod manifest. This also covers configs injected
/// by older managers that still collapsed secrets into that pointer.
pub fn projected_env_vars(
    plan: Option<&KubernetesEnvSecretPlan>,
    bindings: Vec<(String, serde_json::Value)>,
    env_map: HashMap<String, String>,
) -> Result<Vec<EnvVar>> {
    let mut env_vars = Vec::new();

    if let Some(plan) = plan {
        for key in &plan.keys {
            env_vars.push(secret_key_ref_env_var(key, &plan.secret_name, key));
        }
    }

    for (binding_name, binding_json) in bindings {
        let extraction = extract_binding_secrets(&binding_name, &binding_json).context(
            ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Failed to project secret references from binding '{binding_name}'"
                ),
                resource_id: Some(binding_name.clone()),
            },
        )?;

        for (env_name, secret_name, secret_key) in extraction.secret_env_vars {
            env_vars.push(secret_key_ref_env_var(&env_name, &secret_name, &secret_key));
        }

        let env_key = format!(
            "ALIEN_{}_BINDING",
            binding_name.to_uppercase().replace('-', "_")
        );
        env_vars.push(EnvVar {
            name: env_key,
            value: Some(extraction.resolved_binding_json),
            value_from: None,
        });
    }

    for (key, value) in env_map {
        if key == ENV_ALIEN_SECRETS {
            continue;
        }
        if !env_vars.iter().any(|ev| ev.name == key) {
            env_vars.push(EnvVar {
                name: key,
                value: Some(value),
                value_from: None,
            });
        }
    }

    Ok(env_vars)
}

/// Tracks the checksum of the environment Secret applied last (create/update).
///
/// Secret-typed env vars never enter the resource config on Kubernetes — they
/// are projected via `secretKeyRef` — so config diffing alone cannot see secret
/// rotations. `drifted` compares the snapshot-derived checksum against the one
/// recorded last so a controller's `needs_update` can schedule an update that
/// re-reconciles the Secret and rolls pods via the pod-template checksum
/// annotation. Serializes transparently as the bare checksum so controller
/// state keeps the same on-disk shape as a plain `Option<String>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnvSecretRotationTracker {
    checksum: Option<String>,
}

impl EnvSecretRotationTracker {
    /// Records the checksum applied by the latest create/update reconcile.
    pub fn record(&mut self, plan: Option<&KubernetesEnvSecretPlan>) {
        self.checksum = plan.map(|plan| plan.checksum.clone());
    }

    /// Returns true when the current env snapshot would derive a different
    /// env-secret checksum than the one recorded last (i.e. a secret rotated,
    /// appeared, or was removed).
    pub fn drifted(
        &self,
        resource_id: &str,
        workload_name: &str,
        variables: &[EnvironmentVariable],
    ) -> bool {
        self.drifted_with_additional_secrets(
            resource_id,
            workload_name,
            variables,
            &BTreeMap::new(),
        )
    }

    pub fn drifted_with_additional_secrets(
        &self,
        resource_id: &str,
        workload_name: &str,
        variables: &[EnvironmentVariable],
        additional_secrets: &BTreeMap<String, String>,
    ) -> bool {
        let current = environment_secret_plan_with_additional_secrets(
            resource_id,
            workload_name,
            variables,
            additional_secrets,
        )
        .map(|plan| plan.checksum);
        current != self.checksum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kubernetes_manifest_test_support::KubernetesManifestTestHarness;
    use crate::core::{
        direct_monitoring_auth_headers, MockPlatformServiceProvider, OTEL_EXPORTER_OTLP_HEADERS,
        OTEL_EXPORTER_OTLP_METRICS_HEADERS,
    };
    use alien_core::{OtlpConfig, Resource, Vault, ENV_ALIEN_COMMANDS_TOKEN};
    use alien_k8s_clients::secrets::{MockSecretsApi, SecretsApi};
    use std::sync::Arc;

    fn secret_var(name: &str, value: &str, targets: Option<Vec<&str>>) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: targets
                .map(|targets| targets.into_iter().map(str::to_string).collect()),
        }
    }

    fn plain_var(name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        }
    }

    #[test]
    fn plan_is_none_without_applicable_secrets() {
        let variables = vec![
            plain_var("APP_ENV", "prod"),
            secret_var("OTHER_SECRET", "v", Some(vec!["other"])),
        ];

        assert!(environment_secret_plan("web", "web", &variables).is_none());
    }

    #[test]
    fn plan_collects_applicable_secret_keys_and_checksum() {
        let variables = vec![
            plain_var("APP_ENV", "prod"),
            secret_var("APP_SECRET", "s3cret", None),
            secret_var(ENV_ALIEN_COMMANDS_TOKEN, "tok", Some(vec!["web"])),
            secret_var("OTHER_SECRET", "v", Some(vec!["other"])),
        ];

        let plan = environment_secret_plan("web", "web", &variables).expect("plan");

        assert_eq!(plan.secret_name, "web-env");
        assert_eq!(
            plan.keys,
            vec![
                ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                "APP_SECRET".to_string()
            ]
        );
        assert!(!plan.checksum.is_empty());
    }

    #[test]
    fn plan_checksum_changes_only_when_secret_values_change() {
        let before = vec![secret_var("APP_SECRET", "v1", None)];
        let unchanged = vec![secret_var("APP_SECRET", "v1", None)];
        let rotated = vec![secret_var("APP_SECRET", "v2", None)];

        let plan_before = environment_secret_plan("web", "web", &before).expect("plan");
        let plan_unchanged = environment_secret_plan("web", "web", &unchanged).expect("plan");
        let plan_rotated = environment_secret_plan("web", "web", &rotated).expect("plan");

        assert_eq!(plan_before.checksum, plan_unchanged.checksum);
        assert_ne!(
            plan_before.checksum, plan_rotated.checksum,
            "rotating a secret value must change the checksum that rolls pods"
        );
    }

    #[test]
    fn secret_manifest_is_a_typed_opaque_secret_with_values_and_checksum() {
        let variables = vec![
            secret_var("APP_SECRET", "s3cret", None),
            secret_var(ENV_ALIEN_COMMANDS_TOKEN, "tok", Some(vec!["web"])),
        ];
        let plan = environment_secret_plan("web", "web", &variables).expect("plan");
        let secret_values = environment_secret_values("web", &variables, &BTreeMap::new());

        let secret = environment_secret_manifest(&plan, "web", "test-ns", &secret_values);

        assert_eq!(secret.metadata.name.as_deref(), Some("web-env"));
        assert_eq!(secret.metadata.namespace.as_deref(), Some("test-ns"));
        assert_eq!(secret.type_.as_deref(), Some("Opaque"));
        assert_eq!(
            secret
                .metadata
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.get("env-secret-checksum")),
            Some(&plan.checksum)
        );
        let data = secret.data.expect("secret data");
        assert_eq!(
            data.get("APP_SECRET"),
            Some(&ByteString(b"s3cret".to_vec()))
        );
        assert_eq!(
            data.get(ENV_ALIEN_COMMANDS_TOKEN),
            Some(&ByteString(b"tok".to_vec()))
        );
    }

    #[test]
    fn controller_owned_secrets_are_planned_and_override_snapshot_values() {
        let variables = vec![secret_var(
            OTEL_EXPORTER_OTLP_HEADERS,
            "stale-user-value",
            None,
        )];
        let additional = BTreeMap::from([
            (
                OTEL_EXPORTER_OTLP_HEADERS.to_string(),
                "authorization=Bearer current".to_string(),
            ),
            (
                OTEL_EXPORTER_OTLP_METRICS_HEADERS.to_string(),
                "authorization=Bearer metrics".to_string(),
            ),
        ]);
        let plan =
            environment_secret_plan_with_additional_secrets("web", "web", &variables, &additional)
                .expect("plan");
        let values = environment_secret_values("web", &variables, &additional);
        let secret = environment_secret_manifest(&plan, "web", "test-ns", &values);

        assert_eq!(
            plan.keys,
            vec![
                OTEL_EXPORTER_OTLP_HEADERS.to_string(),
                OTEL_EXPORTER_OTLP_METRICS_HEADERS.to_string(),
            ]
        );
        assert_eq!(
            secret.data.expect("data").get(OTEL_EXPORTER_OTLP_HEADERS),
            Some(&ByteString(b"authorization=Bearer current".to_vec()))
        );
    }

    #[test]
    fn direct_monitoring_headers_use_logs_fallback_when_metrics_auth_is_missing() {
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("agent".to_string()).build()),
            vec![],
        )
        .with_monitoring(OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer logs".to_string(),
            metrics_endpoint: Some("https://manager.test/v1/metrics".to_string()),
            metrics_auth_header: None,
            resource_attributes: Default::default(),
        });

        let headers = direct_monitoring_auth_headers(&harness.ctx());

        assert_eq!(
            headers.get(OTEL_EXPORTER_OTLP_HEADERS).map(String::as_str),
            Some("authorization=Bearer logs")
        );
        assert_eq!(
            headers
                .get(OTEL_EXPORTER_OTLP_METRICS_HEADERS)
                .map(String::as_str),
            Some("authorization=Bearer logs")
        );
    }

    #[test]
    fn rotation_tracker_detects_controller_owned_secret_changes_and_removal() {
        let before = BTreeMap::from([(
            OTEL_EXPORTER_OTLP_HEADERS.to_string(),
            "authorization=Bearer v1".to_string(),
        )]);
        let rotated = BTreeMap::from([(
            OTEL_EXPORTER_OTLP_HEADERS.to_string(),
            "authorization=Bearer v2".to_string(),
        )]);
        let plan = environment_secret_plan_with_additional_secrets("agent", "agent", &[], &before)
            .expect("monitoring plan");
        let mut tracker = EnvSecretRotationTracker::default();
        tracker.record(Some(&plan));

        assert!(!tracker.drifted_with_additional_secrets("agent", "agent", &[], &before));
        assert!(tracker.drifted_with_additional_secrets("agent", "agent", &[], &rotated));
        assert!(tracker.drifted_with_additional_secrets("agent", "agent", &[], &BTreeMap::new()));
    }

    #[test]
    fn environment_secret_cleanup_requires_exact_ownership_labels() {
        let owned = Secret {
            metadata: ObjectMeta {
                name: Some("web-env".to_string()),
                labels: Some(BTreeMap::from([
                    ("managed-by".to_string(), "runtime".to_string()),
                    ("resource-id".to_string(), "web".to_string()),
                ])),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(ensure_environment_secret_is_owned(&owned, "web-env", "web").is_ok());
        assert!(ensure_environment_secret_is_owned(&owned, "web-env", "other").is_err());

        let unmanaged = Secret {
            metadata: ObjectMeta {
                name: Some("web-env".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(ensure_environment_secret_is_owned(&unmanaged, "web-env", "web").is_err());
    }

    #[tokio::test]
    async fn reconcile_without_desired_values_deletes_owned_workload_secret() {
        let existing = Secret {
            metadata: ObjectMeta {
                name: Some("agent-env".to_string()),
                labels: Some(BTreeMap::from([
                    ("managed-by".to_string(), "runtime".to_string()),
                    ("resource-id".to_string(), "agent".to_string()),
                ])),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_get_secret()
            .withf(|namespace, name| namespace == "test-ns" && name == "agent-env")
            .times(1)
            .return_once(move |_, _| Ok(existing));
        secrets
            .expect_delete_secret()
            .withf(|namespace, name| namespace == "test-ns" && name == "agent-env")
            .times(1)
            .return_once(|_, _| Ok(()));
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_secrets_client()
            .times(1)
            .returning(move |_| Ok(secrets.clone()));
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("agent".to_string()).build()),
            vec![],
        )
        .with_service_provider(Arc::new(provider));

        let plan =
            reconcile_environment_secret("daemon", "agent", "agent", "test-ns", &harness.ctx())
                .await
                .expect("cleanup reconcile");

        assert!(plan.is_none());
    }

    #[tokio::test]
    async fn reconcile_without_desired_values_preserves_unowned_same_name_secret() {
        let existing = Secret {
            metadata: ObjectMeta {
                name: Some("agent-env".to_string()),
                labels: Some(BTreeMap::from([(
                    "managed-by".to_string(),
                    "someone-else".to_string(),
                )])),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_get_secret()
            .times(1)
            .return_once(move |_, _| Ok(existing));
        secrets.expect_delete_secret().times(0);
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_secrets_client()
            .times(1)
            .returning(move |_| Ok(secrets.clone()));
        let harness = KubernetesManifestTestHarness::new(
            Resource::new(Vault::new("agent".to_string()).build()),
            vec![],
        )
        .with_service_provider(Arc::new(provider));

        let plan =
            reconcile_environment_secret("daemon", "agent", "agent", "test-ns", &harness.ctx())
                .await
                .expect("non-owned cleanup reconcile");

        assert!(plan.is_none());
    }
}

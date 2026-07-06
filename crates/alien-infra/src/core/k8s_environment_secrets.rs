use std::collections::BTreeMap;

use alien_core::{EnvironmentVariable, EnvironmentVariableType};
use alien_error::{Context, ContextError};
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::ByteString;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

#[derive(Debug, Clone)]
pub struct KubernetesEnvSecretPlan {
    pub secret_name: String,
    pub checksum: String,
    pub keys: Vec<String>,
}

fn matches_environment_target(resource_id: &str, target_resources: &Option<Vec<String>>) -> bool {
    match target_resources {
        None => true,
        Some(patterns) if patterns.is_empty() => false,
        Some(patterns) => patterns.iter().any(|pattern| {
            if let Some(prefix) = pattern.strip_suffix('*') {
                resource_id.starts_with(prefix)
            } else {
                resource_id == pattern
            }
        }),
    }
}

pub(crate) fn applicable_secret_environment_variables<'a>(
    resource_id: &str,
    variables: &'a [EnvironmentVariable],
) -> Vec<&'a EnvironmentVariable> {
    variables
        .iter()
        .filter(|var| var.var_type == EnvironmentVariableType::Secret)
        .filter(|var| matches_environment_target(resource_id, &var.target_resources))
        .collect()
}

fn secret_checksum(secret_vars: &[&EnvironmentVariable]) -> String {
    use sha2::{Digest, Sha256};

    let mut vars = secret_vars.to_vec();
    vars.sort_by(|left, right| left.name.cmp(&right.name));

    let mut hasher = Sha256::new();
    for var in vars {
        hasher.update(var.name.as_bytes());
        hasher.update(b"=");
        hasher.update(var.value.as_bytes());
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
    let secret_vars = applicable_secret_environment_variables(resource_id, variables);
    if secret_vars.is_empty() {
        return None;
    }

    Some(KubernetesEnvSecretPlan {
        secret_name: format!("{workload_name}-env"),
        checksum: secret_checksum(&secret_vars),
        keys: secret_vars
            .iter()
            .map(|var| var.name.clone())
            .collect::<Vec<_>>(),
    })
}

/// Builds the typed Kubernetes Secret manifest holding the plan's key/value
/// pairs, stamped with the plan checksum.
fn environment_secret_manifest(
    plan: &KubernetesEnvSecretPlan,
    resource_id: &str,
    namespace: &str,
    secret_vars: &[&EnvironmentVariable],
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
            secret_vars
                .iter()
                .map(|var| (var.name.clone(), ByteString(var.value.as_bytes().to_vec())))
                .collect(),
        ),
        ..Default::default()
    }
}

pub async fn reconcile_environment_secret(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<KubernetesEnvSecretPlan>> {
    let variables = &ctx.deployment_config.environment_variables.variables;
    let Some(plan) = environment_secret_plan(resource_id, workload_name, variables) else {
        return Ok(None);
    };

    let secret_vars = applicable_secret_environment_variables(resource_id, variables);
    let mut secret = environment_secret_manifest(&plan, resource_id, namespace, &secret_vars);
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::ENV_ALIEN_COMMANDS_TOKEN;

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
                "APP_SECRET".to_string(),
                ENV_ALIEN_COMMANDS_TOKEN.to_string()
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
        let secret_vars = applicable_secret_environment_variables("web", &variables);

        let secret = environment_secret_manifest(&plan, "web", "test-ns", &secret_vars);

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
}

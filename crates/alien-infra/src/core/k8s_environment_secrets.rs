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

pub async fn reconcile_environment_secret(
    resource_kind: &str,
    resource_id: &str,
    workload_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<KubernetesEnvSecretPlan>> {
    let secret_vars = applicable_secret_environment_variables(
        resource_id,
        &ctx.deployment_config.environment_variables.variables,
    );
    if secret_vars.is_empty() {
        return Ok(None);
    }

    let secret_name = format!("{workload_name}-env");
    let checksum = secret_checksum(&secret_vars);
    let keys = secret_vars
        .iter()
        .map(|var| var.name.clone())
        .collect::<Vec<_>>();

    let mut secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(BTreeMap::from([
                ("managed-by".to_string(), "runtime".to_string()),
                ("resource-id".to_string(), resource_id.to_string()),
            ])),
            annotations: Some(BTreeMap::from([(
                "env-secret-checksum".to_string(),
                checksum.clone(),
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
    };

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

    Ok(Some(KubernetesEnvSecretPlan {
        secret_name,
        checksum,
        keys,
    }))
}

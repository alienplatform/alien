use crate::{ErrorData, Result};
use alien_aws_clients::{StsApi, StsClient};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    AwsEnvironmentInfo, AzureEnvironmentInfo, ClientConfig, ComputeBackend, ContainerCluster,
    DeploymentConfig, EnvironmentInfo, EnvironmentVariable, EnvironmentVariableType,
    EnvironmentVariablesSnapshot, GcpEnvironmentInfo, LocalEnvironmentInfo, OtlpConfig, Platform,
    ResourceStatus, Stack, StackState, TemplateInputs, TestEnvironmentInfo,
};
use alien_error::{AlienError, Context, IntoAlienError as _};
use alien_gcp_clients::{ResourceManagerApi, ResourceManagerClient};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

/// Collect environment information from cloud platforms
pub async fn collect_environment_info(
    platform: Platform,
    client_config: &ClientConfig,
) -> Result<EnvironmentInfo> {
    info!(
        "Collecting environment information for platform {:?}",
        platform
    );

    match platform {
        Platform::Aws => collect_aws_env_info(client_config).await,
        Platform::Gcp => collect_gcp_env_info(client_config).await,
        Platform::Azure => collect_azure_env_info(client_config).await,
        Platform::Local => collect_local_env_info(client_config).await,
        Platform::Test => collect_test_env_info().await,
        _ => Err(AlienError::new(ErrorData::MissingConfiguration {
            message: format!(
                "Environment collection not supported for platform {:?}",
                platform
            ),
        })),
    }
}

async fn collect_aws_env_info(client_config: &ClientConfig) -> Result<EnvironmentInfo> {
    let aws_config = client_config.aws_config().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "AWS client config required for environment collection".to_string(),
        })
    })?;

    let sts_client = StsClient::new(reqwest::Client::new(), aws_config.clone());
    let identity = sts_client.get_caller_identity().await.context(
        ErrorData::EnvironmentInfoCollectionFailed {
            platform: "AWS".to_string(),
            reason: "STS GetCallerIdentity failed".to_string(),
        },
    )?;

    Ok(EnvironmentInfo::Aws(AwsEnvironmentInfo {
        account_id: identity
            .get_caller_identity_result
            .account
            .unwrap_or_default(),
        region: aws_config.region.clone(),
    }))
}

async fn collect_gcp_env_info(client_config: &ClientConfig) -> Result<EnvironmentInfo> {
    let gcp_config = client_config.gcp_config().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "GCP client config required for environment collection".to_string(),
        })
    })?;

    let rm_client = ResourceManagerClient::new(reqwest::Client::new(), gcp_config.clone());
    let project = rm_client
        .get_project_metadata(gcp_config.project_id.clone())
        .await
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: "ResourceManager projects.get failed".to_string(),
        })?;

    Ok(EnvironmentInfo::Gcp(GcpEnvironmentInfo {
        project_number: project.project_number.unwrap_or_default(),
        project_id: gcp_config.project_id.clone(),
        region: gcp_config.region.clone(),
    }))
}

async fn collect_azure_env_info(client_config: &ClientConfig) -> Result<EnvironmentInfo> {
    let azure_config = client_config.azure_config().ok_or_else(|| {
        AlienError::new(ErrorData::MissingConfiguration {
            message: "Azure client config required for environment collection".to_string(),
        })
    })?;

    // Azure environment info is available directly from the client config
    // No need to make API calls - subscription_id and tenant_id are already configured
    Ok(EnvironmentInfo::Azure(AzureEnvironmentInfo {
        tenant_id: azure_config.tenant_id.clone(),
        subscription_id: azure_config.subscription_id.clone(),
        location: azure_config
            .region
            .clone()
            .unwrap_or_else(|| "eastus".to_string()),
    }))
}

async fn collect_local_env_info(_client_config: &ClientConfig) -> Result<EnvironmentInfo> {
    // Collect local system information
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    Ok(EnvironmentInfo::Local(LocalEnvironmentInfo {
        hostname,
        os,
        arch,
    }))
}

async fn collect_test_env_info() -> Result<EnvironmentInfo> {
    // Return mock environment info for test platform
    Ok(EnvironmentInfo::Test(TestEnvironmentInfo {
        test_id: format!("test-{}", uuid::Uuid::new_v4().simple()),
    }))
}

/// Configuration for ALIEN_SECRETS environment variable
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AlienSecretsConfig {
    /// Secret keys to load from vault
    keys: Vec<String>,
    /// Hash of all env var values - triggers redeployment when changed
    hash: String,
}

/// Inject environment variables into stack functions and containers
///
/// For all compute resources (Functions and Containers built from source):
/// - Plain variables: Injected directly into resource.environment
/// - Secret variables: Synced to vault, resource gets ALIEN_SECRETS with keys list
///
/// The alien-runtime (present in all source-based resources) loads secrets from vault
/// at startup using the keys in ALIEN_SECRETS.
///
/// Targeting patterns control which resources receive which variables.
pub fn inject_environment_variables(stack: &mut Stack, config: &DeploymentConfig) -> Result<()> {
    info!("Injecting environment variables into compute resources");

    let snapshot = &config.environment_variables;

    // Iterate through all resources in the stack
    for (resource_name, resource_entry) in &mut stack.resources {
        let resource_type = resource_entry.config.resource_type();

        // Use type-safe ResourceType constants instead of string matching
        if resource_type == alien_core::Function::RESOURCE_TYPE {
            inject_into_compute_resource(resource_name, resource_entry, snapshot, true)?;
        } else if resource_type == alien_core::Container::RESOURCE_TYPE {
            inject_into_compute_resource(resource_name, resource_entry, snapshot, false)?;
        }
        // Other resource types don't support environment variables
    }

    Ok(())
}

/// Inject OTLP monitoring environment variables into all compute resources.
///
/// When `DeploymentConfig.monitoring` is set, this injects:
/// - `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` — the OTLP logs endpoint URL
/// - `OTEL_EXPORTER_OTLP_HEADERS`       — auth header in "key=value" format
///
/// deepstore-* resources are skipped: they manage their own telemetry pipeline
/// and receive OTLP config via a different mechanism.
pub fn inject_monitoring_environment_variables(
    stack: &mut Stack,
    monitoring: &OtlpConfig,
) -> Result<()> {
    info!("Injecting OTLP monitoring env vars into compute resources");

    for (resource_name, resource_entry) in &mut stack.resources {
        // Skip deepstore resources — they run DeepStore itself and must not
        // receive the OTLP endpoint that points back into their own storage.
        if resource_name.starts_with("deepstore-") {
            debug!(
                "Skipping OTLP injection for deepstore resource '{}'",
                resource_name
            );
            continue;
        }

        let resource_type = resource_entry.config.resource_type();

        let environment = if resource_type == alien_core::Function::RESOURCE_TYPE {
            Some(
                &mut resource_entry
                    .config
                    .downcast_mut::<alien_core::Function>()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InternalError {
                            message: format!(
                                "Failed to downcast resource '{}' to Function",
                                resource_name
                            ),
                        })
                    })?
                    .environment,
            )
        } else if resource_type == alien_core::Container::RESOURCE_TYPE {
            Some(
                &mut resource_entry
                    .config
                    .downcast_mut::<alien_core::Container>()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InternalError {
                            message: format!(
                                "Failed to downcast resource '{}' to Container",
                                resource_name
                            ),
                        })
                    })?
                    .environment,
            )
        } else {
            None
        };

        if let Some(env) = environment {
            env.insert(
                "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT".to_string(),
                monitoring.logs_endpoint.clone(),
            );
            env.insert(
                "OTEL_EXPORTER_OTLP_HEADERS".to_string(),
                monitoring.logs_auth_header.clone(),
            );
            if let Some(metrics_endpoint) = &monitoring.metrics_endpoint {
                env.insert(
                    "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT".to_string(),
                    metrics_endpoint.clone(),
                );
                // Use the metrics-specific auth header if present, otherwise reuse the logs one.
                let metrics_headers = monitoring
                    .metrics_auth_header
                    .as_ref()
                    .unwrap_or(&monitoring.logs_auth_header);
                env.insert(
                    "OTEL_EXPORTER_OTLP_METRICS_HEADERS".to_string(),
                    metrics_headers.clone(),
                );
            }
            debug!("Injected OTLP monitoring vars into '{}'", resource_name);
        }
    }

    Ok(())
}

/// Stamp deployment-config values onto ContainerCluster template inputs.
///
/// ContainerCluster controllers read `template_inputs` (horizond URL, binary hash,
/// monitoring config) from the resource config. These values originate from
/// `DeploymentConfig` and may change between syncs (e.g., new horizond binary ETag).
///
/// Like `inject_environment_variables`, this runs every step — not just once during
/// preflights — so the executor's `resource_eq()` always compares against the latest
/// deployment config values.
///
/// The OTLP auth header is sensitive, so only a SHA-256 hash is stored for change
/// detection. The actual value is read from `DeploymentConfig` at provisioning time.
pub fn stamp_template_inputs(stack: &mut Stack, config: &DeploymentConfig) -> Result<()> {
    let horizon_config = match &config.compute_backend {
        Some(ComputeBackend::Horizon(h)) => h,
        _ => return Ok(()),
    };

    let monitoring_logs_endpoint = config.monitoring.as_ref().map(|m| m.logs_endpoint.clone());
    let monitoring_metrics_endpoint = config
        .monitoring
        .as_ref()
        .and_then(|m| m.metrics_endpoint.clone());
    let monitoring_auth_hash = config.monitoring.as_ref().map(|m| {
        let hash = Sha256::digest(m.logs_auth_header.as_bytes());
        hex::encode(hash)
    });
    let monitoring_metrics_auth_hash = config.monitoring.as_ref().and_then(|m| {
        m.metrics_auth_header.as_ref().map(|h| {
            let hash = Sha256::digest(h.as_bytes());
            hex::encode(hash)
        })
    });

    let cluster_ids: Vec<String> = stack
        .resources
        .iter()
        .filter(|(_, entry)| entry.config.resource_type().as_ref() == "container-cluster")
        .map(|(id, _)| id.clone())
        .collect();

    for cluster_id in &cluster_ids {
        if let Some(entry) = stack.resources.get_mut(cluster_id) {
            if let Some(cluster) = entry.config.downcast_mut::<ContainerCluster>() {
                cluster.template_inputs = Some(TemplateInputs {
                    horizond_download_base_url: horizon_config.horizond_download_base_url.clone(),
                    horizon_api_url: horizon_config.url.clone(),
                    horizond_binary_hash: horizon_config.horizond_binary_hash.clone(),
                    monitoring_logs_endpoint: monitoring_logs_endpoint.clone(),
                    monitoring_metrics_endpoint: monitoring_metrics_endpoint.clone(),
                    monitoring_auth_hash: monitoring_auth_hash.clone(),
                    monitoring_metrics_auth_hash: monitoring_metrics_auth_hash.clone(),
                });
                debug!(
                    cluster_id = %cluster_id,
                    horizond_url = %horizon_config.horizond_download_base_url,
                    has_monitoring = monitoring_logs_endpoint.is_some(),
                    has_metrics = monitoring_metrics_endpoint.is_some(),
                    "Stamped template inputs onto ContainerCluster"
                );
            }
        }
    }

    Ok(())
}

/// Inject environment variables into a Function or Container compute resource
///
/// Both Functions and Containers built from source include alien-runtime and support:
/// - Plain variables: Injected directly into resource.environment
/// - Secret variables: Loaded from vault via ALIEN_SECRETS mechanism
///
/// alien-runtime reads ALIEN_SECRETS at startup, fetches those keys from the vault,
/// and passes them to the application subprocess as environment variables.
fn inject_into_compute_resource(
    resource_name: &str,
    resource_entry: &mut alien_core::ResourceEntry,
    snapshot: &EnvironmentVariablesSnapshot,
    is_function: bool,
) -> Result<()> {
    // Get the environment map (same interface for Function and Container)
    let environment = if is_function {
        &mut resource_entry
            .config
            .downcast_mut::<alien_core::Function>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InternalError {
                    message: format!(
                        "Failed to downcast resource '{}' to Function",
                        resource_name
                    ),
                })
            })?
            .environment
    } else {
        &mut resource_entry
            .config
            .downcast_mut::<alien_core::Container>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InternalError {
                    message: format!(
                        "Failed to downcast resource '{}' to Container",
                        resource_name
                    ),
                })
            })?
            .environment
    };

    // Filter variables that apply to this resource
    let applicable_vars: Vec<&EnvironmentVariable> = snapshot
        .variables
        .iter()
        .filter(|v| matches_resource_pattern(resource_name, &v.target_resources))
        .collect();

    // Inject plain variables directly
    for var in applicable_vars
        .iter()
        .filter(|v| v.var_type == EnvironmentVariableType::Plain)
    {
        environment.insert(var.name.clone(), var.value.clone());
        debug!(
            "Injected plain variable '{}' into {}",
            var.name, resource_name
        );
    }

    // Collect secret keys this resource needs
    let secret_keys: Vec<String> = applicable_vars
        .iter()
        .filter(|v| v.var_type == EnvironmentVariableType::Secret)
        .filter(|v| {
            // Skip OTEL_EXPORTER secrets for deepstore-* resources
            if resource_name.starts_with("deepstore-") && v.name.starts_with("OTEL_EXPORTER") {
                debug!(
                    "Skipping OTEL secret '{}' for deepstore resource '{}'",
                    v.name, resource_name
                );
                false
            } else {
                true
            }
        })
        .map(|v| v.name.clone())
        .collect();

    // If resource needs secrets, add ALIEN_SECRETS env var
    // alien-runtime will load these from the vault at startup
    if !secret_keys.is_empty() {
        let alien_secrets = AlienSecretsConfig {
            keys: secret_keys.clone(),
            hash: snapshot.hash.clone(),
        };
        let alien_secrets_json = serde_json::to_string(&alien_secrets)
            .into_alien_error()
            .context(ErrorData::InternalError {
                message: "Failed to serialize ALIEN_SECRETS config".to_string(),
            })?;

        environment.insert("ALIEN_SECRETS".to_string(), alien_secrets_json);

        let resource_type = if is_function { "function" } else { "container" };
        debug!(
            "Added ALIEN_SECRETS to {} '{}' with {} secret keys",
            resource_type,
            resource_name,
            secret_keys.len()
        );
    }

    Ok(())
}

/// Check if a resource name matches the target patterns
fn matches_resource_pattern(resource_name: &str, target_resources: &Option<Vec<String>>) -> bool {
    match target_resources {
        // None means apply to all resources
        None => true,
        // Empty list means no resources (shouldn't happen, but handle gracefully)
        Some(patterns) if patterns.is_empty() => false,
        // Check if resource name matches any pattern
        Some(patterns) => patterns.iter().any(|pattern| {
            if pattern.ends_with('*') {
                // Wildcard suffix match: "api-*" matches "api-handler", "api-auth", etc.
                let prefix = &pattern[..pattern.len() - 1];
                resource_name.starts_with(prefix)
            } else {
                // Exact match
                resource_name == pattern
            }
        }),
    }
}

/// Sync secret-type environment variables to the vault
///
/// This function syncs all secret-type variables from the snapshot to the customer's vault.
/// It uses the runtime_metadata to track the last synced hash, avoiding redundant syncs.
///
/// Returns true if sync was performed, false if skipped (already synced).
pub async fn sync_secrets_to_vault(
    stack_state: &StackState,
    client_config: &ClientConfig,
    config: &DeploymentConfig,
    runtime_metadata: &mut alien_core::RuntimeMetadata,
) -> Result<bool> {
    let snapshot = &config.environment_variables;

    // Check if we've already synced this exact snapshot
    if let Some(last_synced_hash) = &runtime_metadata.last_synced_env_vars_hash {
        if last_synced_hash == &snapshot.hash {
            debug!(
                "Secrets already synced for hash {}, skipping",
                snapshot.hash
            );
            return Ok(false);
        }
    }

    // Filter for secret-type variables from snapshot
    let secret_vars: Vec<&EnvironmentVariable> = snapshot
        .variables
        .iter()
        .filter(|v| v.var_type == EnvironmentVariableType::Secret)
        .collect();

    // Skip if no secrets to sync
    if secret_vars.is_empty() {
        debug!("No secrets to sync to vault");
        // Still update the hash to mark as synced
        runtime_metadata.last_synced_env_vars_hash = Some(snapshot.hash.clone());
        return Ok(false);
    }

    info!(
        "Syncing {} secrets to vault (hash: {})",
        secret_vars.len(),
        snapshot.hash
    );

    // Create provider using deployment credentials
    let provider = BindingsProvider::from_stack_state(stack_state, client_config.clone()).context(
        ErrorData::InternalError {
            message: "Failed to create bindings provider for secret sync".to_string(),
        },
    )?;

    // Load the secrets vault
    let vault = provider
        .load_vault("secrets")
        .await
        .context(ErrorData::SecretSyncFailed {
            vault_name: "secrets".to_string(),
            reason: "Failed to load secrets vault".to_string(),
        })?;

    // Sync each secret from snapshot (values already decrypted)
    for env_var in secret_vars {
        vault
            .set_secret(&env_var.name, &env_var.value)
            .await
            .context(ErrorData::SecretSyncFailed {
                vault_name: "secrets".to_string(),
                reason: format!("Failed to set secret '{}'", env_var.name),
            })?;
        debug!("Synced secret '{}' to vault", env_var.name);
    }

    // Update metadata to mark this snapshot as synced
    runtime_metadata.last_synced_env_vars_hash = Some(snapshot.hash.clone());

    info!("Successfully synced all secrets to vault");
    Ok(true)
}

/// Interrupts all non-terminal, non-failed resources when a deployment failure is detected.
///
/// When one resource fails and the deployment stops, other resources may be left in
/// intermediate states (`Provisioning`, `Updating`, `Deleting`, `Pending`). This function
/// transitions them to their corresponding `*Failed` status with a `DeploymentInterrupted`
/// error, while preserving their controller state in `last_failed_state` for retry.
///
/// On retry, `retry_failed()` restores each interrupted resource from `last_failed_state`,
/// allowing it to resume exactly where it was interrupted.
///
/// # Arguments
/// * `stack_state` - The stack state to modify (mutable)
/// * `failed_resources` - The resources that actually failed (used to build the error message)
pub fn interrupt_in_progress_resources(
    stack_state: &mut StackState,
    failed_resources: &[(&str, &str)], // (resource_id, resource_type)
) {
    // Pick the first real failure to reference in the interrupted error message.
    // If there are no failures yet (shouldn't happen), fall back to a generic message.
    let (ref_id, ref_type) = failed_resources
        .first()
        .copied()
        .unwrap_or(("unknown", "unknown"));

    let interrupted_error = AlienError::new(ErrorData::DeploymentInterrupted {
        failed_resource_id: ref_id.to_string(),
        failed_resource_type: ref_type.to_string(),
    })
    .into_generic();

    for resource_state in stack_state.resources.values_mut() {
        let interrupted_status = match resource_state.status {
            // Already terminal — don't touch it.
            ResourceStatus::Running
            | ResourceStatus::Deleted
            | ResourceStatus::ProvisionFailed
            | ResourceStatus::UpdateFailed
            | ResourceStatus::DeleteFailed
            | ResourceStatus::RefreshFailed => continue,

            ResourceStatus::Provisioning | ResourceStatus::Pending => {
                ResourceStatus::ProvisionFailed
            }
            ResourceStatus::Updating => ResourceStatus::UpdateFailed,
            ResourceStatus::Deleting => ResourceStatus::DeleteFailed,
        };

        // Preserve the current controller state in last_failed_state so retry can resume from here.
        // For Pending resources (no controller yet), last_failed_state stays None and retry_failed()
        // will reset them back to Pending.
        resource_state.last_failed_state = resource_state.internal_state.clone();
        resource_state.status = interrupted_status;
        resource_state.error = Some(interrupted_error.clone());
        resource_state.retry_attempt = 0;
    }
}

/// Creates an aggregated error from resource errors in a stack state.
///
/// Collects errors from all failed resources. Resources that carry a
/// `DEPLOYMENT_INTERRUPTED` error (stopped as a side-effect of a sibling failure)
/// are counted separately and excluded from `resource_errors` so the caller gets
/// an accurate picture of what actually broke vs. what was collateral damage.
///
/// Returns `None` if no resources carry any error at all.
pub fn create_aggregated_error_from_stack_state(stack_state: &StackState) -> Option<AlienError> {
    use crate::error::ResourceError;

    let mut resource_errors: Vec<ResourceError> = Vec::new();
    let mut interrupted_resources: usize = 0;

    for resource in stack_state.resources.values() {
        let Some(error) = resource.error.as_ref() else {
            continue;
        };

        if error.code == "DEPLOYMENT_INTERRUPTED" {
            interrupted_resources += 1;
        } else {
            resource_errors.push(ResourceError {
                resource_id: resource.config.id().to_string(),
                resource_type: resource.resource_type.clone(),
                error: Some(error.clone().into_generic()),
            });
        }
    }

    if resource_errors.is_empty() && interrupted_resources == 0 {
        return None;
    }

    let total_resources = stack_state.resources.len();
    let failed_resources = resource_errors.len();

    Some(
        AlienError::new(ErrorData::AgentDeploymentFailed {
            resource_errors,
            total_resources,
            failed_resources,
            interrupted_resources,
        })
        .into_generic(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_resource_pattern_null() {
        // None means all resources
        assert!(matches_resource_pattern("api-handler", &None));
        assert!(matches_resource_pattern("worker", &None));
        assert!(matches_resource_pattern("anything", &None));
    }

    #[test]
    fn test_matches_resource_pattern_exact() {
        let patterns = Some(vec!["api-handler".to_string()]);

        assert!(matches_resource_pattern("api-handler", &patterns));
        assert!(!matches_resource_pattern("api-auth", &patterns));
        assert!(!matches_resource_pattern("worker", &patterns));
    }

    #[test]
    fn test_matches_resource_pattern_wildcard() {
        let patterns = Some(vec!["api-*".to_string()]);

        assert!(matches_resource_pattern("api-handler", &patterns));
        assert!(matches_resource_pattern("api-auth", &patterns));
        assert!(matches_resource_pattern("api-", &patterns));
        assert!(!matches_resource_pattern("api", &patterns));
        assert!(!matches_resource_pattern("worker", &patterns));
    }

    #[test]
    fn test_matches_resource_pattern_multiple() {
        let patterns = Some(vec!["api-*".to_string(), "worker".to_string()]);

        assert!(matches_resource_pattern("api-handler", &patterns));
        assert!(matches_resource_pattern("api-auth", &patterns));
        assert!(matches_resource_pattern("worker", &patterns));
        assert!(!matches_resource_pattern("scheduler", &patterns));
    }

    #[test]
    fn test_matches_resource_pattern_empty() {
        let patterns = Some(vec![]);

        assert!(!matches_resource_pattern("api-handler", &patterns));
        assert!(!matches_resource_pattern("worker", &patterns));
    }
}

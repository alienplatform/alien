use crate::{ErrorData, Result};
use alien_aws_clients::{StsApi, StsClient};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    AwsEnvironmentInfo, AzureEnvironmentInfo, ClientConfig, DeploymentConfig, EnvironmentInfo,
    EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot, GcpEnvironmentInfo,
    LocalEnvironmentInfo, OtlpConfig, Platform, ResourceStatus, Stack, StackState,
    TestEnvironmentInfo, ENV_ALIEN_SECRETS,
};
use alien_error::{AlienError, Context, IntoAlienError as _};
use alien_gcp_clients::{ResourceManagerApi, ResourceManagerClient};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info};

const OTEL_RESOURCE_ATTRIBUTES: &str = "OTEL_RESOURCE_ATTRIBUTES";

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

/// Inject environment variables into stack functions and containers.
///
/// For all compute resources (Workers and Containers built from source):
/// - Plain variables: Injected directly into resource.environment
/// - Secret variables: Keys are listed in ALIEN_SECRETS; alien-runtime loads
///   the actual values from the "secrets" vault at startup.
///
/// The secrets vault is a dependency of every compute resource (added by
/// `SecretsVaultMutation`). The executor won't start a function until its
/// vault dependency is Running, so ALIEN_SECRETS is always safe to inject.
pub fn inject_environment_variables(stack: &mut Stack, config: &DeploymentConfig) -> Result<()> {
    info!("Injecting environment variables into compute resources");

    let snapshot = &config.environment_variables;

    for (resource_name, resource_entry) in &mut stack.resources {
        let resource_type = resource_entry.config.resource_type();

        if resource_type == alien_core::Worker::RESOURCE_TYPE {
            inject_into_compute_resource(resource_name, resource_entry, snapshot, true)?;
        } else if resource_type == alien_core::Container::RESOURCE_TYPE {
            inject_into_compute_resource(resource_name, resource_entry, snapshot, false)?;
        }
    }

    Ok(())
}

/// Inject OTLP monitoring environment variables into all compute resources.
///
/// When `DeploymentConfig.monitoring` is set, this injects:
/// - `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` — the OTLP logs endpoint URL
/// - `OTEL_EXPORTER_OTLP_HEADERS`       — auth header in "key=value" format
/// - `OTEL_SERVICE_NAME`                — defaults to the resource name so
///   each resource within the stack appears as a distinct `service.name` in
///   logs (drives the dashboard's "Resource" column). Skipped if the user
///   has already set `OTEL_SERVICE_NAME` via plain or secret env vars.
/// - `OTEL_RESOURCE_ATTRIBUTES`       — deployment-level resource attributes
///   such as `alien.deployment_id`, merged with any user-provided value.
pub fn inject_monitoring_environment_variables(
    stack: &mut Stack,
    monitoring: &OtlpConfig,
) -> Result<()> {
    info!("Injecting OTLP monitoring env vars into compute resources");

    for (resource_name, resource_entry) in &mut stack.resources {
        let resource_type = resource_entry.config.resource_type();

        let environment = if resource_type == alien_core::Worker::RESOURCE_TYPE {
            Some(
                &mut resource_entry
                    .config
                    .downcast_mut::<alien_core::Worker>()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InternalError {
                            message: format!(
                                "Failed to downcast resource '{}' to Worker",
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
            if !monitoring.resource_attributes.is_empty() {
                let merged =
                    merge_otel_resource_attributes(env.get(OTEL_RESOURCE_ATTRIBUTES), monitoring);
                env.insert(OTEL_RESOURCE_ATTRIBUTES.to_string(), merged);
            }
            // The resource name (e.g. "agent" / "events") is the most useful
            // value for `service.name`: it identifies *which slot in the
            // stack* a log came from, which is the dimension users see in
            // the dashboard's "Resource" column. Without this, alien-runtime
            // falls back to the literal string "alien-runtime" and the
            // column carries no per-row signal.
            //
            // We only set the env var if the user hasn't already pinned
            // OTEL_SERVICE_NAME themselves (e.g. via the platform's user
            // env vars), so explicit overrides keep winning.
            env.entry("OTEL_SERVICE_NAME".to_string())
                .or_insert_with(|| resource_name.clone());
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

fn merge_otel_resource_attributes(existing: Option<&String>, monitoring: &OtlpConfig) -> String {
    let mut attributes = parse_otel_resource_attributes(existing.map(String::as_str));
    for (key, value) in &monitoring.resource_attributes {
        attributes.insert(key.clone(), value.clone());
    }

    attributes
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_otel_resource_attributes(existing: Option<&str>) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();

    if let Some(existing) = existing {
        for entry in existing.split(',') {
            if let Some((key, value)) = entry.split_once('=') {
                let key = key.trim();
                if !key.is_empty() {
                    attributes.insert(key.to_string(), value.trim().to_string());
                }
            }
        }
    }

    attributes
}

/// Inject environment variables into a Worker or Container compute resource.
///
/// - Plain variables: inserted directly into resource.environment.
/// - Secret variables: their keys are collected into ALIEN_SECRETS so
///   alien-runtime can fetch them from the vault at startup.
fn inject_into_compute_resource(
    resource_name: &str,
    resource_entry: &mut alien_core::ResourceEntry,
    snapshot: &EnvironmentVariablesSnapshot,
    is_worker: bool,
) -> Result<()> {
    // Get the environment map (same interface for Worker and Container)
    let environment = if is_worker {
        &mut resource_entry
            .config
            .downcast_mut::<alien_core::Worker>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InternalError {
                    message: format!("Failed to downcast resource '{}' to Worker", resource_name),
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

        environment.insert(ENV_ALIEN_SECRETS.to_string(), alien_secrets_json);

        let resource_type = if is_worker { "worker" } else { "container" };
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
            | ResourceStatus::TeardownRequired
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
/// Returns `None` if no resources carry a real failure. Interrupted resources
/// are fallout from another failure, not a standalone headline cause.
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

    if resource_errors.is_empty() {
        return None;
    }

    let total_resources = stack_state.resources.len();
    let failed_resources = resource_errors.len();

    Some(
        AlienError::new(ErrorData::DeploymentFailed {
            resource_errors,
            total_resources,
            failed_resources,
            interrupted_resources,
        })
        .into_generic(),
    )
}

/// Derive the single headline deployment error from durable deployment state.
///
/// Deployment-level errors win because they describe failures outside a
/// specific resource. Resource controller errors remain preserved on
/// `StackState.resources[*].error` and are summarized only when there is no
/// deployment-level error.
pub fn deployment_headline_error_from_state(
    state: &alien_core::DeploymentState,
) -> Option<AlienError> {
    state.error.clone().or_else(|| {
        state
            .stack_state
            .as_ref()
            .and_then(create_aggregated_error_from_stack_state)
    })
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

    // ── inject_environment_variables tests ──────────────────────────

    use alien_core::{
        ExternalBindings, Platform, Resource, ResourceEntry, ResourceLifecycle, ResourceStatus,
        StackResourceState, StackSettings, Worker, WorkerCode,
    };
    use alien_error::GenericError;
    use indexmap::IndexMap;

    fn make_snapshot(
        plain: &[(&str, &str)],
        secret: &[(&str, &str)],
    ) -> EnvironmentVariablesSnapshot {
        let mut variables = Vec::new();
        for (name, value) in plain {
            variables.push(EnvironmentVariable {
                name: name.to_string(),
                value: value.to_string(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
        }
        for (name, value) in secret {
            variables.push(EnvironmentVariable {
                name: name.to_string(),
                value: value.to_string(),
                var_type: EnvironmentVariableType::Secret,
                target_resources: None,
            });
        }
        EnvironmentVariablesSnapshot {
            variables,
            hash: "test-hash".to_string(),
            created_at: String::new(),
        }
    }

    fn make_config(snapshot: EnvironmentVariablesSnapshot) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(snapshot)
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    fn make_single_function_stack(function_id: &str) -> Stack {
        let function = Worker::new(function_id.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            function_id.to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::PermissionsConfig {
                profiles,
                management: alien_core::ManagementPermissions::Auto,
            },
            supported_platforms: None,
        }
    }

    fn make_worker_resource_state(id: &str, error: Option<AlienError>) -> StackResourceState {
        let worker = Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let resource = Resource::new(worker);

        StackResourceState {
            resource_type: resource.resource_type().as_ref().to_string(),
            internal_state: None,
            status: ResourceStatus::ProvisionFailed,
            outputs: None,
            config: resource,
            previous_config: None,
            retry_attempt: 0,
            error,
            lifecycle: Some(ResourceLifecycle::Live),
            controller_platform: None,
            dependencies: Vec::new(),
            last_failed_state: None,
            remote_binding_params: None,
        }
    }

    fn generic_error(code_message: &str) -> AlienError {
        AlienError::new(GenericError {
            message: code_message.to_string(),
        })
    }

    #[test]
    fn aggregate_error_ignores_interrupted_only_resources() {
        let mut stack_state = StackState::new(Platform::Test);
        stack_state.resources.insert(
            "skipped".to_string(),
            make_worker_resource_state(
                "skipped",
                Some(
                    AlienError::new(ErrorData::DeploymentInterrupted {
                        failed_resource_id: "failed".to_string(),
                        failed_resource_type: "worker".to_string(),
                    })
                    .into_generic(),
                ),
            ),
        );

        assert!(create_aggregated_error_from_stack_state(&stack_state).is_none());
    }

    #[test]
    fn aggregate_error_counts_failed_and_interrupted_resources() {
        let mut stack_state = StackState::new(Platform::Test);
        stack_state.resources.insert(
            "failed".to_string(),
            make_worker_resource_state("failed", Some(generic_error("worker failed"))),
        );
        stack_state.resources.insert(
            "skipped".to_string(),
            make_worker_resource_state(
                "skipped",
                Some(
                    AlienError::new(ErrorData::DeploymentInterrupted {
                        failed_resource_id: "failed".to_string(),
                        failed_resource_type: "worker".to_string(),
                    })
                    .into_generic(),
                ),
            ),
        );

        let error = create_aggregated_error_from_stack_state(&stack_state)
            .expect("real resource error should create aggregate deployment error");

        assert_eq!(error.code, "DEPLOYMENT_FAILED");
        assert_eq!(
            error.context.as_ref().and_then(|context| context
                .get("failed_resources")
                .and_then(serde_json::Value::as_u64)),
            Some(1)
        );
        assert_eq!(
            error.context.as_ref().and_then(|context| context
                .get("interrupted_resources")
                .and_then(serde_json::Value::as_u64)),
            Some(1)
        );
    }

    #[test]
    fn test_inject_adds_alien_secrets_when_secret_vars_present() {
        let snapshot = make_snapshot(
            &[("PLAIN_VAR", "pv")],
            &[("SECRET_TOKEN", "st"), ("SECRET_KEY", "sk")],
        );
        let config = make_config(snapshot);
        let mut stack = make_single_function_stack("worker");

        inject_environment_variables(&mut stack, &config).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        // Plain var injected directly
        assert_eq!(func.environment.get("PLAIN_VAR").unwrap(), "pv");

        // ALIEN_SECRETS present with both secret keys
        let alien_secrets_raw = func.environment.get(ENV_ALIEN_SECRETS).unwrap();
        let parsed: AlienSecretsConfig = serde_json::from_str(alien_secrets_raw).unwrap();
        assert!(parsed.keys.contains(&"SECRET_TOKEN".to_string()));
        assert!(parsed.keys.contains(&"SECRET_KEY".to_string()));
        assert_eq!(parsed.hash, "test-hash");

        // Secret values NOT injected as plain env vars
        assert!(func.environment.get("SECRET_TOKEN").is_none());
        assert!(func.environment.get("SECRET_KEY").is_none());
    }

    #[test]
    fn test_inject_no_alien_secrets_when_only_plain_vars() {
        let snapshot = make_snapshot(&[("APP_ENV", "prod")], &[]);
        let config = make_config(snapshot);
        let mut stack = make_single_function_stack("worker");

        inject_environment_variables(&mut stack, &config).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        assert_eq!(func.environment.get("APP_ENV").unwrap(), "prod");
        assert!(func.environment.get(ENV_ALIEN_SECRETS).is_none());
    }

    #[test]
    fn test_inject_respects_target_resources_for_secrets() {
        let mut snapshot = make_snapshot(&[], &[]);
        snapshot.variables.push(EnvironmentVariable {
            name: "TARGETED_SECRET".to_string(),
            value: "val".to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: Some(vec!["other-fn".to_string()]),
        });
        let config = make_config(snapshot);
        let mut stack = make_single_function_stack("worker");

        inject_environment_variables(&mut stack, &config).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        // Secret targeted at "other-fn" should NOT produce ALIEN_SECRETS on "worker"
        assert!(func.environment.get(ENV_ALIEN_SECRETS).is_none());
    }

    #[test]
    fn test_monitoring_injects_resource_attributes() {
        let mut stack = make_single_function_stack("worker");
        let monitoring = OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer token".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: std::collections::HashMap::from([
                ("alien.workspace_id".to_string(), "ws_test".to_string()),
                ("alien.deployment_id".to_string(), "dep_test".to_string()),
            ]),
        };

        inject_monitoring_environment_variables(&mut stack, &monitoring).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        let attributes = func.environment.get(OTEL_RESOURCE_ATTRIBUTES).unwrap();
        assert!(attributes.contains("alien.deployment_id=dep_test"));
        assert!(attributes.contains("alien.workspace_id=ws_test"));
    }

    #[test]
    fn test_monitoring_resource_attributes_override_existing_alien_keys() {
        let mut stack = make_single_function_stack("worker");
        stack
            .resources
            .get_mut("worker")
            .unwrap()
            .config
            .downcast_mut::<Worker>()
            .unwrap()
            .environment
            .insert(
                OTEL_RESOURCE_ATTRIBUTES.to_string(),
                "custom=value,alien.deployment_id=wrong".to_string(),
            );
        let monitoring = OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer token".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: std::collections::HashMap::from([(
                "alien.deployment_id".to_string(),
                "dep_test".to_string(),
            )]),
        };

        inject_monitoring_environment_variables(&mut stack, &monitoring).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        assert_eq!(
            func.environment.get(OTEL_RESOURCE_ATTRIBUTES).unwrap(),
            "alien.deployment_id=dep_test,custom=value"
        );
    }
}

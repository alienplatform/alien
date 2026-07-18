use crate::{ErrorData, Result};
use alien_aws_clients::{StsApi, StsClient};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    AwsEnvironmentInfo, AzureEnvironmentInfo, ClientConfig, ComputeKind, DeploymentConfig,
    EnvironmentInfo, EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot,
    GcpEnvironmentInfo, LocalEnvironmentInfo, OtlpConfig, Platform, ResourceStatus, SecretDelivery,
    Stack, StackState, TestEnvironmentInfo, ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_RUNTIME_SECRETS,
    ENV_ALIEN_SECRETS,
};
use alien_error::{AlienError, Context, IntoAlienError as _};
use alien_gcp_clients::{ResourceManagerApi, ResourceManagerClient};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use tracing::{debug, info};

const OTEL_RESOURCE_ATTRIBUTES: &str = "OTEL_RESOURCE_ATTRIBUTES";
const OTEL_EXPORTER_OTLP_LOGS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT";
const OTEL_EXPORTER_OTLP_METRICS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT";
const OTEL_SERVICE_NAME: &str = "OTEL_SERVICE_NAME";
// These vault secret key strings keep the historical `__alien_runtime_otlp_*`
// spelling on purpose. They are looked up by name in already-deployed stacks'
// vaults; renaming them to match the `alien-worker-runtime` crate rename would
// orphan the existing secrets on every live deployment. We deliberately take the
// no-migration option and keep the wire keys stable.
const RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET: &str = "__alien_runtime_otlp_logs_auth_header";
const RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET: &str = "__alien_runtime_otlp_metrics_auth_header";
const SECRETS_SYNC_SCHEMA_VERSION: &[u8] = b"\0vault-sync:no-app-command-token:v2\0";

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

    // `aws_config.account_id` is already resolved at credential-load time
    // (`infer_account_id`): from AWS_ACCOUNT_ID env, AWS_ROLE_ARN, web identity,
    // or — only as last resort — STS GetCallerIdentity. Trust it.
    if !aws_config.account_id.is_empty() {
        return Ok(EnvironmentInfo::Aws(AwsEnvironmentInfo {
            account_id: aws_config.account_id.clone(),
            region: aws_config.region.clone(),
        }));
    }

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
/// For all compute resources (Workers, Containers, and Daemons):
/// - Plain variables: Injected directly into resource.environment
/// - Secret variables: Delivery depends on the platform and resource kind —
///   see `alien_core::SecretDelivery`. Where no native projection exists,
///   the keys are listed in ALIEN_SECRETS and alien-worker-runtime loads the
///   actual values from the "secrets" vault at startup.
///
/// `SecretsVaultMutation` links the secrets vault only to Worker wrappers that
/// can consume vault pointers. Native-projected workloads use the deployment
/// environment snapshot directly and receive no workload vault grant.
pub fn inject_environment_variables(
    stack: &mut Stack,
    config: &DeploymentConfig,
    platform: Platform,
) -> Result<()> {
    info!("Injecting environment variables into compute resources");

    let snapshot = &config.environment_variables;

    for (resource_name, resource_entry) in &mut stack.resources {
        let resource_type = resource_entry.config.resource_type();

        if resource_type == alien_core::Worker::RESOURCE_TYPE
            || resource_type == alien_core::Container::RESOURCE_TYPE
            || resource_type == alien_core::Daemon::RESOURCE_TYPE
        {
            inject_into_compute_resource(resource_name, resource_entry, snapshot, platform)?;
        }
    }

    Ok(())
}

/// Configuration for runtime-owned secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlienRuntimeSecretsConfig {
    /// Vault key that contains `OTEL_EXPORTER_OTLP_HEADERS`.
    otlp_logs_auth_header: Option<String>,
    /// Vault key that contains `OTEL_EXPORTER_OTLP_METRICS_HEADERS`.
    otlp_metrics_auth_header: Option<String>,
    /// Hash of runtime secret values.
    hash: String,
}

/// Whether containers and daemons get OTLP env vars on this platform.
///
/// On managed cloud platforms, containers and daemons run inside the hosted
/// container runtime, which already ships their telemetry — only workers need
/// the vars there. Self-hosted platforms have no such runtime, so every
/// compute resource gets them. Exhaustive so a new platform must pick a side.
fn otlp_injection_covers_containers_and_daemons(platform: Platform) -> bool {
    match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => false,
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => true,
    }
}

/// Inject OTLP monitoring environment variables into compute resources.
///
/// Workers are injected on every platform; containers and daemons only where
/// `otlp_injection_covers_containers_and_daemons` says so.
///
/// When `DeploymentConfig.monitoring` is set, this injects:
/// - `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` — the OTLP logs endpoint URL
/// - `OTEL_SERVICE_NAME`                — defaults to the resource name so
///   each resource within the stack appears as a distinct `service.name` in
///   logs (drives the dashboard's "Resource" column). Skipped if the user
///   has already set `OTEL_SERVICE_NAME` via plain or secret env vars.
/// - `OTEL_RESOURCE_ATTRIBUTES`       — deployment-level resource attributes
///   such as `alien.deployment_id`, merged with any user-provided value.
/// - `ALIEN_RUNTIME_SECRETS`          — workers only; points
///   alien-worker-runtime at runtime-owned vault secrets. These are not forwarded to
///   the child application process.
///
/// Runtime-less Containers and Daemons do not store auth headers in their
/// resource config. Their platform controller reads the values directly from
/// `DeploymentConfig.monitoring` at provisioning time: Local passes them to
/// the process, while Kubernetes projects them from a per-workload Secret.
pub fn inject_monitoring_environment_variables(
    stack: &mut Stack,
    monitoring: &OtlpConfig,
    platform: Platform,
) -> Result<()> {
    let covers_containers_and_daemons = otlp_injection_covers_containers_and_daemons(platform);
    info!(
        "Injecting OTLP monitoring env vars into {} runtimes",
        if covers_containers_and_daemons {
            "worker, container, and daemon"
        } else {
            "worker"
        }
    );

    for (resource_name, resource_entry) in &mut stack.resources {
        let resource_type = resource_entry.config.resource_type();
        let is_worker = resource_type == alien_core::Worker::RESOURCE_TYPE;

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
        } else if covers_containers_and_daemons
            && resource_type == alien_core::Daemon::RESOURCE_TYPE
        {
            Some(
                &mut resource_entry
                    .config
                    .downcast_mut::<alien_core::Daemon>()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InternalError {
                            message: format!(
                                "Failed to downcast resource '{}' to Daemon",
                                resource_name
                            ),
                        })
                    })?
                    .environment,
            )
        } else if covers_containers_and_daemons
            && resource_type == alien_core::Container::RESOURCE_TYPE
        {
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
                OTEL_EXPORTER_OTLP_LOGS_ENDPOINT.to_string(),
                monitoring.logs_endpoint.clone(),
            );
            if !monitoring.resource_attributes.is_empty() {
                let merged =
                    merge_otel_resource_attributes(env.get(OTEL_RESOURCE_ATTRIBUTES), monitoring);
                env.insert(OTEL_RESOURCE_ATTRIBUTES.to_string(), merged);
            }
            // The resource name (e.g. "agent" / "events") is the most useful
            // value for `service.name`: it identifies *which slot in the
            // stack* a log came from, which is the dimension users see in
            // the dashboard's "Resource" column. Without this, alien-worker-runtime
            // falls back to the literal string "alien-worker-runtime" and the
            // column carries no per-row signal.
            //
            // We only set the env var if the user hasn't already pinned
            // OTEL_SERVICE_NAME themselves (e.g. via the platform's user
            // env vars), so explicit overrides keep winning.
            env.entry(OTEL_SERVICE_NAME.to_string())
                .or_insert_with(|| resource_name.clone());
            if let Some(metrics_endpoint) = &monitoring.metrics_endpoint {
                env.insert(
                    OTEL_EXPORTER_OTLP_METRICS_ENDPOINT.to_string(),
                    metrics_endpoint.clone(),
                );
            }
            if is_worker {
                let runtime_secrets = AlienRuntimeSecretsConfig {
                    otlp_logs_auth_header: Some(RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET.to_string()),
                    otlp_metrics_auth_header: monitoring
                        .metrics_endpoint
                        .as_ref()
                        .map(|_| RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET.to_string()),
                    hash: runtime_monitoring_secrets_hash(monitoring),
                };
                let runtime_secrets_json = serde_json::to_string(&runtime_secrets)
                    .into_alien_error()
                    .context(ErrorData::InternalError {
                        message: "Failed to serialize ALIEN_RUNTIME_SECRETS config".to_string(),
                    })?;
                env.insert(ENV_ALIEN_RUNTIME_SECRETS.to_string(), runtime_secrets_json);
            }

            debug!(
                "Injected runtime OTLP monitoring vars into '{}'",
                resource_name
            );
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

/// Inject environment variables into a compute resource (Worker, Container, or Daemon).
///
/// - Plain variables: inserted directly into resource.environment.
/// - Secret variables: collapsed into ALIEN_SECRETS so alien-worker-runtime
///   can fetch them from the vault at startup, unless the platform projects
///   them natively (see `alien_core::SecretDelivery`).
fn inject_into_compute_resource(
    resource_name: &str,
    resource_entry: &mut alien_core::ResourceEntry,
    snapshot: &EnvironmentVariablesSnapshot,
    platform: Platform,
) -> Result<()> {
    if let Some(worker) = resource_entry.config.downcast_mut::<alien_core::Worker>() {
        inject_into_environment(
            resource_name,
            ComputeKind::Worker,
            &mut worker.environment,
            snapshot,
            platform,
        )
    } else if let Some(container) = resource_entry
        .config
        .downcast_mut::<alien_core::Container>()
    {
        inject_into_environment(
            resource_name,
            ComputeKind::Container,
            &mut container.environment,
            snapshot,
            platform,
        )
    } else if let Some(daemon) = resource_entry.config.downcast_mut::<alien_core::Daemon>() {
        inject_into_environment(
            resource_name,
            ComputeKind::Daemon,
            &mut daemon.environment,
            snapshot,
            platform,
        )
    } else {
        Err(AlienError::new(ErrorData::InternalError {
            message: format!(
                "Failed to downcast resource '{}' to a compute resource",
                resource_name
            ),
        }))
    }
}

fn inject_into_environment(
    resource_name: &str,
    kind: ComputeKind,
    environment: &mut HashMap<String, String>,
    snapshot: &EnvironmentVariablesSnapshot,
    platform: Platform,
) -> Result<()> {
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
        // The command token authenticates the hosting runtime itself. It is injected at the
        // platform's final launch boundary and must never become an application-readable vault
        // secret or appear in the application's ALIEN_SECRETS load list.
        .filter(|v| v.name != ENV_ALIEN_COMMANDS_TOKEN)
        .map(|v| v.name.clone())
        .collect();

    if SecretDelivery::resolve(platform, kind).is_native_projection() {
        // The hosting layer projects these secrets natively before process
        // start (Kubernetes secretKeyRef, local supervisor plain env, or native
        // cloud container secret injection); injecting ALIEN_SECRETS here would leak a
        // dangling vault-load pointer into a runtime-less workload.
        if !secret_keys.is_empty() {
            debug!(
                "Skipping ALIEN_SECRETS for {} '{}': {} secret keys are projected by the platform",
                kind.as_str(),
                resource_name,
                secret_keys.len()
            );
        }
        return Ok(());
    }

    // If resource needs secrets, add ALIEN_SECRETS env var
    // alien-worker-runtime will load these from the vault at startup
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

        debug!(
            "Added ALIEN_SECRETS to {} '{}' with {} secret keys",
            kind.as_str(),
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
    let sync_hash = secrets_sync_hash(config);
    let desired_secrets = desired_vault_secrets(config);
    let desired_secret_names = desired_secrets.keys().cloned().collect::<Vec<_>>();

    if client_config.platform() == Platform::Machines {
        debug!("Machines platform syncs workload secrets through the runtime controller");
        runtime_metadata.last_synced_env_vars_hash = Some(sync_hash);
        runtime_metadata.last_synced_secret_names.clear();
        return Ok(false);
    }

    // The hash avoids redundant value writes, while the exact owned-key
    // inventory is what makes deletion safe. Old metadata without the
    // inventory deliberately performs one full reconcile to establish it.
    if let Some(last_synced_hash) = &runtime_metadata.last_synced_env_vars_hash {
        if last_synced_hash == &sync_hash
            && runtime_metadata.last_synced_secret_names == desired_secret_names
        {
            debug!("Secrets already synced for hash {}, skipping", sync_hash);
            return Ok(false);
        }
    }

    let removed_secret_names =
        vault_secret_names_to_remove(&runtime_metadata.last_synced_secret_names, &desired_secrets);

    if desired_secrets.is_empty() && removed_secret_names.is_empty() {
        debug!("No deployment-owned secrets to reconcile in vault");
        runtime_metadata.last_synced_env_vars_hash = Some(sync_hash);
        runtime_metadata.last_synced_secret_names = desired_secret_names;
        return Ok(false);
    }

    info!(
        "Reconciling {} desired and {} removed deployment-owned vault secrets (hash: {})",
        desired_secrets.len(),
        removed_secret_names.len(),
        sync_hash,
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

    // Set desired values first so renames never create a window with neither
    // the old nor new key available. Values are already decrypted.
    for (name, value) in &desired_secrets {
        vault
            .set_secret(name, value)
            .await
            .context(ErrorData::SecretSyncFailed {
                vault_name: "secrets".to_string(),
                reason: format!("Failed to set secret '{name}'"),
            })?;
        debug!("Synced deployment-owned secret '{name}' to vault");
    }

    // Delete names recorded by a previous successful sync. The command token is the sole
    // exception: it is a reserved, control-plane-owned key that pre-v2 sync wrote without an
    // ownership inventory. The sync-schema hash forces one idempotent cleanup after upgrade.
    // Never list or infer any other ownership from the shared vault.
    for name in &removed_secret_names {
        vault
            .delete_secret(name)
            .await
            .context(ErrorData::SecretSyncFailed {
                vault_name: "secrets".to_string(),
                reason: format!("Failed to delete removed secret '{name}'"),
            })?;
        debug!("Deleted removed deployment-owned secret '{name}' from vault");
    }

    // Record ownership only after every mutation succeeds. A partial failure
    // leaves the prior inventory in place so the next reconcile retries.
    runtime_metadata.last_synced_env_vars_hash = Some(sync_hash);
    runtime_metadata.last_synced_secret_names = desired_secret_names;

    info!("Successfully reconciled deployment-owned vault secrets");
    Ok(true)
}

fn desired_vault_secrets(config: &DeploymentConfig) -> BTreeMap<String, String> {
    let mut desired = config
        .environment_variables
        .variables
        .iter()
        .filter(|var| var.var_type == EnvironmentVariableType::Secret)
        .filter(|var| var.name != ENV_ALIEN_COMMANDS_TOKEN)
        .map(|var| (var.name.clone(), var.value.clone()))
        .collect::<BTreeMap<_, _>>();

    if let Some(monitoring) = &config.monitoring {
        desired.insert(
            RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET.to_string(),
            monitoring.logs_auth_header.clone(),
        );
        if monitoring.metrics_endpoint.is_some() {
            desired.insert(
                RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET.to_string(),
                monitoring
                    .metrics_auth_header
                    .clone()
                    .unwrap_or_else(|| monitoring.logs_auth_header.clone()),
            );
        }
    }

    desired
}

fn vault_secret_names_to_remove(
    previously_owned: &[String],
    desired: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut removed = previously_owned
        .iter()
        .filter(|name| !desired.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    if !removed.iter().any(|name| name == ENV_ALIEN_COMMANDS_TOKEN) {
        removed.push(ENV_ALIEN_COMMANDS_TOKEN.to_string());
    }
    removed
}

fn runtime_monitoring_secrets_hash(monitoring: &OtlpConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(monitoring.logs_auth_header.as_bytes());
    if monitoring.metrics_endpoint.is_some() {
        hasher.update(b"\0metrics\0");
        hasher.update(
            monitoring
                .metrics_auth_header
                .as_ref()
                .unwrap_or(&monitoring.logs_auth_header)
                .as_bytes(),
        );
    }
    format!("{:x}", hasher.finalize())
}

fn secrets_sync_hash(config: &DeploymentConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SECRETS_SYNC_SCHEMA_VERSION);
    hasher.update(config.environment_variables.hash.as_bytes());
    if let Some(monitoring) = &config.monitoring {
        hasher.update(b"\0runtime-monitoring\0");
        hasher.update(runtime_monitoring_secrets_hash(monitoring).as_bytes());
    }
    format!("{:x}", hasher.finalize())
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

    const OTEL_EXPORTER_OTLP_HEADERS: &str = "OTEL_EXPORTER_OTLP_HEADERS";
    const OTEL_EXPORTER_OTLP_METRICS_HEADERS: &str = "OTEL_EXPORTER_OTLP_METRICS_HEADERS";

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
        RuntimeMetadata, StackResourceState, StackSettings, Vault, VaultBinding, Worker,
        WorkerCode,
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
                gates: Vec::new(),
            },
            supported_platforms: None,
            inputs: Vec::new(),
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
            &[
                ("SECRET_TOKEN", "st"),
                ("SECRET_KEY", "sk"),
                (ENV_ALIEN_COMMANDS_TOKEN, "runtime-only"),
            ],
        );
        let config = make_config(snapshot);
        let mut stack = make_single_function_stack("worker");

        inject_environment_variables(&mut stack, &config, Platform::Aws).unwrap();

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
        assert!(!parsed.keys.contains(&ENV_ALIEN_COMMANDS_TOKEN.to_string()));
        assert_eq!(parsed.hash, "test-hash");

        // Secret values NOT injected as plain env vars
        assert!(!func.environment.contains_key("SECRET_TOKEN"));
        assert!(!func.environment.contains_key("SECRET_KEY"));
        assert!(!func.environment.contains_key(ENV_ALIEN_COMMANDS_TOKEN));
    }

    #[test]
    fn test_inject_no_alien_secrets_when_only_plain_vars() {
        let snapshot = make_snapshot(&[("APP_ENV", "prod")], &[]);
        let config = make_config(snapshot);
        let mut stack = make_single_function_stack("worker");

        inject_environment_variables(&mut stack, &config, Platform::Aws).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        assert_eq!(func.environment.get("APP_ENV").unwrap(), "prod");
        assert!(!func.environment.contains_key(ENV_ALIEN_SECRETS));
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

        inject_environment_variables(&mut stack, &config, Platform::Aws).unwrap();

        let func = stack
            .resources
            .get("worker")
            .unwrap()
            .config
            .downcast_ref::<Worker>()
            .unwrap();

        // Secret targeted at "other-fn" should NOT produce ALIEN_SECRETS on "worker"
        assert!(!func.environment.contains_key(ENV_ALIEN_SECRETS));
    }

    #[test]
    fn kubernetes_projects_all_compute_secrets_natively() {
        let snapshot = make_snapshot(&[("APP_ENV", "prod")], &[("APP_SECRET", "s3cret")]);
        let config = make_config(snapshot);
        let mut stack = make_compute_stack();

        inject_environment_variables(&mut stack, &config, Platform::Kubernetes).unwrap();

        // Kubernetes controllers project all compute secrets via secretKeyRef:
        // no pointer and no raw value enters the resource config.
        for id in ["worker", "web", "agent"] {
            let env = resource_env(&stack, id);
            assert!(
                !env.contains_key(ENV_ALIEN_SECRETS),
                "'{id}' must not get ALIEN_SECRETS on Kubernetes"
            );
            assert!(
                !env.contains_key("APP_SECRET"),
                "'{id}' must not get the raw secret value"
            );
            assert_eq!(
                env.get("APP_ENV").unwrap(),
                "prod",
                "'{id}' still gets plain vars"
            );
        }
    }

    #[test]
    fn vault_pointer_is_limited_to_worker_hosts_without_native_projection() {
        // Runtime-less Containers/Daemons have nothing that could load the
        // ALIEN_SECRETS pointer; the hosting layer projects their secrets
        // natively before process start on every platform.
        // Workers keep it only on hosts without native projection.
        for platform in [
            Platform::Local,
            Platform::Test,
            Platform::Aws,
            Platform::Kubernetes,
            Platform::Machines,
        ] {
            let snapshot = make_snapshot(&[], &[("APP_SECRET", "s3cret")]);
            let config = make_config(snapshot);
            let mut stack = make_compute_stack();

            inject_environment_variables(&mut stack, &config, platform).unwrap();

            let worker_env = resource_env(&stack, "worker");
            assert_eq!(
                worker_env.contains_key(ENV_ALIEN_SECRETS),
                !matches!(platform, Platform::Kubernetes | Platform::Machines),
                "{platform:?}: Worker pointer must match host delivery"
            );
            for id in ["web", "agent"] {
                let env = resource_env(&stack, id);
                assert!(
                    !env.contains_key(ENV_ALIEN_SECRETS),
                    "{platform:?}: runtime-less '{id}' must never receive ALIEN_SECRETS"
                );
            }
        }
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

        inject_monitoring_environment_variables(&mut stack, &monitoring, Platform::Aws).unwrap();

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

        inject_monitoring_environment_variables(&mut stack, &monitoring, Platform::Aws).unwrap();

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

    fn make_compute_stack() -> Stack {
        let mut stack = make_single_function_stack("worker");

        let container = alien_core::Container::new("web".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "256Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .permissions("default".to_string())
            .build();
        stack.resources.insert(
            "web".to_string(),
            ResourceEntry {
                config: Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let daemon = alien_core::Daemon::new("agent".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        stack.resources.insert(
            "agent".to_string(),
            ResourceEntry {
                config: Resource::new(daemon),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        stack
    }

    fn make_monitoring_with_metrics() -> OtlpConfig {
        OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer logs-token".to_string(),
            metrics_endpoint: Some("https://manager.test/v1/metrics".to_string()),
            metrics_auth_header: None,
            resource_attributes: std::collections::HashMap::new(),
        }
    }

    fn pre_v2_snapshot_only_sync_hash(config: &DeploymentConfig) -> String {
        let mut hasher = Sha256::new();
        hasher.update(config.environment_variables.hash.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    #[tokio::test]
    async fn machines_skips_stack_vault_secret_sync() {
        let mut config = make_config(make_snapshot(&[], &[("API_TOKEN", "secret")]));
        config.monitoring = Some(make_monitoring_with_metrics());
        let expected_hash = secrets_sync_hash(&config);
        let mut runtime_metadata = RuntimeMetadata::default();

        let synced = sync_secrets_to_vault(
            &StackState::new(Platform::Machines),
            &ClientConfig::Machines,
            &config,
            &mut runtime_metadata,
        )
        .await
        .expect("Machines should not require a stack secrets vault");

        assert!(!synced);
        assert_eq!(
            runtime_metadata.last_synced_env_vars_hash.as_deref(),
            Some(expected_hash.as_str())
        );
        assert!(runtime_metadata.last_synced_secret_names.is_empty());
    }

    #[test]
    fn vault_reconcile_deletes_owned_names_and_reserved_legacy_command_token() {
        let mut config = make_config(make_snapshot(&[], &[("KEEP", "current")]));
        config.monitoring = Some(OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer current".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: HashMap::new(),
        });
        let desired = desired_vault_secrets(&config);
        let previously_owned = vec![
            "KEEP".to_string(),
            "REMOVED".to_string(),
            RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET.to_string(),
            RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET.to_string(),
        ];

        let removed = vault_secret_names_to_remove(&previously_owned, &desired);

        assert_eq!(
            removed,
            vec![
                "REMOVED".to_string(),
                RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET.to_string(),
                ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            ]
        );
        assert!(desired.contains_key("KEEP"));
        assert!(desired.contains_key(RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET));
        assert!(!removed.contains(&"UNRELATED".to_string()));
    }

    #[test]
    fn token_only_legacy_metadata_cannot_take_the_empty_reconcile_fast_path() {
        let removed = vault_secret_names_to_remove(&[], &BTreeMap::new());
        assert_eq!(removed, vec![ENV_ALIEN_COMMANDS_TOKEN.to_string()]);
    }

    #[test]
    fn desired_vault_secrets_excludes_runtime_command_token() {
        let config = make_config(make_snapshot(
            &[],
            &[
                ("APP_SECRET", "app-value"),
                (ENV_ALIEN_COMMANDS_TOKEN, "runtime-only"),
            ],
        ));

        let desired = desired_vault_secrets(&config);

        assert_eq!(desired.get("APP_SECRET"), Some(&"app-value".to_string()));
        assert!(!desired.contains_key(ENV_ALIEN_COMMANDS_TOKEN));
    }

    #[tokio::test]
    async fn vault_sync_removes_stale_owned_values_and_preserves_unrelated_values() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let state_dir = temp.path().to_string_lossy().to_string();
        let client_config = ClientConfig::Local {
            state_directory: state_dir.clone(),
        };

        let mut vault_state = StackResourceState::new_pending(
            Vault::RESOURCE_TYPE.to_string(),
            Resource::new(Vault::new("secrets".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        vault_state.status = ResourceStatus::Running;
        vault_state.remote_binding_params = Some(
            serde_json::to_value(VaultBinding::local("secrets", &state_dir))
                .expect("local vault binding"),
        );
        let mut stack_state = StackState::new(Platform::Local);
        stack_state
            .resources
            .insert("secrets".to_string(), vault_state);

        let provider = BindingsProvider::from_stack_state(&stack_state, client_config.clone())
            .expect("bindings provider");
        let vault = provider.load_vault("secrets").await.expect("local vault");
        vault
            .set_secret("UNRELATED", "keep-me")
            .await
            .expect("seed unrelated value");
        vault
            .set_secret(ENV_ALIEN_COMMANDS_TOKEN, "legacy-runtime-token")
            .await
            .expect("seed previously synced runtime token");

        let mut first = make_config(make_snapshot(
            &[],
            &[
                ("KEEP", "v1"),
                ("REMOVE", "old"),
                (ENV_ALIEN_COMMANDS_TOKEN, "current-runtime-token"),
            ],
        ));
        first.environment_variables.hash = "first".to_string();
        first.monitoring = Some(make_monitoring_with_metrics());
        let mut metadata = RuntimeMetadata::default();
        // This is the real pre-inventory upgrade case: the old hash exists, but there is no list
        // of owned names. The reserved token still has to be deleted from the shared vault.
        metadata.last_synced_env_vars_hash = Some("legacy".to_string());
        metadata.last_synced_secret_names.clear();
        assert!(
            sync_secrets_to_vault(&stack_state, &client_config, &first, &mut metadata)
                .await
                .expect("first sync")
        );
        assert!(vault.get_secret(ENV_ALIEN_COMMANDS_TOKEN).await.is_err());
        assert_eq!(
            vault
                .get_secret("KEEP")
                .await
                .expect("ordinary app secret remains available"),
            "v1"
        );
        assert!(!metadata
            .last_synced_secret_names
            .contains(&ENV_ALIEN_COMMANDS_TOKEN.to_string()));

        let mut second = make_config(make_snapshot(&[], &[("KEEP", "v2")]));
        second.environment_variables.hash = "second".to_string();
        second.monitoring = Some(OtlpConfig {
            logs_endpoint: "https://manager.test/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer next".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: HashMap::new(),
        });
        assert!(
            sync_secrets_to_vault(&stack_state, &client_config, &second, &mut metadata)
                .await
                .expect("second sync")
        );

        assert_eq!(vault.get_secret("KEEP").await.expect("kept value"), "v2");
        assert_eq!(
            vault
                .get_secret("UNRELATED")
                .await
                .expect("unrelated value"),
            "keep-me"
        );
        assert!(vault.get_secret("REMOVE").await.is_err());
        assert!(vault
            .get_secret(RUNTIME_OTLP_METRICS_AUTH_HEADER_SECRET)
            .await
            .is_err());
        assert_eq!(
            metadata.last_synced_secret_names,
            vec![
                "KEEP".to_string(),
                RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET.to_string(),
            ]
        );

        let mut third = make_config(make_snapshot(&[], &[]));
        third.environment_variables.hash = "third".to_string();
        assert!(
            sync_secrets_to_vault(&stack_state, &client_config, &third, &mut metadata)
                .await
                .expect("delete-only sync")
        );
        assert!(vault.get_secret("KEEP").await.is_err());
        assert!(vault
            .get_secret(RUNTIME_OTLP_LOGS_AUTH_HEADER_SECRET)
            .await
            .is_err());
        assert_eq!(
            vault
                .get_secret("UNRELATED")
                .await
                .expect("unrelated survives delete-only sync"),
            "keep-me"
        );
        assert!(metadata.last_synced_secret_names.is_empty());

        // A deployment upgraded from the pre-v2 sync can have only a stored hash: there was no
        // owned-name inventory, and the token may be its only secret. The schema hash must force
        // this reconcile past the empty-desired fast path and delete the reserved legacy value.
        vault
            .set_secret(ENV_ALIEN_COMMANDS_TOKEN, "legacy-token-only")
            .await
            .expect("reseed legacy token-only value");
        let mut token_only = make_config(make_snapshot(
            &[],
            &[(ENV_ALIEN_COMMANDS_TOKEN, "current-runtime-token")],
        ));
        token_only.environment_variables.hash = "token-only".to_string();
        let old_hash = pre_v2_snapshot_only_sync_hash(&token_only);
        let policy_hash = secrets_sync_hash(&token_only);
        assert_ne!(old_hash, policy_hash);
        let mut legacy_metadata = RuntimeMetadata::default();
        legacy_metadata.last_synced_env_vars_hash = Some(old_hash);

        assert!(sync_secrets_to_vault(
            &stack_state,
            &client_config,
            &token_only,
            &mut legacy_metadata,
        )
        .await
        .expect("legacy token-only cleanup"));
        assert!(vault.get_secret(ENV_ALIEN_COMMANDS_TOKEN).await.is_err());
        assert!(legacy_metadata.last_synced_secret_names.is_empty());
        assert_eq!(
            legacy_metadata.last_synced_env_vars_hash.as_deref(),
            Some(policy_hash.as_str())
        );
        assert!(!sync_secrets_to_vault(
            &stack_state,
            &client_config,
            &token_only,
            &mut legacy_metadata,
        )
        .await
        .expect("already-clean token-only sync is idempotent"));
        assert!(vault.get_secret(ENV_ALIEN_COMMANDS_TOKEN).await.is_err());
    }

    fn resource_env<'a>(stack: &'a Stack, id: &str) -> &'a HashMap<String, String> {
        let entry = &stack.resources.get(id).expect("resource exists").config;
        if let Some(worker) = entry.downcast_ref::<Worker>() {
            &worker.environment
        } else if let Some(container) = entry.downcast_ref::<alien_core::Container>() {
            &container.environment
        } else if let Some(daemon) = entry.downcast_ref::<alien_core::Daemon>() {
            &daemon.environment
        } else {
            panic!("resource '{id}' is not a compute resource");
        }
    }

    #[test]
    fn monitoring_cloud_platforms_inject_worker_only() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let mut stack = make_compute_stack();
            let monitoring = make_monitoring_with_metrics();

            inject_monitoring_environment_variables(&mut stack, &monitoring, platform).unwrap();

            let worker_env = resource_env(&stack, "worker");
            assert_eq!(
                worker_env.get(OTEL_EXPORTER_OTLP_LOGS_ENDPOINT).unwrap(),
                "https://manager.test/v1/logs",
                "{platform:?}: worker gets the logs endpoint"
            );
            assert!(
                worker_env.contains_key(ENV_ALIEN_RUNTIME_SECRETS),
                "{platform:?}: worker gets the runtime-secrets pointer"
            );

            for id in ["web", "agent"] {
                let env = resource_env(&stack, id);
                assert!(
                    env.is_empty(),
                    "{platform:?}: '{id}' must get no OTLP vars (got {:?})",
                    env.keys().collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn monitoring_self_hosted_platforms_inject_all_compute() {
        for platform in [Platform::Kubernetes, Platform::Local, Platform::Test] {
            let mut stack = make_compute_stack();
            let monitoring = make_monitoring_with_metrics();

            inject_monitoring_environment_variables(&mut stack, &monitoring, platform).unwrap();

            for id in ["worker", "web", "agent"] {
                let env = resource_env(&stack, id);
                assert_eq!(
                    env.get(OTEL_EXPORTER_OTLP_LOGS_ENDPOINT).unwrap(),
                    "https://manager.test/v1/logs",
                    "{platform:?}: '{id}' gets the logs endpoint"
                );
                assert_eq!(
                    env.get(OTEL_EXPORTER_OTLP_METRICS_ENDPOINT).unwrap(),
                    "https://manager.test/v1/metrics",
                    "{platform:?}: '{id}' gets the metrics endpoint"
                );
                assert_eq!(
                    env.get(OTEL_SERVICE_NAME).unwrap(),
                    id,
                    "{platform:?}: '{id}' gets its name as service name"
                );
            }

            let worker_env = resource_env(&stack, "worker");
            assert!(
                worker_env.contains_key(ENV_ALIEN_RUNTIME_SECRETS),
                "{platform:?}: Worker gets the runtime-secrets pointer"
            );

            for id in ["web", "agent"] {
                let env = resource_env(&stack, id);
                assert!(
                    !env.contains_key(ENV_ALIEN_RUNTIME_SECRETS),
                    "{platform:?}: runtime-less '{id}' must not get a runtime-secrets pointer"
                );
                for header in [
                    OTEL_EXPORTER_OTLP_HEADERS,
                    OTEL_EXPORTER_OTLP_METRICS_HEADERS,
                ] {
                    assert!(
                        !env.contains_key(header),
                        "{platform:?}: runtime-less '{id}' must not store raw monitoring header '{header}' in resource config"
                    );
                }
            }
        }
    }
}

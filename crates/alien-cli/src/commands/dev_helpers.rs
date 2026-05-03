//! Helper functions for dev mode operation
//!
//! These functions handle building, posting releases, and creating deployments
//! for the dev server. They're used by the main CLI router for dev mode operations.
//!
//! Dev mode is local-only — it always uses `Platform::Local`.

use crate::{
    config::load_configuration,
    error::{ErrorData, Result},
    get_current_dir,
    output::write_json_file,
};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::{
    AgentStatus, BinaryTarget, DeploymentStatus, DevResourceInfo, DevStatus, DevStatusState, Stack,
    StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager::{
    providers::{
        in_memory_telemetry::InMemoryTelemetryBackend, local_credentials::LocalCredentialResolver,
        permissive_auth::PermissiveAuthValidator,
    },
    LogBuffer,
};
use alien_manager_api::types::{CreateReleaseRequest, StackByPlatform};
use alien_manager_api::Client as AlienManagerClient;
use alien_manager_api::SdkResultExt;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

/// Parsed CLI environment variable.
#[derive(Debug, Clone)]
pub struct CliEnvVar {
    pub name: String,
    pub value: String,
    pub is_secret: bool,
    pub target_resources: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevDeploymentSnapshot {
    pub deployment_id: String,
    pub deployment_name: String,
    pub status: DeploymentStatus,
    pub commands_url: String,
    pub resources: HashMap<String, DevResourceInfo>,
}

#[derive(Debug, Clone)]
pub struct DevDeploymentLiveState {
    pub deployment_id: String,
    pub deployment_name: String,
    pub status: DeploymentStatus,
    pub current_release_id: Option<String>,
    pub resources: HashMap<String, DevResourceInfo>,
    pub stack_state: Option<StackState>,
    pub error: Option<serde_json::Value>,
}

/// Check if dev server is healthy
pub async fn check_server_health(port: u16) -> bool {
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));
    client.health().send().await.is_ok()
}

/// Ensure the dev server is running (start if not)
pub async fn ensure_server_running(port: u16) -> Result<()> {
    ensure_server_running_with_env(port, None, Vec::new()).await
}

/// Ensure the dev server is running for the full `alien dev` session.
///
/// When we need to start a fresh embedded manager, clear stale runtime state so
/// deployment recovery from older sessions does not leak into the new run.
pub async fn ensure_server_running_for_dev_session(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    ensure_server_running_internal(port, status_file, user_env_vars, true).await
}

/// Ensure the dev server is running with user-provided env vars and optional status file (start if not)
pub async fn ensure_server_running_with_env(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    ensure_server_running_internal(port, status_file, user_env_vars, false).await
}

async fn ensure_server_running_internal(
    port: u16,
    status_file: Option<PathBuf>,
    _user_env_vars: Vec<CliEnvVar>,
    reset_state_on_start: bool,
) -> Result<()> {
    if check_server_health(port).await {
        if reset_state_on_start {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "port".to_string(),
                message: format!(
                    "Another `alien dev` manager is already running on port {port}. Stop it first (Ctrl+C in that terminal, or kill the process), then rerun `alien dev`. If you need multiple sessions, use a different port with `alien dev --port <port>`."
                ),
            }));
        }

        info!("Dev server already running on port {}", port);
        ensure_local_dev_deployment_group(port).await?;
        if let Some(status_file) = status_file {
            write_dev_status(
                &status_file,
                &build_dev_status(port, DevStatusState::Initializing, None, None),
            )?;
        }
        return Ok(());
    }

    if let Some(status_file) = status_file {
        write_dev_status(
            &status_file,
            &build_dev_status(port, DevStatusState::Initializing, None, None),
        )?;
    }

    if reset_state_on_start {
        reset_local_dev_runtime_state()?;
    }

    ensure_dev_port_available(port)?;

    start_embedded_dev_manager(port).await
}

fn ensure_dev_port_available(port: u16) -> Result<()> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "port".to_string(),
                message: format!(
                    "Port {port} is already in use by another process. Free the port (or stop the process) and rerun `alien dev`, or start on a different port with `alien dev --port <port>`."
                ),
            }))
        }
        Err(error) => Err(AlienError::new(ErrorData::NetworkError {
            message: format!("Failed to bind local dev server to port {port}: {error}"),
        })),
    }
}

fn reset_local_dev_runtime_state() -> Result<()> {
    let state_dir = get_current_dir()?.join(".alien");
    if !state_dir.exists() {
        return Ok(());
    }

    for db_file in ["dev-server.db", "dev-server.db-shm", "dev-server.db-wal"] {
        let path = state_dir.join(db_file);
        if path.exists() {
            std::fs::remove_file(&path).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "remove file".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to reset local dev manager database".to_string(),
                },
            )?;
        }
    }

    for runtime_dir in ["commands_kv", "commands_storage"] {
        let path = state_dir.join(runtime_dir);
        if path.exists() {
            std::fs::remove_dir_all(&path).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "remove directory".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to reset local dev runtime state".to_string(),
                },
            )?;
        }
    }

    let entries = std::fs::read_dir(&state_dir).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read directory".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to scan local dev state directory".to_string(),
        },
    )?;

    for entry in entries {
        let entry = entry
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read directory entry".to_string(),
                file_path: state_dir.display().to_string(),
                reason: "Failed to inspect local dev state entry".to_string(),
            })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if name.starts_with("dep_") {
            std::fs::remove_dir_all(&path).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "remove directory".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to remove stale local deployment state".to_string(),
                },
            )?;
        }
    }

    Ok(())
}

/// Build the embedded dev manager instance used by `alien dev`.
///
/// This is a standalone manager with dev-friendly defaults:
/// - Permissive auth (no tokens needed)
/// - In-memory telemetry (for dev UI log streaming)
/// - Local credential resolution
/// - Binds to localhost only
pub async fn build_embedded_dev_manager(
    port: u16,
) -> Result<(alien_manager::AlienManager, SocketAddr)> {
    use alien_manager::standalone_config::ManagerTomlConfig;

    let state_dir = get_current_dir()?.join(".alien");
    std::fs::create_dir_all(&state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to create .alien directory".to_string(),
        })?;

    let config = alien_manager::ManagerConfig {
        port,
        db_path: Some(state_dir.join("dev-server.db")),
        state_dir: Some(state_dir.clone()),
        enable_local_log_ingest: true,
        response_signing_key: b"alien-dev-commands-signing-key".to_vec(),
        ..Default::default()
    };

    let log_buffer = Arc::new(LogBuffer::new());
    let toml_config = ManagerTomlConfig::default();

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let server = alien_manager::AlienManager::builder(config)
        .credential_resolver(Arc::new(LocalCredentialResolver::new(state_dir)))
        .telemetry_backend(Arc::new(InMemoryTelemetryBackend::new(log_buffer.clone())))
        .auth_validator(Arc::new(PermissiveAuthValidator::new()))
        .log_buffer(log_buffer)
        .with_standalone_defaults(&toml_config)
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to set up dev server defaults".to_string(),
        })?
        .build()
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to initialize dev server".to_string(),
        })?;

    Ok((server, addr))
}

/// Start the embedded dev manager in the background and wait for it to be healthy.
pub async fn start_embedded_dev_manager(port: u16) -> Result<()> {
    info!("Starting dev server on port {}...", port);
    let (server, addr) = build_embedded_dev_manager(port).await?;

    tokio::spawn(async move {
        if let Err(e) = server.start(addr).await {
            tracing::error!("Dev server error: {}", e);
        }
    });

    wait_for_dev_server_ready(port).await?;
    ensure_local_dev_deployment_group(port).await?;
    info!("Dev server ready");

    Ok(())
}

fn local_dev_client(port: u16) -> AlienManagerClient {
    AlienManagerClient::new(&format!("http://localhost:{port}"))
}

pub(crate) async fn wait_for_dev_server_ready(port: u16) -> Result<()> {
    for _ in 0..50 {
        if check_server_health(port).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(AlienError::new(ErrorData::ServerStartFailed {
        reason: "Timeout waiting for dev server to start".to_string(),
    }))
}

pub(crate) async fn ensure_local_dev_deployment_group(port: u16) -> Result<()> {
    let client = local_dev_client(port);

    let response = client
        .list_deployment_groups()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list dev deployment groups".to_string(),
            url: None,
        })?;

    if response.items.iter().any(|group| group.name == "local-dev") {
        return Ok(());
    }

    client
        .create_deployment_group()
        .body_map(|body| body.name("local-dev").max_deployments(100))
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create default dev deployment group".to_string(),
            url: None,
        })?;

    info!("Created default 'local-dev' deployment group");

    Ok(())
}

/// Build and post a release to the dev server for the local `alien dev` flow.
///
/// Always builds for the local platform — dev mode is local-only.
/// Reads the stack from `.alien/build/local/stack.json`.
pub async fn build_and_post_release_simple(
    current_dir: &PathBuf,
    port: u16,
    skip_build: bool,
    config_file: Option<&PathBuf>,
) -> Result<String> {
    let output_dir = current_dir.join(".alien");

    // Build if needed
    if !skip_build {
        info!("Building stack for local platform...");
        let config_path = match config_file {
            Some(cf) if cf.is_relative() => current_dir.join(cf),
            Some(cf) => cf.clone(),
            None => current_dir.clone(),
        };
        let stack =
            load_configuration(config_path)
                .await
                .context(ErrorData::ConfigurationError {
                    message: "Failed to load configuration".to_string(),
                })?;

        let settings = BuildSettings {
            output_directory: output_dir.to_str().unwrap().to_string(),
            platform: PlatformBuildSettings::Local {},
            targets: Some(vec![BinaryTarget::current_os()]),
            cache_url: None,
            override_base_image: None,
            debug_mode: true,
        };

        alien_build::build_stack(stack, &settings)
            .await
            .context(ErrorData::BuildFailed)?;
    }

    // Load the built stack
    let stack_file = output_dir.join("build").join("local").join("stack.json");
    let stack: Stack = serde_json::from_str(
        &std::fs::read_to_string(&stack_file)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: stack_file.display().to_string(),
                reason: "Failed to read stack.json for local platform".to_string(),
            })?,
    )
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "deserialize".to_string(),
        reason: "Failed to parse stack.json".to_string(),
    })?;

    // Post release to dev server
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));

    let stack_json =
        serde_json::to_value(&stack)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "Failed to serialize stack".to_string(),
            })?;

    let stack_by_platform = StackByPlatform {
        aws: None,
        gcp: None,
        azure: None,
        kubernetes: None,
        local: Some(stack_json),
        test: None,
    };

    let response = client
        .create_release()
        .body(CreateReleaseRequest {
            stack: stack_by_platform,
            git_metadata: None,
            // dev mode is single-project; "default" is the canonical
            // sentinel and is required by the wire schema.
            project_id: "default".to_string(),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create release on dev server".to_string(),
            url: None,
        })?;

    Ok(response.id.clone())
}

/// Create initial deployment if it doesn't exist.
///
/// Always creates a local-platform deployment — dev mode is local-only.
/// Accepts optional environment variables to include in the deployment request.
pub async fn create_initial_deployment(
    deployment_name: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
) -> Result<String> {
    let client = local_dev_client(port);

    // Check if deployment exists
    let list_response = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list deployments".to_string(),
            url: None,
        })?;

    if let Some(existing) = list_response
        .items
        .iter()
        .find(|d| d.name == deployment_name)
    {
        info!("Deployment '{}' already exists", deployment_name);
        return Ok(existing.id.clone());
    }

    // Create deployment
    info!("Creating initial deployment '{}'...", deployment_name);

    let env_vars: Option<Vec<alien_manager_api::types::EnvironmentVariable>> =
        environment_variables.map(|vars| {
            vars.into_iter()
                .map(|v| {
                    let var_type = match v.var_type {
                        alien_core::EnvironmentVariableType::Secret => {
                            alien_manager_api::types::EnvironmentVariableType::Secret
                        }
                        alien_core::EnvironmentVariableType::Plain => {
                            alien_manager_api::types::EnvironmentVariableType::Plain
                        }
                    };
                    alien_manager_api::types::EnvironmentVariable {
                        name: v.name,
                        value: v.value,
                        type_: var_type,
                        target_resources: v.target_resources,
                    }
                })
                .collect()
        });

    let response = client
        .create_deployment()
        .body_map(|body| {
            let mut b = body
                .name(deployment_name)
                .platform(alien_manager_api::types::Platform::Local);
            if let Some(ref vars) = env_vars {
                b = b.environment_variables(vars.clone());
            }
            b
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment".to_string(),
            url: None,
        })?;

    let deployment_id = response.deployment.id.clone();
    info!("Deployment '{}' created", deployment_name);
    Ok(deployment_id)
}

/// Prepare the deployment used by the full `alien dev` session.
///
/// Unlike ad hoc deployment subcommands, the main dev loop should own a fresh
/// deployment so stale local processes and incompatible stack state do not leak
/// across runs.
pub async fn prepare_dev_session_deployment(
    deployment_name: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
) -> Result<String> {
    let client = local_dev_client(port);

    if let Some(existing) = find_named_local_deployment(&client, deployment_name).await? {
        info!(
            "Refreshing existing local deployment '{}' ({})",
            deployment_name, existing.id
        );

        client
            .delete_deployment()
            .id(&existing.id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!(
                    "Failed to delete existing local deployment '{}'",
                    deployment_name
                ),
                url: None,
            })?;

        wait_for_local_deployment_absent(port, deployment_name).await?;
    }

    create_initial_deployment(deployment_name, port, environment_variables).await
}

async fn find_named_local_deployment(
    client: &AlienManagerClient,
    deployment_name: &str,
) -> Result<Option<alien_manager_api::types::DeploymentResponse>> {
    let list_response = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list deployments".to_string(),
            url: None,
        })?;

    let inner = list_response.into_inner();
    Ok(inner
        .items
        .into_iter()
        .find(|deployment| deployment.name == deployment_name))
}

async fn wait_for_local_deployment_absent(port: u16, deployment_name: &str) -> Result<()> {
    let client = local_dev_client(port);
    for _ in 0..30 {
        if find_named_local_deployment(&client, deployment_name)
            .await?
            .is_none()
        {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(AlienError::new(ErrorData::ConfigurationError {
        message: format!(
            "Timed out deleting existing local deployment '{}'",
            deployment_name
        ),
    }))
}

use alien_manager_api::types::{DeploymentInfoResponse, DeploymentResponse};

pub async fn wait_for_dev_deployment_ready(
    port: u16,
    deployment_name: &str,
    status_file: Option<&PathBuf>,
) -> Result<DevDeploymentSnapshot> {
    wait_for_dev_deployment_ready_with_progress(port, deployment_name, status_file, |_| {}).await
}

pub async fn wait_for_dev_deployment_ready_with_progress<F>(
    port: u16,
    deployment_name: &str,
    status_file: Option<&PathBuf>,
    mut on_status: F,
) -> Result<DevDeploymentSnapshot>
where
    F: FnMut(DeploymentStatus),
{
    let client = local_dev_client(port);

    for _ in 0..180 {
        if let Ok(list_response) = client.list_deployments().send().await {
            if let Some(deployment) = list_response
                .items
                .iter()
                .find(|d| d.name == deployment_name)
            {
                if let Ok(info_response) =
                    client.get_deployment_info().id(&deployment.id).send().await
                {
                    let info = info_response.into_inner();
                    let snapshot = snapshot_from_info(&info, deployment_name)?;

                    if let Some(status_file) = status_file {
                        let state = if snapshot.status == DeploymentStatus::Running {
                            DevStatusState::Ready
                        } else {
                            DevStatusState::Initializing
                        };
                        write_dev_status(
                            status_file,
                            &build_dev_status(port, state, Some(&snapshot), None),
                        )?;
                    }

                    on_status(snapshot.status.clone());

                    if snapshot.status == DeploymentStatus::Running {
                        return Ok(snapshot);
                    }

                    if snapshot.status.is_failed() {
                        let error_detail =
                            info.error.map(|e| format!(": {}", e)).unwrap_or_default();
                        return Err(AlienError::new(ErrorData::ConfigurationError {
                            message: format!(
                                "Local deployment '{}' failed with status {:?}{}",
                                snapshot.deployment_name, snapshot.status, error_detail
                            ),
                        }));
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Err(AlienError::new(ErrorData::ConfigurationError {
        message: "Timed out waiting for local deployment to become ready".to_string(),
    }))
}

/// Convert a DeploymentInfoResponse into a DevDeploymentSnapshot.
fn snapshot_from_info(
    info: &DeploymentInfoResponse,
    deployment_name: &str,
) -> Result<DevDeploymentSnapshot> {
    Ok(DevDeploymentSnapshot {
        deployment_id: info.commands.deployment_id.clone(),
        deployment_name: deployment_name.to_string(),
        status: parse_deployment_status(&info.status)?,
        commands_url: info.commands.url.clone(),
        resources: info
            .resources
            .iter()
            .filter_map(|(name, resource)| {
                resource.public_url.as_ref().map(|url| {
                    (
                        name.clone(),
                        DevResourceInfo {
                            url: url.clone(),
                            resource_type: Some(resource.resource_type.clone()),
                        },
                    )
                })
            })
            .collect(),
    })
}

pub async fn fetch_dev_deployment_live_state(
    port: u16,
    deployment_name: &str,
) -> Result<Option<DevDeploymentLiveState>> {
    let client = local_dev_client(port);

    let list_response = match client.list_deployments().send().await {
        Ok(response) => response.into_inner(),
        Err(_) => return Ok(None),
    };

    let deployment = match list_response
        .items
        .iter()
        .find(|d| d.name == deployment_name)
    {
        Some(d) => d.clone(),
        None => return Ok(None),
    };

    live_state_from_deployment(&client, &deployment).await
}

pub async fn fetch_all_dev_deployment_live_states(
    port: u16,
) -> Result<Vec<DevDeploymentLiveState>> {
    let client = local_dev_client(port);

    let list_response = match client.list_deployments().send().await {
        Ok(response) => response.into_inner(),
        Err(_) => return Ok(Vec::new()),
    };

    let mut states = Vec::new();
    for deployment in &list_response.items {
        if let Some(state) = live_state_from_deployment(&client, deployment).await? {
            states.push(state);
        }
    }

    states.sort_by(|left, right| left.deployment_name.cmp(&right.deployment_name));
    Ok(states)
}

async fn live_state_from_deployment(
    client: &AlienManagerClient,
    deployment: &DeploymentResponse,
) -> Result<Option<DevDeploymentLiveState>> {
    let info: Option<DeploymentInfoResponse> = client
        .get_deployment_info()
        .id(&deployment.id)
        .send()
        .await
        .ok()
        .map(|r| r.into_inner());

    let stack_state = deployment
        .stack_state
        .as_ref()
        .map(|value| {
            serde_json::from_value(value.clone())
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialize".to_string(),
                    reason: "Failed to parse local stack state".to_string(),
                })
        })
        .transpose()?;

    let resources = info
        .as_ref()
        .map(|info| {
            info.resources
                .iter()
                .filter_map(|(name, resource)| {
                    resource.public_url.as_ref().map(|url| {
                        (
                            name.clone(),
                            DevResourceInfo {
                                url: url.clone(),
                                resource_type: Some(resource.resource_type.clone()),
                            },
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(DevDeploymentLiveState {
        deployment_id: deployment.id.clone(),
        deployment_name: deployment.name.clone(),
        status: parse_deployment_status(&deployment.status)?,
        current_release_id: deployment.current_release_id.clone(),
        resources,
        stack_state,
        error: deployment
            .error
            .clone()
            .or_else(|| info.and_then(|i| i.error)),
    }))
}

pub fn build_dev_status(
    port: u16,
    status: DevStatusState,
    snapshot: Option<&DevDeploymentSnapshot>,
    error: Option<AlienError>,
) -> DevStatus {
    let state_dir = get_current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".alien");
    let mut agents = HashMap::new();

    if let Some(snapshot) = snapshot {
        agents.insert(
            snapshot.deployment_name.clone(),
            AgentStatus {
                id: snapshot.deployment_id.clone(),
                name: snapshot.deployment_name.clone(),
                commands_url: Some(snapshot.commands_url.clone()),
                status: snapshot.status,
                resources: snapshot.resources.clone(),
                created_at: Utc::now().to_rfc3339(),
                error: None,
            },
        );
    }

    DevStatus {
        pid: std::process::id(),
        platform: "local".to_string(),
        stack_id: "dev".to_string(),
        state_dir: state_dir.display().to_string(),
        api_url: format!("http://localhost:{port}"),
        started_at: Utc::now().to_rfc3339(),
        status,
        agents,
        last_updated: Utc::now().to_rfc3339(),
        error,
    }
}

pub fn write_dev_status(path: &PathBuf, status: &DevStatus) -> Result<()> {
    write_json_file(path, status)
}

fn parse_deployment_status(status: &str) -> Result<DeploymentStatus> {
    match status {
        "pending" => Ok(DeploymentStatus::Pending),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
        "deleted" => Ok(DeploymentStatus::Deleted),
        "error" => Ok(DeploymentStatus::Error),
        other => Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment_status".to_string(),
            message: format!("Unknown local deployment status '{other}'"),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_deployment_status_rejects_unknown_values() {
        let err = parse_deployment_status("mystery").unwrap_err();
        assert!(err.to_string().contains("Unknown local deployment status"));
    }

    #[test]
    fn build_dev_status_includes_snapshot_agent() {
        let mut resources = HashMap::new();
        resources.insert(
            "web".to_string(),
            DevResourceInfo {
                url: "http://localhost:3000".to_string(),
                resource_type: Some("http-server".to_string()),
            },
        );
        let snapshot = DevDeploymentSnapshot {
            deployment_id: "dep_123".to_string(),
            deployment_name: "default".to_string(),
            status: DeploymentStatus::Running,
            commands_url: "http://localhost:9090/commands".to_string(),
            resources: resources.clone(),
        };

        let status = build_dev_status(9090, DevStatusState::Ready, Some(&snapshot), None);

        assert_eq!(status.api_url, "http://localhost:9090");
        assert_eq!(status.platform, "local");
        assert!(matches!(status.status, DevStatusState::Ready));
        assert_eq!(status.agents["default"].id, "dep_123");
        assert_eq!(
            status.agents["default"].commands_url.as_deref(),
            Some("http://localhost:9090/commands")
        );
        assert_eq!(
            status.agents["default"].resources["web"].url,
            "http://localhost:3000"
        );
        assert_eq!(
            status.agents["default"].resources["web"]
                .resource_type
                .as_deref(),
            Some("http-server")
        );
    }

    #[test]
    fn write_dev_status_writes_json_file() {
        let temp_dir = TempDir::new().unwrap();
        let status_path = temp_dir.path().join("nested").join("status.json");
        let status = build_dev_status(9090, DevStatusState::Initializing, None, None);

        write_dev_status(&status_path, &status).unwrap();

        let written = fs::read_to_string(&status_path).unwrap();
        assert!(written.contains("\"apiUrl\": \"http://localhost:9090\""));
        assert!(written.contains("\"status\": \"initializing\""));
    }
}

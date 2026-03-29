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
use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_commands::server::NullCommandDispatcher;
use alien_core::{
    AgentStatus, BinaryTarget, DeploymentStatus, DevResourceInfo, DevStatus, DevStatusState, Stack,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager::{
    providers::{
        in_memory_telemetry::InMemoryTelemetryBackend, local_credentials::LocalCredentialResolver,
        permissive_auth::PermissiveAuthValidator,
    },
    stores::sqlite::{
        SqliteCommandRegistry, SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore,
        SqliteTokenStore,
    },
    traits::{DeploymentStore, ReleaseStore, ServerBindings, TokenStore},
    LogBuffer,
};
use alien_manager_api::types::{CreateReleaseRequest, StackByPlatform};
use alien_manager_api::Client as AlienManagerClient;
use alien_manager_api::SdkResultExt;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
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

    start_embedded_dev_manager(port).await
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

        if name.starts_with("ag_") || name.starts_with("dep_") {
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
pub async fn build_embedded_dev_manager(
    port: u16,
) -> Result<(alien_manager::AlienManager, SocketAddr)> {
    let state_dir = get_current_dir()?.join(".alien");
    std::fs::create_dir_all(&state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to create .alien directory".to_string(),
        })?;

    let db_path = state_dir.join("dev-server.db");

    let config = alien_manager::ManagerConfig {
        port,
        db_path: Some(db_path),
        state_dir: Some(state_dir.clone()),
        enable_local_log_ingest: true,
        ..Default::default()
    };

    let db = Arc::new(
        SqliteDatabase::new(
            config
                .db_path
                .as_ref()
                .expect("dev manager db_path should always be set")
                .to_string_lossy()
                .as_ref(),
        )
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to initialize dev server database".to_string(),
        })?,
    );
    let deployment_store: Arc<dyn DeploymentStore> =
        Arc::new(SqliteDeploymentStore::new(db.clone()));
    let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));

    let kv_path = state_dir.join("commands_kv");
    let storage_path = state_dir.join("commands_storage");
    let command_kv: Arc<dyn alien_bindings::traits::Kv> = Arc::new(
        LocalKv::new(kv_path)
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to create dev command KV store".to_string(),
            })?,
    );
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(storage_path.to_string_lossy().into_owned()).context(
            ErrorData::ServerStartFailed {
                reason: "Failed to create dev command storage".to_string(),
            },
        )?,
    );
    let server_bindings = ServerBindings {
        command_kv,
        command_storage,
        command_dispatcher: Arc::new(NullCommandDispatcher),
        command_registry: Arc::new(SqliteCommandRegistry::new(db, deployment_store.clone())),
        artifact_registry: None,
        bindings_provider: None,
    };
    let log_buffer = Arc::new(LogBuffer::new());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let server = alien_manager::AlienManager::builder(config)
        .deployment_store(deployment_store)
        .release_store(release_store)
        .token_store(token_store)
        .credential_resolver(Arc::new(LocalCredentialResolver::new(state_dir)))
        .telemetry_backend(Arc::new(InMemoryTelemetryBackend::new(log_buffer.clone())))
        .auth_validator(Arc::new(PermissiveAuthValidator::new()))
        .server_bindings(server_bindings)
        .log_buffer(log_buffer)
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
            tracing::error!("Dev server error: {:?}", e);
        }
    });

    wait_for_dev_server_ready(port).await?;
    ensure_local_dev_deployment_group(port).await?;
    info!("Dev server ready");

    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevDeploymentGroup {
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListDeploymentGroupsResponse {
    items: Vec<DevDeploymentGroup>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateDeploymentGroupBody {
    name: String,
    max_deployments: i64,
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
    let base_url = format!("http://localhost:{port}");
    let http_client = reqwest::Client::new();

    let response = http_client
        .get(format!("{base_url}/v1/deployment-groups"))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list dev deployment groups".to_string(),
            url: None,
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "Failed to list dev deployment groups ({}): {}",
                status, body
            ),
            url: None,
        }));
    }

    let body: ListDeploymentGroupsResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialize".to_string(),
                reason: "Failed to parse dev deployment groups response".to_string(),
            })?;

    if body.items.iter().any(|group| group.name == "local-dev") {
        return Ok(());
    }

    let create_response = http_client
        .post(format!("{base_url}/v1/deployment-groups"))
        .json(&CreateDeploymentGroupBody {
            name: "local-dev".to_string(),
            max_deployments: 100,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create default dev deployment group".to_string(),
            url: None,
        })?;

    if !create_response.status().is_success() {
        let status = create_response.status();
        let body = create_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "Failed to create default dev deployment group ({}): {}",
                status, body
            ),
            url: None,
        }));
    }

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
/// Uses raw HTTP instead of the auto-generated SDK to support the `environmentVariables`
/// field which may not yet be in the generated SDK types.
pub async fn create_initial_deployment(
    deployment_name: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
) -> Result<String> {
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));
    let base_url = format!("http://localhost:{}", port);

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

    let exists = list_response
        .items
        .iter()
        .any(|d| d.name == deployment_name);
    if exists {
        info!("Deployment '{}' already exists", deployment_name);
        let existing = list_response
            .items
            .clone()
            .into_iter()
            .find(|deployment| deployment.name == deployment_name)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Deployment disappeared while resolving local deployment".to_string(),
                })
            })?;
        return Ok(existing.id);
    }

    // Create deployment with environment variables via raw HTTP
    info!("Creating initial deployment '{}'...", deployment_name);

    let mut body = serde_json::json!({
        "name": deployment_name,
        "platform": "local",
    });

    if let Some(env_vars) = &environment_variables {
        if !env_vars.is_empty() {
            body["environmentVariables"] = serde_json::to_value(env_vars)
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "serialize".to_string(),
                    reason: "Failed to serialize environment variables".to_string(),
                })?;
        }
    }

    let http_client = reqwest::Client::new();
    let response = http_client
        .post(format!("{}/v1/deployments", base_url))
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment".to_string(),
            url: None,
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Failed to create deployment ({}): {}", status, body_text),
            url: None,
        }));
    }

    let body: serde_json::Value =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialize".to_string(),
                reason: "Failed to parse deployment creation response".to_string(),
            })?;

    let deployment_id = body
        .get("deployment")
        .and_then(|deployment| deployment.get("id"))
        .and_then(|id| id.as_str())
        .or_else(|| body.get("id").and_then(|id| id.as_str()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::JsonError {
                operation: "extract".to_string(),
                reason: "Deployment creation response did not include an ID".to_string(),
            })
        })?;

    info!("Deployment '{}' created", deployment_name);
    Ok(deployment_id.to_string())
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
    let base_url = format!("http://localhost:{port}");
    let http_client = reqwest::Client::new();

    if let Some(existing) = find_named_local_deployment(port, deployment_name).await? {
        info!(
            "Refreshing existing local deployment '{}' ({})",
            deployment_name, existing.id
        );

        let response = http_client
            .delete(format!("{base_url}/v1/deployments/{}", existing.id))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!(
                    "Failed to delete existing local deployment '{}'",
                    deployment_name
                ),
                url: None,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Failed to delete existing local deployment '{}' ({}): {}",
                    deployment_name, status, body_text
                ),
                url: None,
            }));
        }

        wait_for_local_deployment_absent(port, deployment_name).await?;
    }

    create_initial_deployment(deployment_name, port, environment_variables).await
}

async fn find_named_local_deployment(
    port: u16,
    deployment_name: &str,
) -> Result<Option<DevDeploymentListItem>> {
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));
    let list_response = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list deployments".to_string(),
            url: None,
        })?;

    Ok(list_response
        .items
        .iter()
        .find(|deployment| deployment.name == deployment_name)
        .map(|deployment| DevDeploymentListItem {
            id: deployment.id.clone(),
            name: deployment.name.clone(),
            status: deployment.status.clone(),
        }))
}

async fn wait_for_local_deployment_absent(port: u16, deployment_name: &str) -> Result<()> {
    for _ in 0..30 {
        if find_named_local_deployment(port, deployment_name)
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevDeploymentListItem {
    id: String,
    name: String,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevDeploymentListResponse {
    items: Vec<DevDeploymentListItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevCommandsInfo {
    url: String,
    deployment_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevResourceEntry {
    resource_type: Option<String>,
    public_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevDeploymentInfoResponse {
    commands: DevCommandsInfo,
    resources: HashMap<String, DevResourceEntry>,
    status: String,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

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
    let http_client = reqwest::Client::new();
    let base_url = format!("http://localhost:{port}");

    for _ in 0..180 {
        let list_response = http_client
            .get(format!("{base_url}/v1/deployments"))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to list local deployments".to_string(),
                url: None,
            })?;

        if list_response.status().is_success() {
            let deployments: DevDeploymentListResponse = list_response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialize".to_string(),
                    reason: "Failed to parse local deployment list".to_string(),
                })?;

            if let Some(deployment) = deployments
                .items
                .into_iter()
                .find(|deployment| deployment.name == deployment_name)
            {
                let info_response = http_client
                    .get(format!("{base_url}/v1/deployments/{}/info", deployment.id))
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::ApiRequestFailed {
                        message: "Failed to fetch local deployment details".to_string(),
                        url: None,
                    })?;

                if info_response.status().is_success() {
                    let info: DevDeploymentInfoResponse =
                        info_response.json().await.into_alien_error().context(
                            ErrorData::JsonError {
                                operation: "deserialize".to_string(),
                                reason: "Failed to parse local deployment info".to_string(),
                            },
                        )?;

                    let snapshot = DevDeploymentSnapshot {
                        deployment_id: info.commands.deployment_id,
                        deployment_name: deployment.name,
                        status: parse_deployment_status(&info.status)?,
                        commands_url: info.commands.url,
                        resources: info
                            .resources
                            .into_iter()
                            .filter_map(|(name, resource)| {
                                resource.public_url.map(|url| {
                                    (
                                        name,
                                        DevResourceInfo {
                                            url,
                                            resource_type: resource.resource_type,
                                        },
                                    )
                                })
                            })
                            .collect(),
                    };

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
            deployment_id: "ag_123".to_string(),
            deployment_name: "default".to_string(),
            status: DeploymentStatus::Running,
            commands_url: "http://localhost:9090/commands".to_string(),
            resources: resources.clone(),
        };

        let status = build_dev_status(9090, DevStatusState::Ready, Some(&snapshot), None);

        assert_eq!(status.api_url, "http://localhost:9090");
        assert_eq!(status.platform, "local");
        assert!(matches!(status.status, DevStatusState::Ready));
        assert_eq!(status.agents["default"].id, "ag_123");
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

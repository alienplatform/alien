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
    AgentStatus, Container, ContainerCode, Daemon, DaemonCode, DeploymentStatus, DevResourceInfo,
    DevStatus, DevStatusState, Stack, StackState, Worker, WorkerCode,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager::{
    auth::Subject,
    providers::{
        in_memory_telemetry::InMemoryTelemetryBackend, local_credentials::LocalCredentialResolver,
        permissive_auth::PermissiveAuthValidator,
    },
    standalone_config::ManagerTomlConfig,
    stores::sqlite::{SqliteDatabase, SqliteDeploymentStore},
    traits::deployment_store::{DeploymentFilter, DeploymentStore},
    LogBuffer,
};
use alien_manager_api::types::{
    CreateReleaseRequest, DeploymentInfoResponse, DeploymentResponse, StackByPlatform,
};
use alien_manager_api::Client as AlienManagerClient;
use alien_manager_api::SdkResultExt;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::{sync::oneshot, task::JoinHandle, time::Duration};
use tracing::info;

/// Parsed CLI environment variable.
#[derive(Clone)]
pub struct CliEnvVar {
    pub name: String,
    pub value: String,
    pub is_secret: bool,
    pub target_resources: Option<Vec<String>>,
}

impl std::fmt::Debug for CliEnvVar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CliEnvVar")
            .field("name", &self.name)
            .field(
                "value",
                if self.is_secret {
                    &"[REDACTED]" as &dyn std::fmt::Debug
                } else {
                    &self.value
                },
            )
            .field("is_secret", &self.is_secret)
            .field("target_resources", &self.target_resources)
            .finish()
    }
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
/// Own the manager port while retaining the deployment database and local storage.
pub async fn ensure_server_running_for_dev_session(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
    deployment_name: &str,
) -> Result<EmbeddedDevManager> {
    ensure_server_running_internal(port, status_file, user_env_vars, Some(deployment_name))
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::ServerStartFailed {
                reason: "The full development session must own its manager".to_string(),
            })
        })
}

/// Ensure the dev server is running with user-provided env vars and optional status file (start if not)
pub async fn ensure_server_running_with_env(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    ensure_server_running_internal(port, status_file, user_env_vars, None)
        .await
        .map(|_| ())
}

async fn ensure_server_running_internal(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
    deployment_name: Option<&str>,
) -> Result<Option<EmbeddedDevManager>> {
    if check_server_health(port).await {
        if deployment_name.is_some() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "port".to_string(),
                message: format!(
                    "Another `alien dev` manager is already running on port {port}. Stop it first (Ctrl+C in that terminal, or kill the process), then rerun `alien dev`. For simultaneous sessions, use separate project directories and different ports."
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
        return Ok(None);
    }

    if let Some(status_file) = status_file {
        write_dev_status(
            &status_file,
            &build_dev_status(port, DevStatusState::Initializing, None, None),
        )?;
    }

    ensure_dev_port_available(port)?;

    let state_lock = prepare_dev_state(&get_current_dir()?.join(".alien")).await?;
    if let Some(name) = deployment_name {
        refresh_local_deployment_environment(
            &get_current_dir()?.join(".alien"),
            name,
            &user_env_vars,
        )
        .await?;
    }

    if deployment_name.is_some() {
        start_owned_embedded_dev_manager(port, state_lock)
            .await
            .map(Some)
    } else {
        start_embedded_dev_manager_with_lock(port, state_lock)
            .await
            .map(|_| None)
    }
}

async fn prepare_dev_state(state_dir: &Path) -> Result<File> {
    acquire_dev_state_lock(state_dir)
}

/// Hold an OS lock for the entire manager lifetime, before opening its database.
/// The lock file stays in place: unlinking it would let another process lock a new inode.
fn acquire_dev_state_lock(state_dir: &Path) -> Result<File> {
    std::fs::create_dir_all(state_dir)
        .into_alien_error()
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to create the local manager state directory".to_string(),
        })?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state_dir.join("dev-server.lock"))
        .into_alien_error()
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to open the local manager ownership lock".to_string(),
        })?;
    file.try_lock().map_err(|error| {
        AlienError::new(ErrorData::ServerStartFailed {
            reason: format!(
                "Cannot own local manager state '{}': {error}. Stop the manager using this directory before restarting; simultaneous sessions require separate project directories and different ports.",
                state_dir.display()
            ),
        })
    })?;
    Ok(file)
}

/// The full dev session owns this stopped manager's database. Refresh its CLI
/// environment before execution resumes, preserving deployment identity and data.
async fn refresh_local_deployment_environment(
    state_dir: &Path,
    name: &str,
    variables: &[CliEnvVar],
) -> Result<()> {
    let path = state_dir.join("dev-server.db");
    if !path.exists() {
        if name.starts_with("dep_") || name.contains('/') {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "deployment-name".to_string(),
                message: format!(
                    "No local deployment exists for '{name}'; use a bare name to create a new deployment"
                ),
            }));
        }
        return Ok(());
    }
    let db = SqliteDatabase::new(&path.to_string_lossy()).await.context(
        ErrorData::ServerStartFailed {
            reason: "Failed to open the stopped local manager".to_string(),
        },
    )?;
    let store = SqliteDeploymentStore::new(Arc::new(db));
    let subject = Subject::system();
    let groups =
        store
            .list_deployment_groups(&subject)
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to resolve the local development group".to_string(),
            })?;
    let canonical = groups.iter().find(|group| group.name == "local-dev");
    let explicit = name.starts_with("dep_") || name.contains('/');
    let (source_group, deployment_name) = match name.split_once('/') {
        Some((group, name)) if !group.is_empty() && !name.is_empty() && !name.contains('/') => {
            (Some(group), name)
        }
        Some(_) => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "deployment-name".to_string(),
                message: "Use a deployment ID or <group>/<name> for an explicit local migration"
                    .to_string(),
            }))
        }
        None => (None, name),
    };
    let deployments = store
        .list_deployments(
            &subject,
            &DeploymentFilter {
                name: if name.starts_with("dep_") {
                    None
                } else {
                    Some(deployment_name.to_string())
                },
                platforms: Some(vec![alien_core::Platform::Local]),
                ..Default::default()
            },
        )
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to resolve local session deployment".to_string(),
        })?;
    let matches: Vec<_> = deployments
        .iter()
        .filter(|deployment| {
            if name.starts_with("dep_") {
                deployment.id == name
            } else if let Some(source_group) = source_group {
                deployment.name == deployment_name
                    && groups.iter().any(|group| {
                        group.name == source_group && group.id == deployment.deployment_group_id
                    })
            } else {
                deployment.name == name
                    && canonical.is_some_and(|group| group.id == deployment.deployment_group_id)
            }
        })
        .collect();
    if matches.len() > 1 || (explicit && matches.is_empty()) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-name".to_string(),
            message: format!(
                "Expected one local deployment for '{name}'; found {}",
                matches.len()
            ),
        }));
    }
    if !explicit && matches.is_empty() && !deployments.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-name".to_string(),
            message: format!("A local deployment named '{name}' exists outside local-dev. Preserve its identity by selecting it explicitly: alien dev --deployment-name {}", deployments[0].id),
        }));
    }
    if let Some(deployment) = matches.first() {
        if explicit {
            let group_id =
                match canonical {
                    Some(group) => group.id.clone(),
                    None => store
                        .create_deployment_group(
                            &subject,
                            alien_manager::traits::deployment_store::CreateDeploymentGroupParams {
                                name: "local-dev".to_string(),
                                max_deployments: 100,
                                setup: Default::default(),
                            },
                        )
                        .await
                        .context(ErrorData::ServerStartFailed {
                            reason: "Failed to create the local development group".to_string(),
                        })?
                        .id,
                };
            store.reassign_local_deployment_group(&deployment.id, &group_id).await
                .context(ErrorData::ServerStartFailed {
                    reason: "Failed to migrate the selected local deployment; check for a conflicting name in local-dev".to_string(),
                })?;
        }
        let variables = crate::cli_env_vars_to_core(variables).unwrap_or_default();
        store
            .replace_environment_variables(&deployment.id, &variables)
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to refresh the local session environment".to_string(),
            })?;
    }
    Ok(())
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
/// Owns the embedded manager until its reconciliation and native runtimes finish.
pub struct EmbeddedDevManager {
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<()>>,
}

impl EmbeddedDevManager {
    pub async fn shutdown(self) -> Result<()> {
        let _ = self.shutdown.send(());
        self.task
            .await
            .into_alien_error()
            .context(ErrorData::ServerStartFailed {
                reason: "The local manager task failed during shutdown".to_string(),
            })?
    }
}

async fn start_owned_embedded_dev_manager(
    port: u16,
    state_lock: File,
) -> Result<EmbeddedDevManager> {
    let (server, addr) = build_embedded_dev_manager(port).await?;
    alien_local::start_docker_bridge_proxy(addr)
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to expose the dev server on Docker's private host gateway".to_string(),
        })?;
    let (shutdown, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _state_lock = state_lock;
        server
            .start_with_shutdown(addr, async {
                let _ = receiver.await;
            })
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "The local manager failed".to_string(),
            })
    });
    let manager = EmbeddedDevManager { shutdown, task };
    wait_for_dev_server_ready(port).await?;
    ensure_local_dev_deployment_group(port).await?;
    Ok(manager)
}

pub async fn start_embedded_dev_manager(port: u16) -> Result<()> {
    let state_lock = prepare_dev_state(&get_current_dir()?.join(".alien")).await?;
    start_embedded_dev_manager_with_lock(port, state_lock).await
}

async fn start_embedded_dev_manager_with_lock(port: u16, state_lock: File) -> Result<()> {
    info!("Starting dev server on port {}...", port);
    let (server, addr) = build_embedded_dev_manager(port).await?;

    alien_local::start_docker_bridge_proxy(addr)
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to expose the dev server on Docker's private host gateway".to_string(),
        })?;

    tokio::spawn(async move {
        let _state_lock = state_lock;
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

async fn local_dev_group_id(client: &AlienManagerClient) -> Result<String> {
    let groups = client
        .list_deployment_groups()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Resolving the local development group".to_string(),
            url: None,
        })?;
    groups
        .items
        .iter()
        .find(|group| group.name == "local-dev")
        .map(|group| group.id.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ServerStartFailed {
                reason: "The local development group is missing; restart the local manager"
                    .to_string(),
            })
        })
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
            // `None` resolves to the host target for Workers/Daemons (Local
            // platform default) while still letting source Containers build
            // their Linux image. An explicit host-only list means "host-binary
            // CI job" to the build and skips containers entirely — see
            // `requested_host_binary_only` in alien-build.
            targets: None,
            cache_url: None,
            override_base_image: None,
            debug_mode: true,
            rebuild: false,
            pull_base_images: false,
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

    validate_local_release_artifacts(&stack)?;

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
        machines: None,
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
            channel: None,
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

fn validate_local_release_artifacts(stack: &Stack) -> Result<()> {
    for (_, entry) in stack.resources() {
        if let Some(worker) = entry.config.downcast_ref::<Worker>() {
            if let WorkerCode::Image { image } = &worker.code {
                validate_local_image_artifact(
                    "Worker",
                    &worker.id,
                    image,
                    alien_core::BinaryTarget::current_os(),
                )?;
            }
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            if let DaemonCode::Image { image } = &daemon.code {
                validate_local_image_artifact(
                    "Daemon",
                    &daemon.id,
                    image,
                    alien_core::BinaryTarget::current_os(),
                )?;
            }
        } else if let Some(container) = entry.config.downcast_ref::<Container>() {
            if let ContainerCode::Image { image } = &container.code {
                validate_local_image_artifact(
                    "Container",
                    &container.id,
                    image,
                    alien_core::BinaryTarget::linux_container_target(),
                )?;
            }
        }
    }
    Ok(())
}

fn validate_local_image_artifact(
    resource_type: &str,
    resource_id: &str,
    image: &str,
    target: alien_core::BinaryTarget,
) -> Result<()> {
    let path = Path::new(image);
    let is_local_reference =
        path.is_absolute() || image.starts_with("./") || image.starts_with("../");
    if !is_local_reference {
        return Ok(());
    }

    if !path.exists() {
        return Err(local_artifact_error(
            resource_type,
            resource_id,
            format!("local image path '{}' does not exist", path.display()),
        ));
    }
    if path.is_file() {
        return Ok(());
    }

    let expected = path.join(format!("{}.oci.tar", target.runtime_platform_id()));
    if expected.is_file() {
        return Ok(());
    }

    Err(local_artifact_error(
        resource_type,
        resource_id,
        format!(
            "artifact directory '{}' has no image for target '{}' (expected '{}')",
            path.display(),
            target.runtime_platform_id(),
            expected.display()
        ),
    ))
}

fn local_artifact_error(
    resource_type: &str,
    resource_id: &str,
    message: String,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: format!(
            "{}.{}.code.image",
            resource_type.to_ascii_lowercase(),
            resource_id
        ),
        message: format!("{message}. Run `alien dev` without `--skip-build` to rebuild it"),
    })
}

/// Create initial deployment if it doesn't exist.
///
/// Always creates a local-platform deployment — dev mode is local-only.
/// Accepts optional environment variables to include in the deployment request.
pub async fn create_initial_deployment(
    deployment_name: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
    input_values: HashMap<String, serde_json::Value>,
) -> Result<String> {
    let client = local_dev_client(port);
    let group_id = local_dev_group_id(&client).await?;

    // Check if deployment exists
    let list_response = client
        .list_deployments()
        .deployment_group_id(&group_id)
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
        .find(|d| d.name == deployment_name && d.deployment_group_id == group_id)
    {
        // Inputs are fixed when a deployment is created, and the manager doesn't return them
        // (they may be secrets), so a rerun can't tell whether they changed: say so instead of
        // dropping them silently.
        if !input_values.is_empty() {
            eprintln!(
                "{} local deployment '{deployment_name}' already exists; its inputs are set at \
                 creation and were not changed. Destroy it with `alien dev destroy --name \
                 {deployment_name}` to create it with new inputs.",
                crate::ui::dim_label("Warning:")
            );
        }
        info!("Deployment '{}' already exists", deployment_name);
        return Ok(existing.id.clone());
    }

    // Old clients omitted the group ID. Without an explicit identity, a same-name
    // deployment in another group cannot safely be distinguished from an intentional one.
    let outside = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to check for an existing local deployment".to_string(),
            url: None,
        })?
        .into_inner();
    if outside.next_cursor.is_some()
        || outside.items.iter().any(|deployment| {
            deployment.name == deployment_name
                && deployment.platform == alien_manager_api::types::Platform::Local
        })
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-name".to_string(),
            message: "A same-name local deployment may exist outside local-dev. Stop the manager and select its ID with alien dev --deployment-name dep_... to preserve its state".to_string(),
        }));
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
                .deployment_group_id(group_id.clone())
                .platform(alien_manager_api::types::Platform::Local)
                .input_values(input_values.into_iter().collect::<serde_json::Map<_, _>>());
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
/// Reuse its durable deployment so restart does not delete storage or change bindings.
pub async fn prepare_dev_session_deployment(
    deployment_name: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
) -> Result<String> {
    create_initial_deployment(deployment_name, port, environment_variables, HashMap::new()).await
}

/// `alien dev destroy`: delete a local deployment by name and wait until it's gone. Dev
/// deployments live only in the dev manager, never in the tracker `alien destroy` reads.
pub async fn destroy_local_deployment(port: u16, deployment_name: &str, force: bool) -> Result<()> {
    let client = local_dev_client(port);
    let existing = find_named_local_deployment(&client, deployment_name)
        .await?
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: format!(
                    "No local deployment is named '{deployment_name}'. List them with \
                     `alien dev deployments ls`."
                ),
            })
        })?;

    if !force {
        return super::destroy::destroy_local_target(port, existing).await;
    }

    let action = alien_manager_api::types::DeleteDeploymentAction::Forget;
    client
        .delete_deployment()
        .id(&existing.id)
        .body(alien_manager_api::types::DeleteDeploymentRequest { action })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Failed to delete local deployment '{deployment_name}'"),
            url: None,
        })?;

    wait_for_local_deployment_absent(port, deployment_name).await
}

async fn find_named_local_deployment(
    client: &AlienManagerClient,
    deployment_name: &str,
) -> Result<Option<alien_manager_api::types::DeploymentResponse>> {
    let group_id = local_dev_group_id(client).await?;
    let response = client
        .list_deployments()
        .deployment_group_id(&group_id)
        .name(deployment_name)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list local development deployments".to_string(),
            url: None,
        })?
        .into_inner();
    if response.next_cursor.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: "The manager returned an incomplete local deployment list".to_string(),
        }));
    }
    let mut matches = response.items.into_iter().filter(|deployment| {
        deployment.name == deployment_name
            && deployment.deployment_group_id == group_id
            && deployment.platform == alien_manager_api::types::Platform::Local
    });
    let first = matches.next();
    if matches.next().is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!(
                "Multiple local development deployments are named '{deployment_name}'"
            ),
        }));
    }
    Ok(first)
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

    let timeout_seconds = std::env::var("ALIEN_DEV_DEPLOYMENT_TIMEOUT_SECONDS")
        .map(|value| value.parse::<u64>())
        .unwrap_or(Ok(300))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "ALIEN_DEV_DEPLOYMENT_TIMEOUT_SECONDS must be a positive integer".to_string(),
        })?;
    if timeout_seconds == 0 {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "ALIEN_DEV_DEPLOYMENT_TIMEOUT_SECONDS must be greater than zero".to_string(),
        }));
    }
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(timeout_seconds))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "ALIEN_DEV_DEPLOYMENT_TIMEOUT_SECONDS is too large".to_string(),
            })
        })?;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(list_response)) =
            tokio::time::timeout_at(deadline, client.list_deployments().send()).await
        {
            if let Some(deployment) = list_response
                .items
                .iter()
                .find(|d| d.name == deployment_name)
            {
                if let Ok(Ok(info_response)) = tokio::time::timeout_at(
                    deadline,
                    client.get_deployment_info().id(&deployment.id).send(),
                )
                .await
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
        message: format!("Stopped waiting after {}s; the local deployment may still be progressing. Inspect it with `alien dev deployments ls` or increase ALIEN_DEV_DEPLOYMENT_TIMEOUT_SECONDS.", timeout_seconds),
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
    let group_id = local_dev_group_id(&client).await?;

    let list_response = match client
        .list_deployments()
        .deployment_group_id(group_id)
        .send()
        .await
    {
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
        "preflights-failed" => Ok(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "waiting-for-machines" => Ok(DeploymentStatus::WaitingForMachines),
        "waiting-for-secrets" => Ok(DeploymentStatus::WaitingForSecrets),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
        "teardown-required" => Ok(DeploymentStatus::TeardownRequired),
        "teardown-failed" => Ok(DeploymentStatus::TeardownFailed),
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
    use alien_core::ResourceLifecycle;
    use alien_manager::traits::deployment_store::{
        CreateDeploymentGroupParams, CreateDeploymentParams,
    };
    use axum::{
        extract::{Path as AxumPath, Query, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use std::{
        fs,
        sync::{Arc, Mutex},
    };
    use tempfile::TempDir;

    fn worker_with_image(image: String) -> Worker {
        Worker::new("worker".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image { image })
            .build()
    }

    /// The dev manager as `alien dev destroy` sees it: one failed deployment named `api`, which
    /// stays listed as `deleting` for one poll after the delete is accepted (cleanup runs
    /// asynchronously), then disappears.
    #[derive(Default)]
    struct DestroyManagerState {
        deleted: Vec<(String, serde_json::Value)>,
        lists_after_delete: usize,
    }

    type SharedDestroyManager = Arc<Mutex<DestroyManagerState>>;

    async fn destroy_manager_list(
        State(manager): State<SharedDestroyManager>,
    ) -> Json<serde_json::Value> {
        let mut manager = manager.lock().unwrap();
        // Deletable until the delete is accepted, then `deleting` for one poll, then gone.
        let status = if manager.deleted.is_empty() {
            Some("provisioning-failed")
        } else {
            manager.lists_after_delete += 1;
            (manager.lists_after_delete == 1).then_some("deleting")
        };
        let items = if let Some(status) = status {
            serde_json::json!([{
                "id": "dep_1",
                "name": "api",
                "platform": "local",
                "status": status,
                "deploymentGroupId": "dg_1",
                "deploymentProtocolVersion": 1,
                "projectId": "default",
                "workspaceId": "default",
                "retryRequested": false,
                "createdAt": "2026-01-01T00:00:00Z",
            }])
        } else {
            serde_json::json!([])
        };
        let mut items = items.as_array().unwrap().clone();
        items.insert(
            0,
            serde_json::json!({
                "id":"dep_other", "name":"api", "platform":"local", "status":"running",
                "deploymentGroupId":"dg_other", "deploymentProtocolVersion":1,
                "projectId":"default", "workspaceId":"default", "retryRequested":false,
                "createdAt":"2026-01-01T00:00:00Z"
            }),
        );
        Json(serde_json::json!({ "items": items }))
    }

    async fn destroy_manager_groups() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "items": [{
            "id":"dg_1", "name":"local-dev", "deploymentCount":1, "maxDeployments":100,
            "projectId":"default", "workspaceId":"default", "createdAt":"2026-01-01T00:00:00Z"
        }] }))
    }

    async fn destroy_manager_delete(
        State(manager): State<SharedDestroyManager>,
        AxumPath(id): AxumPath<String>,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        manager.lock().unwrap().deleted.push((id, body));
        // The manager answers a delete with 202 Accepted and tears down in the background.
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "action": "cleanup",
                "message": "Deployment deletion accepted"
            })),
        )
    }

    /// Forced local deletion forgets the named record and waits until it is absent.
    /// An unknown deployment name is an error.
    #[tokio::test]
    async fn force_destroy_local_deployment_deletes_by_name_and_waits() {
        let manager: SharedDestroyManager = Arc::default();
        let app = Router::new()
            .route("/v1/deployment-groups", get(destroy_manager_groups))
            .route("/v1/deployments", get(destroy_manager_list))
            .route("/v1/deployments/{id}/delete", post(destroy_manager_delete))
            .with_state(manager.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        destroy_local_deployment(port, "api", true)
            .await
            .expect("the named deployment is deleted");
        {
            let manager = manager.lock().unwrap();
            assert_eq!(
                manager.deleted,
                vec![(
                    "dep_1".to_string(),
                    serde_json::json!({ "action": "forget" })
                )]
            );
            // One poll still saw the deployment while cleanup ran; the command kept waiting
            // until a second poll saw it gone.
            assert_eq!(manager.lists_after_delete, 2);
        }

        assert!(
            destroy_local_deployment(port, "api", true).await.is_err(),
            "the remaining same-name deployment outside local-dev must never be deleted"
        );
        assert_eq!(manager.lock().unwrap().deleted.len(), 1);

        let error = destroy_local_deployment(port, "missing", false)
            .await
            .expect_err("an unknown name is refused");
        assert!(
            error
                .message
                .contains("No local deployment is named 'missing'"),
            "{error:?}"
        );
    }

    /// `alien dev deploy --input` reaches the dev manager with the create request; a rerun
    /// reuses the existing deployment, whose inputs were fixed at creation.
    #[tokio::test]
    async fn create_initial_deployment_sends_inputs() {
        fn deployment(name: &str) -> serde_json::Value {
            serde_json::json!({
                "id": "dep_1",
                "name": name,
                "platform": "local",
                "status": "pending",
                "deploymentGroupId": "dg_1",
                "deploymentProtocolVersion": 1,
                "projectId": "default",
                "workspaceId": "default",
                "retryRequested": false,
                "createdAt": "2026-01-01T00:00:00Z",
            })
        }
        type Created = Arc<Mutex<Vec<serde_json::Value>>>;
        async fn list(
            State(created): State<Created>,
            Query(query): Query<HashMap<String, String>>,
        ) -> Response {
            if query.contains_key("name") && !query.contains_key("deploymentGroupId") {
                return StatusCode::BAD_REQUEST.into_response();
            }
            let mut items: Vec<_> = created
                .lock()
                .unwrap()
                .iter()
                .map(|body| deployment(body["name"].as_str().unwrap()))
                .collect();
            let mut unrelated = deployment("api");
            unrelated["id"] = serde_json::json!("dep_other");
            unrelated["deploymentGroupId"] = serde_json::json!("dg_other");
            if !items.is_empty() {
                items.insert(0, unrelated);
            }
            Json(serde_json::json!({ "items": items })).into_response()
        }
        async fn create(
            State(created): State<Created>,
            Json(body): Json<serde_json::Value>,
        ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
            let name = body["name"].as_str().unwrap().to_string();
            created.lock().unwrap().push(body);
            // The manager answers a create with 201 Created.
            (
                axum::http::StatusCode::CREATED,
                Json(serde_json::json!({
                    "deployment": deployment(&name),
                    "deploymentModel": "push",
                })),
            )
        }

        async fn groups() -> Json<serde_json::Value> {
            Json(serde_json::json!({ "items": [
                { "id":"dg_other", "name":"other", "deploymentCount":1, "maxDeployments":100,
                  "projectId":"default", "workspaceId":"default", "createdAt":"2026-01-01T00:00:00Z" },
                { "id":"dg_1", "name":"local-dev", "deploymentCount":0, "maxDeployments":100,
                  "projectId":"default", "workspaceId":"default", "createdAt":"2026-01-01T00:00:00Z" }
            ] }))
        }
        let created: Created = Arc::default();
        let app = Router::new()
            .route("/v1/deployment-groups", get(groups))
            .route("/v1/deployments", get(list).post(create))
            .with_state(created.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let inputs = HashMap::from([("managedKey".to_string(), serde_json::json!(false))]);

        create_initial_deployment("api", port, None, inputs.clone())
            .await
            .expect("the deployment is created");
        assert_eq!(created.lock().unwrap()[0]["deploymentGroupId"], "dg_1");
        assert_eq!(
            created.lock().unwrap()[0]["inputValues"],
            serde_json::json!({ "managedKey": false })
        );

        // A rerun reuses the deployment; inputs are only sent when creating it.
        let rerun = create_initial_deployment("api", port, None, inputs)
            .await
            .expect("a rerun reuses the existing deployment");
        assert_eq!(rerun, "dep_1");
        let session = prepare_dev_session_deployment("api", port, None)
            .await
            .expect("the full dev session also reuses durable state");
        assert_eq!(session, "dep_1");
        assert_eq!(created.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_local_migration_target_fails_without_creating_state() {
        let directory = TempDir::new().unwrap();
        for reference in ["dep_missing", "legacy/api", "legacy/"] {
            let error = refresh_local_deployment_environment(directory.path(), reference, &[])
                .await
                .expect_err("an explicit reference must select an existing deployment");
            assert_eq!(error.code, "VALIDATION_ERROR");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
        refresh_local_deployment_environment(directory.path(), "api", &[])
            .await
            .expect("a bare name may start a new deployment");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn legacy_local_migration_requires_identity_and_preserves_state() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("dev-server.db");
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let subject = Subject::system();
        let legacy = store
            .create_deployment_group(
                &subject,
                CreateDeploymentGroupParams {
                    name: "legacy".to_string(),
                    max_deployments: 100,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let parameters = CreateDeploymentParams {
            name: "api".to_string(),
            deployment_group_id: legacy.id,
            platform: alien_core::Platform::Local,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            stack_settings: Default::default(),
            stack_state: Some(StackState::with_resource_prefix(
                alien_core::Platform::Local,
                "retained".to_string(),
            )),
            environment_variables: None,
            public_subdomain: None,
            input_values: Default::default(),
            setup_item: None,
            deployment_token: None,
        };
        let before = store
            .create_deployment(&subject, parameters.clone())
            .await
            .unwrap();
        drop(store);
        assert!(
            refresh_local_deployment_environment(directory.path(), "api", &[])
                .await
                .is_err(),
            "a same-name deployment in another group must not be adopted implicitly"
        );
        let variables = vec![CliEnvVar {
            name: "APP_KEY".to_string(),
            value: "updated".to_string(),
            is_secret: false,
            target_resources: None,
        }];
        refresh_local_deployment_environment(directory.path(), &before.id, &variables)
            .await
            .unwrap();
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let after = store
            .get_deployment(&subject, &before.id)
            .await
            .unwrap()
            .unwrap();
        let groups = store.list_deployment_groups(&subject).await.unwrap();
        let canonical = groups
            .iter()
            .find(|group| group.name == "local-dev")
            .unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.deployment_group_id, canonical.id);
        assert_eq!(
            serde_json::to_value(&after.stack_state).unwrap(),
            serde_json::to_value(&before.stack_state).unwrap()
        );
        assert_eq!(after.status, before.status);
        assert_eq!(after.created_at, before.created_at);
        assert_eq!(
            after.user_environment_variables.unwrap()[0].value,
            "updated"
        );
        assert_eq!(
            store
                .list_deployments(&subject, &DeploymentFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
        let conflicting = store.create_deployment(&subject, parameters).await.unwrap();
        drop(store);
        assert!(
            refresh_local_deployment_environment(directory.path(), &conflicting.id, &variables)
                .await
                .is_err(),
            "migration must not overwrite a same-name deployment in local-dev"
        );
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let untouched = store
            .get_deployment(&subject, &conflicting.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            untouched.deployment_group_id,
            conflicting.deployment_group_id
        );
        assert_eq!(
            untouched.user_environment_variables,
            conflicting.user_environment_variables
        );
        assert_eq!(
            serde_json::to_value(&untouched.stack_state).unwrap(),
            serde_json::to_value(&conflicting.stack_state).unwrap()
        );
        drop(store);
        refresh_local_deployment_environment(directory.path(), "local-dev/api", &[])
            .await
            .unwrap();
        refresh_local_deployment_environment(directory.path(), "api", &[])
            .await
            .unwrap();
    }

    #[test]
    fn dev_state_ownership_is_exclusive_and_released_on_drop() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let owner = acquire_dev_state_lock(first.path()).expect("first manager owns state");
        assert!(
            acquire_dev_state_lock(first.path()).is_err(),
            "another port must not share state"
        );
        let independent =
            acquire_dev_state_lock(second.path()).expect("separate state is independent");
        drop(owner);
        let restarted =
            acquire_dev_state_lock(first.path()).expect("stopped manager releases state");
        assert!(
            acquire_dev_state_lock(first.path()).is_err(),
            "restarted manager owns the same lock inode"
        );
        drop((restarted, independent));
    }

    #[tokio::test]
    async fn local_manager_restart_recovers_only_abandoned_local_claims() {
        let directory = TempDir::new().unwrap();
        let owner = prepare_dev_state(directory.path()).await.unwrap();
        let database = Arc::new(
            SqliteDatabase::new(&directory.path().join("dev-server.db").to_string_lossy())
                .await
                .unwrap(),
        );
        let store = SqliteDeploymentStore::new(database.clone());
        let subject = Subject::system();
        let group = store
            .create_deployment_group(
                &subject,
                CreateDeploymentGroupParams {
                    name: "local-dev".to_string(),
                    max_deployments: 100,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let mut records = Vec::new();
        for (name, platform) in [
            ("local", alien_core::Platform::Local),
            ("cloud", alien_core::Platform::Aws),
        ] {
            records.push(
                store
                    .create_deployment(
                        &subject,
                        CreateDeploymentParams {
                            name: name.to_string(),
                            deployment_group_id: group.id.clone(),
                            platform,
                            deployment_protocol_version:
                                alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                            base_platform: None,
                            stack_settings: Default::default(),
                            stack_state: Some(StackState::with_resource_prefix(
                                platform,
                                "retained".to_string(),
                            )),
                            environment_variables: None,
                            public_subdomain: None,
                            input_values: Default::default(),
                            setup_item: None,
                            deployment_token: None,
                        },
                    )
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(
            store
                .acquire(&subject, "old-process", &DeploymentFilter::default(), 10)
                .await
                .unwrap()
                .len(),
            2
        );
        assert!(
            prepare_dev_state(directory.path()).await.is_err(),
            "a live owner must prevent recovery"
        );
        let locked = store
            .get_deployment(&subject, &records[0].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(locked.locked_by.as_deref(), Some("old-process"));
        drop((store, database, owner));

        let restarted = prepare_dev_state(directory.path()).await.unwrap();
        let database = Arc::new(
            SqliteDatabase::new(&directory.path().join("dev-server.db").to_string_lossy())
                .await
                .unwrap(),
        );
        let store = SqliteDeploymentStore::new(database);
        let recovered = store
            .acquire(&subject, "new-process", &DeploymentFilter::default(), 10)
            .await
            .unwrap();
        assert_eq!(
            recovered.len(),
            1,
            "a replacement manager must acquire local work immediately"
        );
        assert_eq!(recovered[0].deployment.id, records[0].id);
        assert_eq!(
            recovered[0]
                .deployment
                .stack_state
                .as_ref()
                .unwrap()
                .resource_prefix,
            "retained"
        );
        let cloud = store
            .get_deployment(&subject, &records[1].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cloud.locked_by.as_deref(), Some("old-process"));
        assert!(prepare_dev_state(directory.path()).await.is_err());
        drop((store, restarted));
    }

    #[tokio::test]
    async fn session_environment_refresh_preserves_deployment_and_clears_old_values() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("dev-server.db");
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let subject = Subject::system();
        let group = store
            .create_deployment_group(
                &subject,
                CreateDeploymentGroupParams {
                    name: "local-dev".to_string(),
                    max_deployments: 100,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let parameters = CreateDeploymentParams {
            name: "api".to_string(),
            deployment_group_id: group.id,
            platform: alien_core::Platform::Local,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            stack_settings: Default::default(),
            stack_state: Some(StackState::with_resource_prefix(
                alien_core::Platform::Local,
                "retained".to_string(),
            )),
            environment_variables: None,
            public_subdomain: None,
            input_values: Default::default(),
            setup_item: None,
            deployment_token: None,
        };
        let before = store
            .create_deployment(&subject, parameters.clone())
            .await
            .unwrap();
        let other_group = store
            .create_deployment_group(
                &subject,
                CreateDeploymentGroupParams {
                    name: "another-group".to_string(),
                    max_deployments: 100,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let other = store
            .create_deployment(
                &subject,
                CreateDeploymentParams {
                    deployment_group_id: other_group.id,
                    ..parameters
                },
            )
            .await
            .unwrap();
        drop(store);
        let variables = vec![CliEnvVar {
            name: "APP_KEY".to_string(),
            value: "new-value".to_string(),
            is_secret: true,
            target_resources: Some(vec!["api".to_string()]),
        }];
        refresh_local_deployment_environment(directory.path(), "api", &variables)
            .await
            .unwrap();
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let after = store
            .get_deployment(&subject, &before.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.status, before.status);
        assert_eq!(after.stack_state.unwrap().resource_prefix, "retained");
        let actual = after.user_environment_variables.unwrap();
        assert_eq!(actual[0].value, "new-value");
        assert_eq!(
            actual[0].var_type,
            alien_core::EnvironmentVariableType::Secret
        );
        assert_eq!(actual[0].target_resources, Some(vec!["api".to_string()]));
        drop(store);
        refresh_local_deployment_environment(directory.path(), "api", &[])
            .await
            .unwrap();
        let store = SqliteDeploymentStore::new(Arc::new(
            SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
        ));
        let cleared = store
            .get_deployment(&subject, &before.id)
            .await
            .unwrap()
            .unwrap();
        let untouched = store
            .get_deployment(&subject, &other.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            untouched.user_environment_variables,
            other.user_environment_variables
        );
        assert_eq!(untouched.updated_at, other.updated_at);
        assert!(cleared.user_environment_variables.unwrap().is_empty());
        assert_eq!(cleared.stack_state.unwrap().resource_prefix, "retained");
    }

    #[test]
    fn parse_deployment_status_rejects_unknown_values() {
        let err = parse_deployment_status("mystery").unwrap_err();
        assert!(err.to_string().contains("Unknown local deployment status"));
    }

    #[test]
    fn parse_deployment_status_accepts_waiting_for_machines() {
        assert_eq!(
            parse_deployment_status("waiting-for-machines").unwrap(),
            DeploymentStatus::WaitingForMachines
        );
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

    #[test]
    fn local_release_accepts_host_worker_artifact() {
        let temp_dir = TempDir::new().unwrap();
        let artifact_dir = temp_dir.path().join("worker-12345678");
        fs::create_dir(&artifact_dir).unwrap();
        let target = alien_core::BinaryTarget::current_os();
        fs::write(
            artifact_dir.join(format!("{}.oci.tar", target.runtime_platform_id())),
            b"oci",
        )
        .unwrap();
        let stack = Stack::new("local-artifact".to_string())
            .add(
                worker_with_image(artifact_dir.to_string_lossy().into_owned()),
                ResourceLifecycle::Live,
            )
            .build();

        validate_local_release_artifacts(&stack).unwrap();
    }

    #[test]
    fn local_release_rejects_missing_artifact_before_post() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("missing-worker");
        let stack = Stack::new("local-artifact".to_string())
            .add(
                worker_with_image(missing.to_string_lossy().into_owned()),
                ResourceLifecycle::Live,
            )
            .build();

        let error = validate_local_release_artifacts(&stack).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("does not exist"));
        assert!(message.contains("without `--skip-build`"));
        assert!(message.contains("worker.worker.code.image"));
    }

    #[test]
    fn local_release_rejects_incompatible_worker_artifact() {
        let temp_dir = TempDir::new().unwrap();
        let artifact_dir = temp_dir.path().join("worker-12345678");
        fs::create_dir(&artifact_dir).unwrap();
        let incompatible = match alien_core::BinaryTarget::current_os() {
            alien_core::BinaryTarget::LinuxX64 => alien_core::BinaryTarget::LinuxArm64,
            _ => alien_core::BinaryTarget::LinuxX64,
        };
        fs::write(
            artifact_dir.join(format!("{}.oci.tar", incompatible.runtime_platform_id())),
            b"oci",
        )
        .unwrap();

        let error = validate_local_image_artifact(
            "Worker",
            "worker",
            artifact_dir.to_string_lossy().as_ref(),
            alien_core::BinaryTarget::current_os(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("has no image for target"));
    }

    #[test]
    fn local_release_preserves_registry_images() {
        for image in ["postgres:16-alpine", "team/app:release.tar"] {
            validate_local_image_artifact(
                "Container",
                "database",
                image,
                alien_core::BinaryTarget::linux_container_target(),
            )
            .unwrap();
        }
    }
}

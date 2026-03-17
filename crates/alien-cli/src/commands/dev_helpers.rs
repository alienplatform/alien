//! Helper functions for dev mode operation
//!
//! These functions handle building, posting releases, and creating deployments
//! for the dev server. They're used by the main CLI router for dev mode operations.

use crate::{
    config::load_configuration,
    error::{ErrorData, Result},
    get_current_dir,
};
use alien_build::settings::{BuildSettings, PlatformBuildSettings};
use alien_core::{BinaryTarget, Platform, Stack};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_server_sdk::types::{
    CreateDeploymentRequest, CreateReleaseRequest, Platform as SdkPlatform, StackByPlatform,
};
use alien_server_sdk::Client as AlienManagerClient;
use std::path::PathBuf;
use std::str::FromStr;
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

/// Check if dev server is healthy
pub async fn check_server_health(port: u16) -> bool {
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));
    client.health().send().await.is_ok()
}

/// Ensure the dev server is running (start if not)
pub async fn ensure_server_running(port: u16) -> Result<()> {
    ensure_server_running_with_env(port, None, Vec::new()).await
}

/// Ensure the dev server is running with user-provided env vars and optional status file (start if not)
pub async fn ensure_server_running_with_env(
    port: u16,
    _status_file: Option<PathBuf>,
    _user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    if check_server_health(port).await {
        info!("Dev server already running on port {}", port);
        return Ok(());
    }

    info!("Starting dev server on port {}...", port);
    let state_dir = get_current_dir()?.join(".alien");
    std::fs::create_dir_all(&state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to create .alien directory".to_string(),
        })?;

    let db_path = state_dir.join("dev-server.db");

    // Spawn alien-manager in background using the builder
    let config = alien_manager::ManagerConfig {
        port,
        db_path: Some(db_path),
        state_dir: Some(state_dir.clone()),
        dev_mode: true,
        ..Default::default()
    };

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    tokio::spawn(async move {
        match alien_manager::AlienManager::builder(config).build().await {
            Ok(server) => {
                if let Err(e) = server.start(addr).await {
                    tracing::error!("Dev server error: {:?}", e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to start dev server: {:?}", e);
            }
        }
    });

    // Wait for server to be ready
    for _ in 0..50 {
        if check_server_health(port).await {
            info!("Dev server ready");
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(AlienError::new(ErrorData::ServerStartFailed {
        reason: "Timeout waiting for dev server to start".to_string(),
    }))
}

/// Build and post a release to the dev server (simple version for use in TUI setup).
///
/// Platform-aware: builds with the correct `PlatformBuildSettings` variant, reads the
/// stack from `.alien/build/{platform}/stack.json`, and for cloud platforms pushes images
/// to the dev registry before posting the release.
pub async fn build_and_post_release_simple(
    current_dir: &PathBuf,
    port: u16,
    skip_build: bool,
    config_file: Option<&PathBuf>,
    platform: &str,
) -> Result<String> {
    let output_dir = current_dir.join(".alien");
    let platform_typed = Platform::from_str(platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    // Build if needed
    if !skip_build {
        info!("Building stack for {} platform...", platform);
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

        let platform_build_settings = match platform_typed {
            Platform::Aws => PlatformBuildSettings::Aws {
                managing_account_id: None,
            },
            Platform::Gcp => PlatformBuildSettings::Gcp {},
            Platform::Azure => PlatformBuildSettings::Azure {},
            Platform::Kubernetes => PlatformBuildSettings::Kubernetes {},
            Platform::Local => PlatformBuildSettings::Local {},
            Platform::Test => PlatformBuildSettings::Test {},
        };

        // For local platform, build for current OS only (native binary).
        // For cloud platforms, use default targets (Linux containers).
        let targets = match platform_typed {
            Platform::Local => Some(vec![BinaryTarget::current_os()]),
            _ => None, // Use platform-specific defaults
        };

        let settings = BuildSettings {
            output_directory: output_dir.to_str().unwrap().to_string(),
            platform: platform_build_settings,
            targets,
            cache_url: None,
            override_base_image: None,
            debug_mode: true,
        };

        alien_build::build_stack(stack, &settings)
            .await
            .context(ErrorData::BuildFailed)?;
    }

    // Load the built stack
    let stack_file = output_dir.join("build").join(platform).join("stack.json");
    let mut stack: Stack = serde_json::from_str(
        &std::fs::read_to_string(&stack_file)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: stack_file.display().to_string(),
                reason: format!("Failed to read stack.json for {} platform", platform),
            })?,
    )
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "deserialize".to_string(),
        reason: "Failed to parse stack.json".to_string(),
    })?;

    // For cloud platforms, push images to the dev registry before creating the release.
    // Local platform doesn't need pushing — it uses local OCI tarballs directly.
    if platform_typed != Platform::Local && platform_typed != Platform::Test {
        info!("Pushing images to dev registry...");
        let push_settings = super::dev_registry::resolve_dev_push_settings(
            &platform_typed,
            &output_dir,
            current_dir,
        )
        .await?;

        stack = alien_build::push_stack(stack, platform_typed.clone(), &push_settings)
            .await
            .context(ErrorData::BuildFailed)?;
        info!("Images pushed to dev registry");
    }

    // Post release to dev server
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));

    let stack_json =
        serde_json::to_value(&stack)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "Failed to serialize stack".to_string(),
            })?;

    // Place the stack in the correct platform slot
    let mut stack_by_platform = StackByPlatform {
        aws: None,
        gcp: None,
        azure: None,
        kubernetes: None,
        local: None,
        test: None,
    };
    match platform_typed {
        Platform::Aws => stack_by_platform.aws = Some(stack_json),
        Platform::Gcp => stack_by_platform.gcp = Some(stack_json),
        Platform::Azure => stack_by_platform.azure = Some(stack_json),
        Platform::Kubernetes => stack_by_platform.kubernetes = Some(stack_json),
        Platform::Local => stack_by_platform.local = Some(stack_json),
        Platform::Test => stack_by_platform.test = Some(stack_json),
    }

    let response = client
        .create_release()
        .body(CreateReleaseRequest {
            stack: stack_by_platform,
            git_metadata: None,
        })
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("Failed to create release on dev server: {}", e),
                url: None,
            })
        })?;

    Ok(response.id.clone())
}

/// Create initial deployment if it doesn't exist.
///
/// Accepts optional environment variables to include in the deployment request.
/// Uses raw HTTP instead of the auto-generated SDK to support the `environmentVariables`
/// field which may not yet be in the generated SDK types.
pub async fn create_initial_deployment(
    deployment_name: &str,
    platform: &str,
    port: u16,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
) -> Result<()> {
    let client = AlienManagerClient::new(&format!("http://localhost:{}", port));
    let base_url = format!("http://localhost:{}", port);

    // Check if deployment exists
    let list_response = client.list_deployments().send().await.map_err(|e| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Failed to list deployments: {}", e),
            url: None,
        })
    })?;

    let exists = list_response
        .items
        .iter()
        .any(|d| d.name == deployment_name);
    if exists {
        info!("Deployment '{}' already exists", deployment_name);
        return Ok(());
    }

    // Create deployment with environment variables via raw HTTP
    info!("Creating initial deployment '{}'...", deployment_name);

    let mut body = serde_json::json!({
        "name": deployment_name,
        "platform": platform,
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

    info!("Deployment '{}' created", deployment_name);
    Ok(())
}

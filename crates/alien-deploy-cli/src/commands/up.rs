//! Deploy command — sets up and runs a deployment.
//!
//! Push model (AWS, GCP, Azure): runs initial setup locally, then the manager
//! continues reconciliation remotely.
//!
//! Pull model (Local, Kubernetes): installs and starts the alien-agent service.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, Platform, ReleaseInfo,
    StackSettings,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::Client as ServerClient;
use clap::Parser;
use std::str::FromStr;
use tokio::time::{sleep, Duration};
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deploy the application to a target environment",
    after_help = "EXAMPLES:
    # Deploy to AWS using a deployment group token
    alien-deploy up --token dg_abc123... --platform aws --manager-url https://manager.example.com

    # Deploy locally (installs alien-agent as a background service)
    alien-deploy up --token dg_abc123... --platform local --manager-url https://manager.example.com

    # Redeploy an existing tracked deployment
    alien-deploy up --name production"
)]
pub struct UpArgs {
    /// Authentication token (deployment or deployment group token)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Manager URL
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Target platform (aws, gcp, azure, kubernetes, local)
    #[arg(long)]
    pub platform: Option<String>,

    /// Deployment name (for tracking)
    #[arg(long)]
    pub name: Option<String>,

    /// Encryption key for agent database (required for pull model)
    #[arg(long, env = "AGENT_ENCRYPTION_KEY")]
    pub encryption_key: Option<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Run the agent in the foreground instead of installing as a service.
    /// Useful for testing — Ctrl+C to stop.
    #[arg(long)]
    pub foreground: bool,
}

pub async fn up_command(args: UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    // Resolve token and manager URL from args, embedded config, or tracked deployment
    let (token, manager_url, platform_str, name) = resolve_deployment_info(&args, embedded_config)?;

    let platform = Platform::from_str(&platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    output::header("Alien Deploy");
    output::status("Manager:", &manager_url);
    output::status("Platform:", &platform_str);
    output::status("Name:", &name);

    // Create authenticated manager client
    let client = create_manager_client(&token, &manager_url)?;

    // Initialize with manager to get deployment_id and deployment token
    output::step(1, 3, "Initializing with manager...");

    let init = initialize_deployment(&client, &token, platform, &name).await?;
    let deployment_id = init.deployment_id;
    output::success(&format!("Deployment ID: {}", deployment_id));

    // Use deployment-scoped token if the manager returned one, otherwise keep the original
    let effective_token = init.deployment_token.unwrap_or_else(|| token.clone());

    // Track the deployment locally (with the effective token for restarts)
    let mut tracker = DeploymentTracker::new()?;
    tracker.track(
        name.clone(),
        deployment_id.clone(),
        effective_token.clone(),
        manager_url.clone(),
        platform_str.clone(),
    )?;

    match platform {
        Platform::Local | Platform::Kubernetes => {
            // Pull model: install agent as OS service (or print Helm commands for K8s)
            output::step(2, 3, "Setting up agent (pull model)...");
            run_pull_model(
                &args,
                &manager_url,
                &effective_token,
                &deployment_id,
                platform,
            )
            .await?;
        }
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            // Push model: run initial setup, then let manager continue
            output::step(2, 3, "Running initial setup (push model)...");
            run_push_model(&client, &deployment_id, platform).await?;
        }
        Platform::Test => {
            output::info("Test platform — no deployment action needed.");
        }
    }

    output::step(3, 3, "Done!");
    output::success(&format!("Deployment '{}' is being managed.", name));

    Ok(())
}

fn resolve_deployment_info(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<(String, String, String, String)> {
    // If name is provided, try to load from tracker
    if let Some(ref name) = args.name {
        let tracker = DeploymentTracker::new()?;
        if let Some(tracked) = tracker.get(name) {
            let token = args.token.clone().unwrap_or_else(|| tracked.token.clone());
            let manager_url = args
                .manager_url
                .clone()
                .unwrap_or_else(|| tracked.manager_url.clone());
            let platform = args
                .platform
                .clone()
                .unwrap_or_else(|| tracked.platform.clone());
            return Ok((token, manager_url, platform, name.clone()));
        }
    }

    // CLI args override embedded config, which overrides nothing (required)
    let token = args
        .token
        .clone()
        .or_else(|| embedded_config.and_then(|c| c.token.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "--token is required for new deployments. Get a token from 'alien onboard' or the deploy page.".to_string(),
            })
        })?;

    let manager_url = args
        .manager_url
        .clone()
        .or_else(|| embedded_config.and_then(|c| c.manager_url.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "manager_url".to_string(),
                message: "--manager-url is required for new deployments. Set ALIEN_MANAGER_URL or use --manager-url <url>.".to_string(),
            })
        })?;

    let platform = args
        .platform
        .clone()
        .or_else(|| embedded_config.and_then(|c| c.default_platform.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: "--platform is required for new deployments. Choose from: aws, gcp, azure, kubernetes, local.".to_string(),
            })
        })?;

    let name = args.name.clone().unwrap_or_else(|| {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "default".to_string())
    });

    Ok((token, manager_url, platform, name))
}

pub fn create_manager_client(token: &str, manager_url: &str) -> Result<ServerClient> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(ServerClient::new_with_client(manager_url, http_client))
}

/// Result of initializing with the manager.
struct InitResult {
    deployment_id: String,
    /// Deployment-scoped token returned by the manager (when using a deployment group token).
    /// If present, this should replace the original token for subsequent requests.
    deployment_token: Option<String>,
}

async fn initialize_deployment(
    client: &ServerClient,
    _token: &str,
    platform: Platform,
    name: &str,
) -> Result<InitResult> {
    let sdk_platform = match platform {
        Platform::Aws => alien_manager_api::types::Platform::Aws,
        Platform::Gcp => alien_manager_api::types::Platform::Gcp,
        Platform::Azure => alien_manager_api::types::Platform::Azure,
        Platform::Kubernetes => alien_manager_api::types::Platform::Kubernetes,
        Platform::Local => alien_manager_api::types::Platform::Local,
        Platform::Test => alien_manager_api::types::Platform::Test,
    };

    let body = alien_manager_api::types::InitializeRequest {
        name: Some(name.to_string()),
        platform: Some(sdk_platform),
    };

    let response = client
        .initialize()
        .body(body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to initialize with manager. Is the manager running? Check that --manager-url is correct.".to_string(),
        })?;

    let init = response.into_inner();
    Ok(InitResult {
        deployment_id: init.deployment_id,
        deployment_token: init.token,
    })
}

async fn run_pull_model(
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    platform: Platform,
) -> Result<()> {
    match platform {
        Platform::Kubernetes => run_kubernetes_pull_model(manager_url, token, deployment_id).await,
        _ => run_local_pull_model(args, manager_url, token, &platform.to_string()).await,
    }
}

async fn run_local_pull_model(
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    platform: &str,
) -> Result<()> {
    let encryption_key = args.encryption_key.clone().unwrap_or_else(|| {
        use super::agent::generate_encryption_key_public;
        generate_encryption_key_public()
    });

    // Find or download the alien-agent binary
    let binary_path = find_or_download_agent_binary().await?;

    output::info(&format!("Agent binary: {}", binary_path.display()));

    if args.foreground {
        return run_agent_foreground(&binary_path, manager_url, token, platform, &encryption_key)
            .await;
    }

    output::info("Installing alien-agent as a system service...");

    // Delegate to the agent install logic
    let install_args = super::agent::InstallArgs {
        binary: Some(binary_path),
        sync_url: manager_url.to_string(),
        sync_token: token.to_string(),
        platform: platform.to_string(),
        data_dir: None,
        encryption_key: Some(encryption_key),
    };

    super::agent::install_service(install_args)?;

    output::success("alien-agent installed and running as a system service.");
    output::info("The agent will sync with the manager and deploy updates automatically.");
    output::info("Use 'alien-deploy agent status' to check the service.");

    Ok(())
}

/// Run the agent as a foreground child process (for testing).
async fn run_agent_foreground(
    binary_path: &std::path::Path,
    manager_url: &str,
    token: &str,
    platform: &str,
    encryption_key: &str,
) -> Result<()> {
    output::info("Running agent in foreground (Ctrl+C to stop)...");

    let data_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".alien")
        .join("agent-data");

    let status = tokio::process::Command::new(binary_path)
        .arg("--platform")
        .arg(platform)
        .arg("--sync-url")
        .arg(manager_url)
        .arg("--sync-token")
        .arg(token)
        .arg("--encryption-key")
        .arg(encryption_key)
        .arg("--data-dir")
        .arg(&data_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to run agent: {}", binary_path.display()),
        })?;

    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Agent exited with status: {}", status),
        }));
    }

    Ok(())
}

async fn run_kubernetes_pull_model(
    manager_url: &str,
    token: &str,
    deployment_id: &str,
) -> Result<()> {
    output::info("Kubernetes platform detected — use Helm to install the agent.");
    output::info("");
    output::info("Add the chart and install:");
    println!();
    println!("  helm install alien-agent ./charts/alien-agent \\");
    println!("    --set syncUrl={} \\", manager_url);
    println!("    --set syncToken={} \\", token);
    println!("    --set encryptionKey=$(openssl rand -hex 32) \\",);
    println!("    --set namespace=<your-app-namespace>");
    println!();
    output::info(&format!("Deployment ID: {}", deployment_id));
    output::info("The agent will register with the manager on first sync.");

    Ok(())
}

/// Default releases URL for downloading binaries.
const DEFAULT_RELEASES_URL: &str = "https://releases.alien.dev";

/// Find the alien-agent binary locally, or download it from the releases URL.
async fn find_or_download_agent_binary() -> Result<std::path::PathBuf> {
    // Try to find it locally first
    if let Ok(path) = super::agent::which_agent_binary() {
        return Ok(path);
    }

    // Download to ~/.alien/bin/alien-agent
    let home = dirs::home_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine home directory".to_string(),
        })
    })?;

    let bin_dir = home.join(".alien").join("bin");
    std::fs::create_dir_all(&bin_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: bin_dir.display().to_string(),
            reason: "Failed to create ~/.alien/bin directory".to_string(),
        })?;

    let binary_path = bin_dir.join("alien-agent");

    let releases_url =
        std::env::var("ALIEN_RELEASES_URL").unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());

    let (os, arch) = detect_os_arch()?;
    let url = format!(
        "{}/alien-agent/latest/{}-{}/alien-agent",
        releases_url, os, arch
    );

    output::info(&format!("Downloading alien-agent from {}...", url));

    let response =
        reqwest::get(&url)
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to download alien-agent from {}", url),
            })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to download alien-agent: HTTP {}", response.status()),
        }));
    }

    let bytes =
        response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read alien-agent download response".to_string(),
            })?;

    std::fs::write(&binary_path, &bytes)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: binary_path.display().to_string(),
            reason: "Failed to write alien-agent binary".to_string(),
        })?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: binary_path.display().to_string(),
                reason: "Failed to make alien-agent executable".to_string(),
            })?;
    }

    output::success("alien-agent downloaded successfully.");

    Ok(binary_path)
}

fn detect_os_arch() -> Result<(&'static str, &'static str)> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported OS: {}", std::env::consts::OS),
        }));
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported architecture: {}", std::env::consts::ARCH),
        }));
    };

    Ok((os, arch))
}

async fn run_push_model(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
) -> Result<()> {
    output::info("Loading client configuration from environment...");

    let client_config = ClientConfig::from_std_env(platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                platform
            ),
        })?;

    push_initial_setup(client, deployment_id, platform, client_config, None, None).await
}

/// Run the push-model initial setup flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through InitialSetup until it reaches Provisioning (or a
/// terminal state), reconciles state back to the manager, and releases the lock.
///
/// This is used by both `alien-deploy up` (push model) and `alien-test` (e2e setup).
pub async fn push_initial_setup(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    client_config: ClientConfig,
    management_config: Option<alien_core::ManagementConfig>,
    image_pull_credentials: Option<alien_core::ImagePullCredentials>,
) -> Result<()> {
    // Get deployment from manager
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Reconstruct DeploymentState from flat API response
    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    let stack_state = deployment
        .stack_state
        .and_then(|v| serde_json::from_value(v).ok());
    let environment_info = deployment
        .environment_info
        .and_then(|v| serde_json::from_value(v).ok());

    // If there's a desired release, fetch the full release info
    let target_release = if let Some(ref release_id) = deployment.desired_release_id {
        match client.get_release().id(release_id).send().await {
            Ok(resp) => {
                let rel = resp.into_inner();
                // The API returns stacks keyed by platform (e.g. {"gcp": {...}}).
                // Extract the inner stack value for the target platform.
                let platform_stack_value = match platform {
                    Platform::Aws => rel.stack.aws,
                    Platform::Gcp => rel.stack.gcp,
                    Platform::Azure => rel.stack.azure,
                    Platform::Kubernetes => rel.stack.kubernetes,
                    Platform::Local => rel.stack.local,
                    Platform::Test => rel.stack.test,
                }
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!(
                            "Release {} has no stack for platform {}",
                            release_id,
                            platform.as_str()
                        ),
                    })
                })?;
                let stack = serde_json::from_value(platform_stack_value)
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to parse release stack".to_string(),
                    })?;

                Some(ReleaseInfo {
                    release_id: rel.id,
                    version: None,
                    description: None,
                    stack,
                })
            }
            Err(e) => {
                output::warn(&format!("Could not fetch release {}: {}", release_id, e));
                None
            }
        }
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release,
        stack_state,
        environment_info,
        runtime_metadata: None,
        retry_requested: deployment.retry_requested,
    };

    // Always override environment_info with the target client_config.
    // The manager may have already run the Pending step with management
    // credentials, setting environment_info to the management project.
    // push_initial_setup runs with *target* credentials, so re-collecting
    // ensures the environment_info reflects the actual target project.
    match alien_deployment::collect_environment_info(platform, &client_config).await {
        Ok(env_info) => {
            state.environment_info = Some(env_info);
        }
        Err(e) => {
            tracing::warn!("Failed to collect target environment info: {e}");
        }
    }

    // Reconstruct DeploymentConfig from stack_settings
    let stack_settings: StackSettings = deployment
        .stack_settings
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Build a minimal config JSON and deserialize to get proper defaults
    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "managementConfig": serde_json::to_value(&management_config).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    config.image_pull_credentials = image_pull_credentials;

    // Acquire sync lock — retry until the specific deployment is locked by us.
    // The manager's deployment loop may already hold the lock; we must wait for
    // it to release before proceeding.
    let session = format!("push-setup-{}", uuid::Uuid::new_v4());
    let max_acquire_attempts = 60;
    for attempt in 1..=max_acquire_attempts {
        let resp = client
            .acquire()
            .body(alien_manager_api::types::AcquireRequest {
                session: session.clone(),
                deployment_ids: Some(vec![deployment_id.to_string()]),
                statuses: None,
                platforms: None,
                limit: None,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::DeploymentFailed {
                operation: "acquire sync lock".to_string(),
            })?;

        if !resp.into_inner().deployments.is_empty() {
            break;
        }

        if attempt == max_acquire_attempts {
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                operation: "acquire sync lock: timed out waiting for lock".to_string(),
            }));
        }

        output::info(&format!(
            "Waiting for deployment lock (attempt {}/{})",
            attempt, max_acquire_attempts
        ));
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Re-fetch the deployment state now that we hold the lock.
    // The manager may have advanced the state while we were waiting.
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    state.status = status;
    state.stack_state = deployment
        .stack_state
        .and_then(|v| serde_json::from_value(v).ok());
    state.runtime_metadata = None;

    // Step loop with lock release guard
    let result = run_step_loop(&mut state, &config, &client_config, deployment_id).await;

    // Always reconcile + release, even on error.
    let state_json = serde_json::to_value(&state).unwrap_or_default();
    if let Err(e) = client
        .reconcile()
        .body(alien_manager_api::types::ReconcileRequest {
            deployment_id: deployment_id.to_string(),
            session: session.clone(),
            state: state_json,
            update_heartbeat: Some(false),
            error: None,
        })
        .send()
        .await
    {
        tracing::error!("Failed to reconcile deployment state: {e}");
        output::error(&format!("Failed to reconcile deployment state: {e}"));
    }

    if let Err(e) = client
        .release()
        .body(alien_manager_api::types::ReleaseRequest {
            deployment_id: deployment_id.to_string(),
            session: session.clone(),
        })
        .send()
        .await
    {
        tracing::error!("Failed to release sync lock: {e}");
        output::error(&format!("Failed to release sync lock: {e}"));
    }

    result
}

/// Inner step loop for push_initial_setup. Separated so the caller can
/// always reconcile + release regardless of the outcome.
async fn run_step_loop(
    state: &mut DeploymentState,
    config: &DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
) -> Result<()> {
    let max_steps = 200;

    for step_count in 1..=max_steps {
        info!("Step {}: status = {:?}", step_count, state.status);
        output::info(&format!("Step {}: {:?}", step_count, state.status));

        let step_result =
            alien_deployment::step(state.clone(), config.clone(), client_config.clone(), None)
                .await
                .context(ErrorData::DeploymentFailed {
                    operation: "initial setup".to_string(),
                })?;

        *state = step_result.state;

        // Synced or Running — done
        if state.status.is_synced() || matches!(state.status, DeploymentStatus::Running) {
            output::success("Initial setup complete. Manager will continue deployment.");
            return Ok(());
        }

        // Failed — report error
        if state.status.is_failed() {
            if let Some(err) = step_result.error {
                return Err(AlienError::new(ErrorData::DeploymentFailed {
                    operation: format!("initial setup: {}", err),
                }));
            }
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                operation: "initial setup".to_string(),
            }));
        }

        // Provisioning/Updating — hand off to manager
        if matches!(
            state.status,
            DeploymentStatus::Provisioning | DeploymentStatus::Updating
        ) {
            output::success("Initial setup complete. Manager will continue provisioning.");
            return Ok(());
        }

        if let Some(delay_ms) = step_result.suggested_delay_ms {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    Err(AlienError::new(ErrorData::DeploymentFailed {
        operation: format!(
            "initial setup exceeded {} steps for deployment {}",
            max_steps, deployment_id
        ),
    }))
}

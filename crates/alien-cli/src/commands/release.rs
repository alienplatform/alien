use crate::auth::AuthHttp;
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::git_utils::collect_git_metadata;
use crate::project_link::ensure_project_linked;
use crate::tui::{ErrorPrinter, ReleaseResult, ReleaseUiComponent, ReleaseUiEvent, ReleaseUiProps};
use crate::{ErrorData, Result};
use alien_build::settings::PushSettings;
use alien_core::{alien_event, AlienEvent, EventChange, EventHandler, EventState};
use alien_core::{Platform, Stack};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    CreateReleaseRequest, CreateReleaseRequestProject, CreateReleaseRequestRootDirectory,
    CreateReleaseWorkspace, GitMetadata, StackByPlatform,
};
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use clap::Parser;
use dockdash::{ClientProtocol, RegistryAuth};
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{mpsc, Arc};
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Push images and create a release",
    long_about = "Push built images to a container registry and create a new release on the Alien platform. By default, retrieves registry credentials from the platform's agent manager. Use override flags for custom registries (e.g., deploying the agent manager itself).",
    after_help = "EXAMPLES:
    # Standard release (auto-fetch registry credentials from platform)
    alien release

    # Release for specific project (skip linking)
    alien release --project my-project

    # Release without confirmation prompt
    alien release --yes

    # Release specific platform only
    alien release --platform aws

    # Output JSON (for scripting/automation)
    alien release --json --yes

    # Manual registry override (for deploying agent manager or custom setups)
    alien release --image-repo my-registry.com/my-app --registry-auth basic --registry-username user --registry-password pass

    # Skip git metadata collection
    alien release --no-git"
)]
pub struct ReleaseArgs {
    /// Target platform to release (if not specified, releases all built platforms)
    #[arg(long)]
    pub platform: Option<String>,

    /// Skip git metadata collection
    #[arg(long)]
    pub no_git: bool,

    /// Project ID or name to use for release (skips project linking)
    #[arg(long)]
    pub project: Option<String>,

    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,

    /// Output JSON instead of human-readable text (implies --no-tui)
    #[arg(long)]
    pub json: bool,

    /// Disable TUI and use console output instead
    #[arg(long)]
    pub no_tui: bool,

    // Manual registry override options (for agent manager deployment)
    /// Image repository URL (manual override - skips platform agent manager)
    #[arg(long)]
    pub image_repo: Option<String>,

    /// Registry auth: "basic" or "anonymous"
    #[arg(long)]
    pub registry_auth: Option<String>,

    /// Registry protocol: "http" or "https"
    #[arg(long)]
    pub registry_protocol: Option<String>,

    /// Registry username (required for basic auth)
    #[arg(long)]
    pub registry_username: Option<String>,

    /// Registry password (required for basic auth)
    #[arg(long)]
    pub registry_password: Option<String>,
}

/// JSON output for release command
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseJsonOutput {
    success: bool,
    release_id: Option<String>,
    project: String,
    workspace: String,
    platforms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Main entry point for release command - handles TUI vs console mode
pub async fn release_command(args: ReleaseArgs, ctx: ExecutionMode) -> Result<()> {
    // JSON mode: implies --no-tui and outputs structured JSON
    if args.json {
        match release_task_json(args, ctx).await {
            Ok(output) => {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
                if !output.success {
                    std::process::exit(1);
                }
                Ok(())
            }
            Err(error) => {
                // Even errors should be JSON in JSON mode
                let output = ReleaseJsonOutput {
                    success: false,
                    release_id: None,
                    project: String::new(),
                    workspace: String::new(),
                    platforms: vec![],
                    error: Some(format!("{}", error)),
                };
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
                std::process::exit(1);
            }
        }
    }
    // Use TUI only if no_tui is false and we're in a TTY environment
    else if !args.no_tui && std::io::stderr().is_terminal() && std::io::stdout().is_terminal() {
        match run_release_with_tui(args, ctx).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ =
                    ErrorPrinter::print_alien_error(&error.into_generic(), Some("RELEASE FAILED"));
                std::process::exit(1);
            }
        }
    } else {
        match release_task(args, ctx).await {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ =
                    ErrorPrinter::print_alien_error(&error.into_generic(), Some("RELEASE FAILED"));
                std::process::exit(1);
            }
        }
    }
}

/// Release task that returns JSON-serializable output
async fn release_task_json(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseJsonOutput> {
    let config = load_release_config(&args, &ctx).await?;

    // Run core release logic
    let result = release_task_core(
        args,
        config.output_dir,
        config.http,
        config.workspace_name.clone(),
        config.project_link.clone(),
        config.git_metadata,
        config.platforms.clone(),
    )
    .await;

    match result {
        Ok(ReleaseResult::Success { release_id }) => Ok(ReleaseJsonOutput {
            success: true,
            release_id: Some(release_id),
            project: config.project_link.project_name,
            workspace: config.workspace_name,
            platforms: config.platforms,
            error: None,
        }),
        Ok(ReleaseResult::Failed(err)) => Ok(ReleaseJsonOutput {
            success: false,
            release_id: None,
            project: config.project_link.project_name,
            workspace: config.workspace_name,
            platforms: config.platforms,
            error: Some(format!("{}", err)),
        }),
        Err(err) => Err(err),
    }
}

/// Event handler for alien events that forwards to ReleaseUiComponent
struct ReleaseEventHandler {
    tx: mpsc::Sender<ReleaseUiEvent>,
}

impl ReleaseEventHandler {
    fn new(tx: mpsc::Sender<ReleaseUiEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventHandler for ReleaseEventHandler {
    async fn on_event_change(&self, change: EventChange) -> alien_core::Result<()> {
        let _ = self.tx.send(ReleaseUiEvent::AlienEventChange(change));
        Ok(())
    }
}

/// Run release with TUI using the ReleaseUiComponent
async fn run_release_with_tui(args: ReleaseArgs, ctx: ExecutionMode) -> Result<()> {
    let config = load_release_config(&args, &ctx).await?;

    // Create the ReleaseUiComponent with props
    let props = ReleaseUiProps {
        platforms: config.platforms.clone(),
        project_name: config.project_link.project_name.clone(),
        on_result: None,
        on_cancel: None,
    };

    let mut ui_component = ReleaseUiComponent::new(props);

    // Start the component and get the event sender
    let ui_event_tx = ui_component
        .start()
        .context(ErrorData::TuiOperationFailed {
            message: "Failed to start release UI component".to_string(),
        })?;

    // Set up alien event handler that forwards to UI component
    let event_handler = ReleaseEventHandler::new(ui_event_tx.clone());
    let bus = alien_core::EventBus::with_handlers(vec![Arc::new(event_handler)]);

    // Move config fields into the spawned task
    let args_for_task = args.clone();
    let http_for_task = config.http;
    let workspace_name_for_task = config.workspace_name;
    let project_link_for_task = config.project_link;
    let git_metadata_for_task = config.git_metadata;
    let platforms_for_task = config.platforms;
    let output_dir_for_task = config.output_dir;

    // Run the release task in the background
    let release_ui_tx = ui_event_tx.clone();
    let release_handle = tokio::spawn(async move {
        let result = bus
            .run(|| async {
                // Emit LoadingConfiguration to mark first step as complete
                let _ = AlienEvent::LoadingConfiguration
                    .emit_with_state(EventState::Success)
                    .await;

                // Run core logic
                release_task_core(
                    args_for_task,
                    output_dir_for_task,
                    http_for_task,
                    workspace_name_for_task,
                    project_link_for_task,
                    git_metadata_for_task,
                    platforms_for_task,
                )
                .await
            })
            .await;

        let _ = release_ui_tx.send(ReleaseUiEvent::ReleaseFinished(result));
    });

    // Run the UI component event loop (this blocks until completion)
    let ui_result = ui_component
        .run_event_loop()
        .context(ErrorData::TuiOperationFailed {
            message: "Release UI component failed".to_string(),
        });

    // Handle release task completion
    match release_handle.await {
        Ok(_) => {}
        Err(e) if e.is_cancelled() => {}
        Err(e) => {
            return Err(AlienError::new(ErrorData::TuiOperationFailed {
                message: format!("Task join error: {}", e),
            }))
        }
    }

    ui_result
}

/// Resolved release configuration (shared across TUI, console, and JSON modes)
struct ReleaseConfig {
    output_dir: PathBuf,
    http: AuthHttp,
    workspace_name: String,
    project_link: crate::project_link::ProjectLink,
    git_metadata: Option<GitMetadata>,
    platforms: Vec<String>,
}

/// Load and validate all release configuration from args + execution context.
/// This is the single place where auth, workspace, project, and platform discovery happen.
async fn load_release_config(args: &ReleaseArgs, ctx: &ExecutionMode) -> Result<ReleaseConfig> {
    let current_dir = get_current_dir()?;
    let output_dir = current_dir.join(".alien");

    if !output_dir.exists() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "No .alien directory found. Please run `alien build` first.".to_string(),
        }));
    }

    let http = ctx.auth_http().await?;
    let workspace_name = ctx.resolve_workspace().await?;

    // Release's own --project takes precedence over global --project
    let effective_project = args.project.as_deref().or(ctx.project_override());
    let project_link = if let Some(project_name) = effective_project {
        crate::project_link::get_project_by_name(&http, &workspace_name, project_name).await?
    } else {
        ensure_project_linked(&current_dir, &http, &workspace_name).await?
    };

    let git_metadata = if args.no_git {
        None
    } else {
        match collect_git_metadata(&current_dir) {
            Ok(metadata) => Some(metadata),
            Err(e) => {
                info!("Warning: Failed to collect git metadata: {}", e);
                None
            }
        }
    };

    let platforms = if let Some(platform_str) = &args.platform {
        vec![platform_str.clone()]
    } else {
        discover_built_platforms(&output_dir)?
    };

    if platforms.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "build output".to_string(),
            message:
                "No built platforms found in .alien directory. Please run `alien build` first."
                    .to_string(),
        }));
    }

    Ok(ReleaseConfig {
        output_dir,
        http,
        workspace_name,
        project_link,
        git_metadata,
        platforms,
    })
}

/// Core release logic for console mode (with validation and confirmation)
async fn release_task(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseResult> {
    info!("Starting release command");

    // Step 1: Load configuration (with event scope)
    let config = AlienEvent::LoadingConfiguration
        .in_scope(|_| async {
            let config = load_release_config(&args, &ctx).await?;

            // Console mode: print summary and confirm
            print_release_summary_new(
                &config.platforms,
                &config.project_link.project_name,
                &config.git_metadata,
            );

            if !confirm_release(args.yes)? {
                return Err(AlienError::new(ErrorData::GenericError {
                    message: "Release cancelled.".to_string(),
                }));
            }

            Ok(config)
        })
        .await?;

    release_task_core(
        args,
        config.output_dir,
        config.http,
        config.workspace_name,
        config.project_link,
        config.git_metadata,
        config.platforms,
    )
    .await
}

/// Core release logic (shared by TUI and console modes)
async fn release_task_core(
    args: ReleaseArgs,
    output_dir: PathBuf,
    http: AuthHttp,
    workspace_name: String,
    project_link: crate::project_link::ProjectLink,
    git_metadata: Option<GitMetadata>,
    platforms_to_release: Vec<String>,
) -> Result<ReleaseResult> {
    let client = http.sdk_client();

    // Process each platform: load stack, push images, collect pushed stacks
    let mut stack_by_platform = StackByPlatform {
        aws: None,
        gcp: None,
        azure: None,
        kubernetes: None,
        local: None,
        test: None,
    };

    for platform_str in &platforms_to_release {
        info!("Processing {} platform...", platform_str);

        // Parse platform
        let platform = Platform::from_str(platform_str).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: e,
            })
        })?;

        // Load built stack
        let built_stack = load_built_stack(&output_dir, platform_str)?;

        // Get push settings (either from platform API or manual override)
        let push_settings = if let Some(ref image_repo) = args.image_repo {
            // Manual override mode (for agent manager deployment)
            create_manual_push_settings(&args, image_repo)?
        } else {
            // Auto-fetch mode (standard workflow)
            fetch_push_settings_from_platform(
                &http,
                &workspace_name,
                &project_link.project_id,
                &platform,
            )
            .await?
        };

        info!("   Pushing images to {}...", push_settings.repository);

        // Push images and get updated stack (emits PushingStack and PushingResource events)
        let pushed_stack = alien_build::push_stack(built_stack, platform, &push_settings)
            .await
            .context(ErrorData::ReleaseFailed {
                message: format!("Failed to push images for {} platform", platform_str),
            })?;

        // Convert to JSON for the API
        let stack_json = serde_json::to_value(&pushed_stack)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: format!("Failed to serialize pushed stack for {}", platform_str),
            })?;

        // Store in the appropriate platform field
        match platform {
            Platform::Aws => stack_by_platform.aws = Some(stack_json),
            Platform::Gcp => stack_by_platform.gcp = Some(stack_json),
            Platform::Azure => stack_by_platform.azure = Some(stack_json),
            Platform::Kubernetes => stack_by_platform.kubernetes = Some(stack_json),
            Platform::Local => stack_by_platform.local = Some(stack_json),
            #[allow(unreachable_patterns)]
            _ => {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: format!("Cannot release {} platform", platform.as_str()),
                }));
            }
        }

        info!("   ✓ {} platform ready", platform_str);
    }

    // Create release request
    let release_request = CreateReleaseRequest {
        project: CreateReleaseRequestProject::try_from(project_link.project_id.clone()).map_err(
            |e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "project".to_string(),
                    message: format!("Invalid project: {}", e),
                })
            },
        )?,
        git_metadata,
        stack: stack_by_platform,
        root_directory: project_link
            .root_directory
            .clone()
            .and_then(|rd| CreateReleaseRequestRootDirectory::try_from(rd).ok()),
    };

    // Create the release on the platform
    let release_id = create_platform_release(
        client.clone(),
        workspace_name,
        project_link,
        release_request,
    )
    .await?;

    Ok(ReleaseResult::Success { release_id })
}

/// Create a release on the platform
#[alien_event(AlienEvent::CreatingRelease {
    project: project_link.project_name.clone(),
})]
async fn create_platform_release(
    client: alien_platform_api::Client,
    workspace_name: String,
    project_link: crate::project_link::ProjectLink,
    release_request: CreateReleaseRequest,
) -> Result<String> {
    info!("Creating release on platform...");

    let workspace_param =
        CreateReleaseWorkspace::try_from(workspace_name.as_str()).map_err(|e| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Invalid workspace '{}': {}", workspace_name, e),
            })
        })?;

    let response = client
        .create_release()
        .workspace(&workspace_param)
        .body(&release_request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create release".to_string(),
            url: None,
        })?;

    let release = response.into_inner();
    let release_id = (*release.id).clone();

    info!("✅ Release created successfully!");
    info!("   ID: {}", release_id);
    info!(
        "   Project: {}/{}",
        workspace_name, project_link.project_name
    );

    if let Some(ref git_meta) = release.git_metadata {
        if let Some(ref inner) = git_meta.0 {
            if let Some(ref sha) = inner.commit_sha {
                let short_sha = if sha.len() > 7 { &sha[..7] } else { sha };
                info!("   Commit: {}", short_sha);
            }
        }
    }

    Ok(release_id)
}

/// Discover which platforms have been built
fn discover_built_platforms(output_dir: &PathBuf) -> Result<Vec<String>> {
    let build_dir = output_dir.join("build");
    if !build_dir.exists() {
        return Ok(Vec::new());
    }

    let mut platforms = Vec::new();
    for platform in &["aws", "gcp", "azure", "kubernetes", "local"] {
        let stack_file = build_dir.join(platform).join("stack.json");
        if stack_file.exists() {
            platforms.push(platform.to_string());
        }
    }

    Ok(platforms)
}

/// Load a built stack from .alien/build/{platform}/stack.json
fn load_built_stack(output_dir: &PathBuf, platform: &str) -> Result<Stack> {
    let stack_file = output_dir.join("build").join(platform).join("stack.json");

    if !stack_file.exists() {
        return Err(AlienError::new(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: stack_file.display().to_string(),
            reason: format!(
                "stack.json not found for platform {}. Run 'alien build --platform {}' first.",
                platform, platform
            ),
        }));
    }

    let content = fs::read_to_string(&stack_file).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: stack_file.display().to_string(),
            reason: format!("Failed to read stack.json for platform {}", platform),
        },
    )?;

    // Parse as Stack
    let stack: Stack =
        serde_json::from_str(&content)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: format!("Failed to parse stack.json for platform {}", platform),
            })?;

    Ok(stack)
}

/// Create push settings from manual CLI arguments
fn create_manual_push_settings(args: &ReleaseArgs, image_repo: &str) -> Result<PushSettings> {
    let auth = match args.registry_auth.as_deref().unwrap_or("anonymous") {
        "anonymous" => RegistryAuth::Anonymous,
        "basic" => {
            let username = args.registry_username.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::AuthCredentialsMissing {
                    field: "registry-username".to_string(),
                })
            })?;
            let password = args.registry_password.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::AuthCredentialsMissing {
                    field: "registry-password".to_string(),
                })
            })?;
            RegistryAuth::Basic(username.clone(), password.clone())
        }
        other => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "registry_auth".to_string(),
                message: format!(
                    "Unknown registry auth type: '{}'. Supported: anonymous, basic",
                    other
                ),
            }));
        }
    };

    let protocol = match args.registry_protocol.as_deref().unwrap_or("https") {
        "http" => ClientProtocol::Http,
        "https" => ClientProtocol::Https,
        other => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "registry_protocol".to_string(),
                message: format!(
                    "Unknown registry protocol: '{}'. Supported: http, https",
                    other
                ),
            }));
        }
    };

    Ok(PushSettings {
        repository: image_repo.to_string(),
        options: dockdash::PushOptions {
            auth,
            protocol,
            ..Default::default()
        },
    })
}

/// Fetch push settings from the platform's agent manager
async fn fetch_push_settings_from_platform(
    http: &AuthHttp,
    workspace_name: &str,
    project_id: &str,
    platform: &Platform,
) -> Result<PushSettings> {
    let client = http.sdk_client();

    info!("   Fetching build configuration from platform...");

    // Call the build-config API endpoint with retry logic for agent manager startup
    use alien_platform_api::types::{
        GetProjectBuildConfigPlatform, GetProjectBuildConfigWorkspace, ProjectIdOrNamePathParam,
    };

    let workspace_param =
        GetProjectBuildConfigWorkspace::try_from(workspace_name).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "workspace".to_string(),
                message: format!("Invalid workspace: {}", e),
            })
        })?;

    let project_param = ProjectIdOrNamePathParam::try_from(project_id).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "project".to_string(),
            message: format!("Invalid project: {}", e),
        })
    })?;

    let platform_param =
        GetProjectBuildConfigPlatform::try_from(platform.as_str()).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!("Invalid platform: {}", e),
            })
        })?;

    // Retry logic: agent manager might be starting up
    let max_duration = std::time::Duration::from_secs(60); // 1 minute total
    let start_time = std::time::Instant::now();
    let mut attempt = 0;

    let build_config = loop {
        attempt += 1;

        let result = client
            .get_project_build_config()
            .id_or_name(&project_param)
            .platform(&platform_param)
            .workspace(&workspace_param)
            .send()
            .await
            .into_sdk_error();

        match result {
            Ok(response) => break response.into_inner(),
            Err(sdk_err) => {
                // Check if this is a retryable error (503 - agent manager not ready)
                let is_retryable = sdk_err.http_status_code == Some(503);

                if is_retryable && start_time.elapsed() < max_duration {
                    // Calculate backoff: 2s, 4s, 8s, 16s, capped at 15s
                    let backoff_secs = std::cmp::min(2u64.pow(attempt - 1), 15);

                    if attempt == 1 {
                        info!("   Agent manager not ready yet, waiting for startup...");
                    } else {
                        info!(
                            "   Still waiting for agent manager (attempt {})...",
                            attempt
                        );
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    continue;
                } else {
                    // Either non-retryable error or timeout exceeded
                    if is_retryable {
                        let mut err = AlienError::new(ErrorData::ApiRequestFailed {
                            message: format!(
                                "Agent manager for {} platform did not become ready within {} seconds",
                                platform.as_str(),
                                max_duration.as_secs()
                            ),
                            url: None,
                        });
                        err.source = Some(Box::new(sdk_err));
                        return Err(err);
                    } else {
                        let mut err = AlienError::new(ErrorData::ApiRequestFailed {
                            message: format!(
                                "Failed to get build configuration for {} platform",
                                platform.as_str()
                            ),
                            url: None,
                        });
                        err.source = Some(Box::new(sdk_err));
                        return Err(err);
                    }
                }
            }
        }
    };

    if attempt > 1 {
        info!("   ✓ Agent manager ready after {} attempt(s)", attempt);
    }

    info!("   Manager: {}", build_config.manager_url);
    info!("   Repository: {}", build_config.repository_name);

    // Now call the manager to get credentials
    info!("   Generating repository credentials...");

    use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
    let reqwest_client = reqwest::Client::builder()
        .default_headers({
            let mut headers = HeaderMap::new();
            headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));
            headers
        })
        .build()
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to create HTTP client for manager".to_string(),
            url: None,
        })?;

    // Call manager API to generate credentials
    let credentials_response = reqwest_client
        .post(format!(
            "{}/v1/artifact-registry/repositories/{}/credentials",
            build_config.manager_url, build_config.repository_name
        ))
        .json(&serde_json::json!({
            "operation": "push",
            "durationSeconds": 3600
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to request credentials from manager".to_string(),
            url: Some(build_config.manager_url.clone()),
        })?;

    if !credentials_response.status().is_success() {
        let status = credentials_response.status();
        let body = credentials_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::HttpRequestFailed {
            message: format!("Manager returned error {}: {}", status, body),
            url: Some(build_config.manager_url),
        }));
    }

    let credentials: serde_json::Value = credentials_response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "Failed to parse credentials response from agent manager".to_string(),
        })?;

    // Extract credentials from response
    let username = credentials
        .get("username")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Missing username in credentials response".to_string(),
            })
        })?
        .to_string();

    let password = credentials
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Missing password in credentials response".to_string(),
            })
        })?
        .to_string();

    // Determine repository URI
    let repository_uri = build_config
        .repository_uri
        .unwrap_or_else(|| build_config.repository_name.clone());

    // Translate registry URL for CLI access.
    // The agent manager container sees the host registry as host.docker.internal:<port>,
    // but the CLI runs on the host itself, so we translate to localhost:<port>.
    let (repository_uri, protocol) = translate_registry_url_for_cli(&repository_uri, platform)?;

    info!("   ✓ Credentials generated successfully");

    Ok(PushSettings {
        repository: repository_uri,
        options: dockdash::PushOptions {
            auth: RegistryAuth::Basic(username, password),
            protocol,
            ..Default::default()
        },
    })
}

/// Translate registry URL for CLI access.
///
/// The local platform's artifact registry runs on localhost but is exposed to containers
/// as host.docker.internal. The CLI runs on the host, so it needs localhost URLs.
/// This translation applies regardless of platform -- in development setups a single
/// local agent manager may serve all platforms.
///
/// # Arguments
/// * `repository_uri` - Registry URL from agent manager (e.g., "host.docker.internal:5000/artifacts/repo")
/// * `platform` - Target platform being deployed to
///
/// # Returns
/// Tuple of (translated_uri, protocol) where protocol is HTTP for localhost, HTTPS otherwise
fn translate_registry_url_for_cli(
    repository_uri: &str,
    platform: &Platform,
) -> Result<(String, ClientProtocol)> {
    // Parse registry URL to extract hostname
    // Registry URLs are in format: hostname:port/path or just hostname/path
    // Examples: "localhost:5000/artifacts/repo", "123.dkr.ecr.us-east-1.amazonaws.com/repo"
    let hostname = if let Some(slash_pos) = repository_uri.find('/') {
        &repository_uri[..slash_pos]
    } else {
        repository_uri
    };

    // Extract just the hostname part (before port if present)
    let hostname_without_port = if let Some(colon_pos) = hostname.rfind(':') {
        // Check if this is actually a port (not IPv6)
        // For simplicity, we assume IPv6 addresses won't appear in registry URLs
        &hostname[..colon_pos]
    } else {
        hostname
    };

    // Translate host.docker.internal to localhost for CLI access.
    // host.docker.internal is how containers reach the host, but the CLI runs on the host itself.
    // This is the normal case for the local platform, and also valid in development setups
    // where a single local agent manager serves multiple platforms.
    if hostname_without_port == "host.docker.internal" {
        if platform != &Platform::Local {
            info!(
                platform = %platform.as_str(),
                "Using local registry for non-local platform (typical in development)"
            );
        }

        let translated_uri = repository_uri.replace("host.docker.internal", "localhost");

        info!(
            original = %repository_uri,
            translated = %translated_uri,
            "Translated container URL to host URL"
        );

        return Ok((translated_uri, ClientProtocol::Http));
    }

    // Determine protocol based on hostname
    // Local registries (localhost, 127.0.0.1) use HTTP, everything else uses HTTPS
    let protocol = if hostname_without_port == "localhost" || hostname_without_port == "127.0.0.1" {
        ClientProtocol::Http
    } else {
        ClientProtocol::Https
    };

    Ok((repository_uri.to_string(), protocol))
}

/// Print a summary of what will be released
fn print_release_summary_new(
    platforms: &[String],
    project_name: &str,
    git_metadata: &Option<GitMetadata>,
) {
    println!("📦 Release Summary");
    println!("   Project: {}", project_name);

    if let Some(git_meta) = git_metadata {
        if let Some(ref inner) = git_meta.0 {
            if let Some(ref branch) = inner.commit_ref {
                println!("   Branch: {}", **branch);
            }
            if let Some(ref sha) = inner.commit_sha {
                let short_sha = if sha.len() > 7 { &sha[..7] } else { sha };
                println!("   Commit: {}", short_sha);
            }
            if let Some(ref message) = inner.commit_message {
                // Truncate long commit messages
                let message_str = if message.len() > 60 {
                    format!("{}...", &message[..57])
                } else {
                    message.to_string()
                };
                println!("   Message: {}", message_str);
            }
        }
    }

    println!("   Platforms:");
    for platform in platforms {
        let display_name = match platform.as_str() {
            "aws" => "AWS",
            "gcp" => "Google Cloud Platform",
            "azure" => "Azure",
            "kubernetes" => "Kubernetes",
            "local" => "Local",
            other => other,
        };
        println!("     • {}", display_name);
    }
    println!();
}

/// Confirm release creation with user
fn confirm_release(yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }

    print!("Create this release? [Y/n] ");
    use std::io::{self, Write};
    io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::TuiOperationFailed {
            message: "Failed to flush stdout".to_string(),
        })?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::TuiOperationFailed {
            message: "Failed to read user input".to_string(),
        })?;

    let input = input.trim().to_lowercase();
    Ok(input.is_empty() || input == "y" || input == "yes")
}

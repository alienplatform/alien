use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::get_current_dir;
use crate::git_utils::collect_git_metadata;
use crate::output::{can_prompt, print_json, prompt_confirm};
use crate::{ErrorData, Result};
use alien_build::settings::PushSettings;
use alien_core::{alien_event, AlienEvent};
use alien_core::{Platform, Stack};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::types::{
    CreateReleaseRequest as ManagerCreateReleaseRequest, StackByPlatform as ManagerStackByPlatform,
};
use alien_platform_api::types::GitMetadata;
use clap::Parser;
use dockdash::{ClientProtocol, RegistryAuth};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Push images and create a release",
    long_about = "Push built images to a container registry and create a new release on the Alien platform. By default, retrieves registry credentials from the platform's manager. Use override flags for custom registries (e.g., deploying the manager itself).",
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

    # Manual registry override (for deploying manager or custom setups)
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

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,

    // Manual registry override options (for manager deployment)
    /// Image repository URL (manual override - skips platform manager)
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

type ReleaseResult = String;

/// Main entry point for the release command.
pub async fn release_command(args: ReleaseArgs, ctx: ExecutionMode) -> Result<()> {
    if args.json {
        match release_task_json(args, ctx).await {
            Ok(output) => {
                print_json(&output)?;
                Ok(())
            }
            Err(error) => {
                print_json(&ReleaseJsonOutput {
                    success: false,
                    release_id: None,
                    project: String::new(),
                    workspace: String::new(),
                    platforms: vec![],
                    error: Some(format!("{}", error)),
                })?;
                Err(error)
            }
        }
    } else {
        release_task(args, ctx).await.map(|_| ())
    }
}

/// Release task that returns JSON-serializable output
async fn release_task_json(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseJsonOutput> {
    let config = load_release_config(&args, &ctx, false).await?;

    let project_name = config.project_link.project_name.clone();
    let workspace_name = config.workspace_name.clone();
    let platforms = config.platforms.clone();

    match release_task_core(args, config).await {
        Ok(release_id) => Ok(ReleaseJsonOutput {
            success: true,
            release_id: Some(release_id),
            project: project_name,
            workspace: workspace_name,
            platforms,
            error: None,
        }),
        Err(err) => Err(err),
    }
}

/// Resolved release configuration shared across human and JSON output paths.
struct ReleaseConfig {
    output_dir: PathBuf,
    manager: ManagerContext,
    workspace_name: String,
    project_link: crate::project_link::ProjectLink,
    git_metadata: Option<GitMetadata>,
    platforms: Vec<String>,
}

/// Load and validate all release configuration from args + execution context.
/// This is the single place where auth, workspace, project, and platform discovery happen.
async fn load_release_config(
    args: &ReleaseArgs,
    ctx: &ExecutionMode,
    allow_bootstrap: bool,
) -> Result<ReleaseConfig> {
    let current_dir = get_current_dir()?;
    let output_dir = current_dir.join(".alien");

    let workspace_name = ctx.resolve_workspace_with_bootstrap(allow_bootstrap).await?;

    // Resolve project
    let (_project_id, project_link) = ctx
        .resolve_project(args.project.as_deref(), allow_bootstrap)
        .await?;

    // Determine platforms
    let platforms = if let Some(platform_str) = &args.platform {
        vec![platform_str.clone()]
    } else {
        discover_built_platforms(&output_dir)?
    };

    if platforms.is_empty() && !output_dir.exists() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "No .alien directory found. Please run `alien build` first.".to_string(),
        }));
    }

    // Auto-build if no build output exists (for any discovered platform)
    let first_platform = args.platform.as_deref().unwrap_or("local");
    let stack_file = output_dir
        .join("build")
        .join(first_platform)
        .join("stack.json");
    if !stack_file.exists() {
        println!(
            "No build found for {} platform, building...",
            first_platform
        );

        let platform = Platform::from_str(first_platform).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: e,
            })
        })?;

        let config_path = current_dir.clone();
        let stack = crate::config::load_configuration(config_path)
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to load configuration".to_string(),
            })?;

        let platform_build_settings = match platform {
            Platform::Aws => alien_build::settings::PlatformBuildSettings::Aws {
                managing_account_id: None,
            },
            Platform::Gcp => alien_build::settings::PlatformBuildSettings::Gcp {},
            Platform::Azure => alien_build::settings::PlatformBuildSettings::Azure {},
            Platform::Kubernetes => alien_build::settings::PlatformBuildSettings::Kubernetes {},
            Platform::Local => alien_build::settings::PlatformBuildSettings::Local {},
            Platform::Test => alien_build::settings::PlatformBuildSettings::Test {},
        };

        let targets = match platform {
            Platform::Local => Some(vec![alien_core::BinaryTarget::current_os()]),
            _ => None,
        };

        let settings = alien_build::settings::BuildSettings {
            output_directory: output_dir.to_str().unwrap().to_string(),
            platform: platform_build_settings,
            targets,
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };

        alien_build::build_stack(stack, &settings)
            .await
            .context(ErrorData::BuildFailed)?;
    }

    // Re-discover platforms after potential auto-build
    let platforms = if let Some(platform_str) = &args.platform {
        vec![platform_str.clone()]
    } else {
        let discovered = discover_built_platforms(&output_dir)?;
        if discovered.is_empty() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "build output".to_string(),
                message:
                    "No built platforms found in .alien directory. Please run `alien build` first."
                        .to_string(),
            }));
        }
        discovered
    };

    // Resolve manager (discovers URL in Platform mode, known in Standalone/Dev)
    let manager = ctx
        .resolve_manager(&project_link.project_id, &platforms[0])
        .await?;

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

    Ok(ReleaseConfig {
        output_dir,
        manager,
        workspace_name,
        project_link,
        git_metadata,
        platforms,
    })
}

/// Core release logic for console mode (with validation and confirmation)
async fn release_task(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseResult> {
    println!("Preparing release...");

    let config = AlienEvent::LoadingConfiguration
        .in_scope(|_| async {
            let config = load_release_config(&args, &ctx, true).await?;

            print_release_summary_new(
                &config.platforms,
                &config.project_link.project_name,
                &config.git_metadata,
            );

            if !confirm_release(args.yes, false)? {
                return Err(AlienError::new(ErrorData::UserCancelled));
            }

            Ok(config)
        })
        .await?;

    let release_id = release_task_core(args, config).await?;
    println!("Release created: {release_id}");
    println!("Next: run `alien deployments create ...` or `alien deploy ...` as needed.");
    Ok(release_id)
}

/// Core release logic shared by all output modes.
///
/// Always uses the manager SDK to create the release — no mode branching.
async fn release_task_core(args: ReleaseArgs, config: ReleaseConfig) -> Result<ReleaseResult> {
    let ReleaseConfig {
        output_dir,
        manager,
        git_metadata,
        platforms: platforms_to_release,
        ..
    } = config;

    // Process each platform: load stack, push images, collect pushed stacks
    let mut stack_by_platform = ManagerStackByPlatform {
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

        // Push images if needed (local/test platforms skip pushing)
        let pushed_stack = if platform != Platform::Local && platform != Platform::Test {
            // Get push settings (either manual override or from manager context)
            let push_settings = if let Some(ref image_repo) = args.image_repo {
                create_manual_push_settings(&args, image_repo)?
            } else {
                fetch_push_settings_from_manager(&manager, &platform).await?
            };

            info!("   Pushing images to {}...", push_settings.repository);

            alien_build::push_stack(built_stack, platform.clone(), &push_settings)
                .await
                .context(ErrorData::ReleaseFailed {
                    message: format!("Failed to push images for {} platform", platform_str),
                })?
        } else {
            built_stack
        };

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
            Platform::Test => stack_by_platform.test = Some(stack_json),
        }

        info!("   ✓ {} platform ready", platform_str);
    }

    // Convert platform SDK GitMetadata to manager SDK GitMetadata (serde roundtrip)
    let sdk_git_metadata = git_metadata.and_then(|m| {
        m.0.map(|inner| alien_manager_api::types::GitMetadata {
            commit_sha: inner.commit_sha.map(|s| s.to_string()),
            commit_ref: inner.commit_ref.map(|s| s.to_string()),
            commit_message: inner.commit_message.map(|s| s.to_string()),
        })
    });

    // Create release on the manager
    let release_id = create_manager_release(&manager, stack_by_platform, sdk_git_metadata).await?;

    Ok(release_id)
}

/// Create a release on the manager
#[alien_event(AlienEvent::CreatingRelease {
    project: "release".to_string(),
})]
async fn create_manager_release(
    manager: &ManagerContext,
    stack: ManagerStackByPlatform,
    git_metadata: Option<alien_manager_api::types::GitMetadata>,
) -> Result<String> {
    info!("Creating release on manager...");

    let response = manager
        .client
        .create_release()
        .body(ManagerCreateReleaseRequest {
            stack,
            git_metadata,
        })
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("Failed to create release: {}", e),
                url: None,
            })
        })?;

    let release_id = response.id.clone();

    info!("Release created successfully!");
    info!("   ID: {}", release_id);

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

/// Fetch push settings from the manager's artifact-registry credentials endpoint.
///
/// Uses `ManagerContext.repository_name` (available when resolved via platform discovery).
/// For Standalone mode without `--image-repo`, this will fail with a helpful error.
async fn fetch_push_settings_from_manager(
    manager: &ManagerContext,
    platform: &Platform,
) -> Result<PushSettings> {
    let repository_name = manager.repository_name.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "No repository name available. In standalone mode, use --image-repo to specify a container registry.\n\
                 Example: alien release --platform {} --image-repo my-registry.com/my-app",
                platform.as_str()
            ),
        })
    })?;

    info!("   Generating repository credentials...");

    // Call manager API to generate credentials
    let credentials_url = format!(
        "{}/v1/artifact-registry/repositories/{}/credentials",
        manager.manager_url, repository_name
    );

    // Use the authenticated HTTP client so the manager receives the same
    // Authorization header that the caller used.
    let credentials_response = manager
        .http_client
        .post(&credentials_url)
        .json(&serde_json::json!({
            "operation": "push",
            "durationSeconds": 3600
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to request credentials from manager".to_string(),
            url: Some(manager.manager_url.clone()),
        })?;

    if !credentials_response.status().is_success() {
        let status = credentials_response.status();
        let body = credentials_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::HttpRequestFailed {
            message: format!("Manager returned error {}: {}", status, body),
            url: Some(manager.manager_url.clone()),
        }));
    }

    let credentials: serde_json::Value = credentials_response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "Failed to parse credentials response from manager".to_string(),
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
    let repository_uri = manager
        .repository_uri
        .as_ref()
        .cloned()
        .unwrap_or_else(|| repository_name.clone());

    // Translate registry URL for CLI access.
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
/// local manager may serve all platforms.
///
/// # Arguments
/// * `repository_uri` - Registry URL from manager (e.g., "host.docker.internal:5000/artifacts/repo")
/// * `platform` - Target platform being deployed to
///
/// # Returns
/// Tuple of (translated_uri, protocol) where protocol is HTTP for localhost, HTTPS otherwise
fn translate_registry_url_for_cli(
    repository_uri: &str,
    platform: &Platform,
) -> Result<(String, ClientProtocol)> {
    // Parse registry URL to extract hostname
    let hostname = if let Some(slash_pos) = repository_uri.find('/') {
        &repository_uri[..slash_pos]
    } else {
        repository_uri
    };

    // Extract just the hostname part (before port if present)
    let hostname_without_port = if let Some(colon_pos) = hostname.rfind(':') {
        &hostname[..colon_pos]
    } else {
        hostname
    };

    // Translate host.docker.internal to localhost for CLI access.
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
fn confirm_release(yes: bool, json_mode: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }

    if json_mode || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Release confirmation requires a real terminal. Re-run with `--yes`."
                .to_string(),
        }));
    }

    prompt_confirm("Create this release?", true)
}

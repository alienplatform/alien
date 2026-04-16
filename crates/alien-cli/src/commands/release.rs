use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::get_current_dir;
use crate::git_utils::collect_git_metadata;
use crate::output::print_json;
use crate::ui::{command, contextual_heading, dim_label, success_line};
use crate::{ErrorData, Result};
use alien_build::settings::PushSettings;
use alien_core::{
    alien_event, AlienEvent, Container, ContainerCode, Function, FunctionCode, Platform, Stack,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::types::{
    CreateReleaseRequest as ManagerCreateReleaseRequest, StackByPlatform as ManagerStackByPlatform,
};
use alien_manager_api::SdkResultExt;
use alien_platform_api::types::GitMetadata;
use clap::Parser;
use dockdash::{ClientProtocol, RegistryAuth};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Push images and create a release",
    long_about = "Push built images to a container registry and create a new release on the Alien platform. By default, retrieves registry credentials from the platform's manager. Use override flags for custom registries (e.g., deploying the manager itself).",
    after_help = "EXAMPLES:
    # Standard release (auto-discovers platforms from manager config)
    alien release

    # Release for specific project (skip linking)
    alien release --project my-project

    # Release specific platforms only
    alien release --platforms aws
    alien release --platforms aws,gcp

    # Use pre-built and pre-pushed images (skip build and push)
    alien release --prebuilt

    # Output JSON (for scripting/automation)
    alien release --json

    # Manual registry override (for deploying manager or custom setups)
    alien release --image-repo my-registry.com/my-app --registry-auth basic --registry-username user --registry-password pass

    # Skip git metadata collection
    alien release --no-git"
)]
pub struct ReleaseArgs {
    /// Target platforms to release (comma-separated). If not specified, auto-discovers
    /// from the manager's configured artifact registries, or releases all built platforms.
    #[arg(long, value_delimiter = ',')]
    pub platforms: Option<Vec<String>>,

    /// Skip git metadata collection
    #[arg(long)]
    pub no_git: bool,

    /// Project ID or name to use for release (skips project linking)
    #[arg(long)]
    pub project: Option<String>,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,

    /// Allow experimental platforms (kubernetes, local)
    #[arg(long)]
    pub experimental: bool,

    /// Use pre-built and pre-pushed images. Skips both build and push steps.
    /// Requires that stack.json already contains remote image URIs.
    #[arg(long)]
    pub prebuilt: bool,

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
}

type ReleaseResult = String;

/// Main entry point for the release command.
pub async fn release_command(args: ReleaseArgs, ctx: ExecutionMode) -> Result<()> {
    if args.json {
        let output = release_task_json(args, ctx).await?;
        print_json(&output)?;
        Ok(())
    } else {
        release_task(args, ctx).await.map(|_| ())
    }
}

/// Release task that returns JSON-serializable output
async fn release_task_json(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseJsonOutput> {
    let config = load_release_config(&args, &ctx, false, false).await?;

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
    show_human_output: bool,
) -> Result<ReleaseConfig> {
    let current_dir = get_current_dir()?;
    let output_dir = current_dir.join(".alien");

    let workspace_name = ctx
        .resolve_workspace_with_bootstrap(allow_bootstrap)
        .await?;

    // Resolve project
    let (_project_id, project_link) = ctx
        .resolve_project(args.project.as_deref(), allow_bootstrap)
        .await?;

    // Determine target platforms:
    // 1. Explicit --platforms flag takes priority
    // 2. Otherwise, use already-built platforms
    // 3. If nothing built, ask the manager which platforms are configured
    let target_platforms = if let Some(ref platforms) = args.platforms {
        // Validate explicit platforms against experimental gating
        if !args.experimental {
            for p in platforms {
                if let Ok(platform) = Platform::from_str(p) {
                    if platform.is_experimental() {
                        return Err(AlienError::new(ErrorData::ValidationError {
                            field: "platforms".to_string(),
                            message: format!(
                                "Platform '{}' is experimental and not yet production-ready. Pass --experimental to use it anyway.",
                                p
                            ),
                        }));
                    }
                }
            }
        }
        platforms.clone()
    } else {
        let discovered = discover_built_platforms(&output_dir, args.experimental)?;
        if !discovered.is_empty() {
            discovered
        } else {
            let configured = fetch_configured_platforms(ctx).await;
            let configured: Vec<_> = if args.experimental {
                configured
            } else {
                configured.into_iter()
                    .filter(|p| !Platform::from_str(p).map(|p| p.is_experimental()).unwrap_or(false))
                    .collect()
            };
            if configured.is_empty() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platforms".to_string(),
                    message: "No platforms configured. Run `alien build --platform <aws|gcp|azure>` first, or specify --platforms explicitly.".to_string(),
                }));
            }
            configured
        }
    };

    // Build for every platform unless --prebuilt is set.
    // Content-hash dedup in the build layer makes this fast when nothing changed.
    if !args.prebuilt {
        let stack = crate::config::load_configuration(current_dir.clone())
            .await
            .context(ErrorData::ConfigurationError {
                message: "Failed to load configuration".to_string(),
            })?;
        for platform_str in &target_platforms {
            auto_build_for_platform(
                platform_str,
                &stack,
                &output_dir,
                show_human_output,
            )
            .await?;
        }
    }

    // Re-discover platforms after potential auto-build
    let platforms = if let Some(ref platforms) = args.platforms {
        platforms.clone()
    } else {
        let discovered = discover_built_platforms(&output_dir, args.experimental)?;
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

/// Core release logic for console mode.
async fn release_task(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseResult> {
    let is_dev = ctx.is_dev();
    let config = AlienEvent::LoadingConfiguration
        .in_scope(|_| async { load_release_config(&args, &ctx, true, true).await })
        .await?;

    let platforms_label = format_platform_summary(&config.platforms);
    println!(
        "{}",
        contextual_heading(
            "Releasing",
            &config.project_link.project_name,
            &[("for", &platforms_label)],
        )
    );

    let release_id = release_task_core(args, config).await?;
    println!("{}", success_line("Release created."));
    println!("{} {}", dim_label("Release"), release_id);
    if !is_dev {
        println!(
            "{} run {} for a new customer, or {} to check existing deployments.",
            dim_label("Next"),
            command("alien onboard <customer-name>"),
            command("alien deployments ls")
        );
    }
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
        let mut built_stack = load_built_stack(&output_dir, platform_str)?;

        // Push images if needed (test platform and --prebuilt skip pushing).
        // Local platform pushes to a cloud registry — the alien-agent pulls from it.
        let pushed_stack = if args.prebuilt {
            // Validate all images are remote URIs when using --prebuilt
            validate_prebuilt_stack(&built_stack)?;
            built_stack
        } else if platform != Platform::Test {
            // Load push cache — maps content-hashed dir names to previously pushed URIs
            let mut push_cache = load_push_cache(&output_dir, platform_str);

            // Keep a copy of the stack before cache application so we can map
            // original local paths → pushed URIs for cache updates later.
            let pre_push_stack = built_stack.clone();

            // Apply cached URIs to skip pushing already-pushed artifacts
            let cache_hits = apply_push_cache(&mut built_stack, &push_cache);
            if cache_hits > 0 {
                info!(
                    "   Skipping push for {} resource(s) (already pushed)",
                    cache_hits
                );
            }

            // Get push settings (either manual override or via manager proxy)
            let push_settings = if let Some(ref image_repo) = args.image_repo {
                create_manual_push_settings(&args, image_repo)?
            } else {
                build_proxy_push_settings(&manager, &platform).await?
            };

            info!("   Pushing images to {}...", push_settings.repository);

            let pushed = alien_build::push_stack(built_stack, platform.clone(), &push_settings)
                .await
                .context(ErrorData::ReleaseFailed {
                    message: format!("Failed to push images for {} platform", platform_str),
                })?;

            // Update and persist the push cache with newly pushed URIs
            collect_push_cache_entries(&pushed, &pre_push_stack, &mut push_cache);
            if let Err(e) = save_push_cache(&output_dir, platform_str, &push_cache) {
                info!("Warning: Failed to save push cache: {}", e);
            }

            pushed
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
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create release".to_string(),
            url: None,
        })?;

    let release_id = response.id.clone();

    info!("Release created successfully!");
    info!("   ID: {}", release_id);

    Ok(release_id)
}

/// Discover which platforms have been built
fn discover_built_platforms(output_dir: &PathBuf, include_experimental: bool) -> Result<Vec<String>> {
    let build_dir = output_dir.join("build");
    if !build_dir.exists() {
        return Ok(Vec::new());
    }

    let search_platforms = if include_experimental {
        Platform::DEPLOYABLE
    } else {
        Platform::STABLE
    };

    let mut platforms = Vec::new();
    for platform in search_platforms {
        let stack_file = build_dir.join(platform.as_str()).join("stack.json");
        if stack_file.exists() {
            platforms.push(platform.as_str().to_string());
        }
    }

    Ok(platforms)
}

/// Ask the manager which platforms have artifact registries configured.
///
/// Returns an empty vec on any failure (404, network error, etc.) so the caller
/// can fall back gracefully.
async fn fetch_configured_platforms(ctx: &ExecutionMode) -> Vec<String> {
    let url = format!("{}/v1/platforms", ctx.manager_url());

    let result: std::result::Result<Vec<String>, Box<dyn std::error::Error>> = async {
        let auth_http = ctx.auth_http().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let resp = auth_http.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let body: serde_json::Value = resp.json().await?;
        let platforms = body
            .get("platforms")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(platforms)
    }
    .await;

    match result {
        Ok(platforms) => {
            if !platforms.is_empty() {
                info!(
                    "Discovered configured platforms from manager: {:?}",
                    platforms
                );
            }
            platforms
        }
        Err(e) => {
            info!("Could not fetch configured platforms from manager: {}", e);
            Vec::new()
        }
    }
}

/// Build for a single platform.
async fn auto_build_for_platform(
    platform_str: &str,
    stack: &Stack,
    output_dir: &PathBuf,
    _show_human_output: bool,
) -> Result<()> {

    let platform = Platform::from_str(platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
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

    alien_build::build_stack(stack.clone(), &settings)
        .await
        .context(ErrorData::BuildFailed)?;

    Ok(())
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

/// Build push settings that push through the manager's OCI proxy.
///
/// The manager IS the container registry. Images are pushed to
/// `{manager_url}/v2/{repo_name}/{name}:{tag}` using the caller's auth token.
/// The proxy forwards to the upstream cloud registry transparently.
async fn build_proxy_push_settings(
    manager: &ManagerContext,
    platform: &Platform,
) -> Result<PushSettings> {
    let manager_url = &manager.manager_url;

    // Repository name — the upstream repo prefix. The proxy forwards the OCI
    // path as-is, so this must match the upstream repository name.
    // First try the statically-known repository_name (from platform mode).
    // If not available, call the manager's build-config endpoint to discover it.
    let repo_name = if let Some(ref name) = manager.repository_name {
        name.clone()
    } else {
        fetch_build_config_repo_name(manager, platform).await?
    };

    // Strip scheme to get the registry host (OCI clients use host:port, not URLs).
    let registry_host = alien_core::image_rewrite::strip_url_scheme(manager_url);

    // Translate host.docker.internal → localhost for CLI access (dev mode).
    let (registry_host, protocol) =
        translate_registry_url_for_cli(&registry_host, &Platform::Local)?;

    // Full repository: host/repo_name (e.g., "manager.alien.dev/alien-e2e")
    let repository = format!("{}/{}", registry_host, repo_name);

    // Auth: use the caller's token as Basic auth password.
    // The manager validates both Bearer and Basic auth — for OCI clients
    // (which speak Basic), the password is the token.
    let auth = match &manager.auth_token {
        Some(token) => RegistryAuth::Basic("token".to_string(), token.clone()),
        None => RegistryAuth::Anonymous,
    };

    info!("   Pushing through manager proxy at {}", repository);

    Ok(PushSettings {
        repository,
        options: dockdash::PushOptions {
            auth,
            protocol,
            // Always use monolithic push through the proxy. The proxy doesn't
            // know the upstream registry type, and some registries (GAR) reject
            // chunked PATCH uploads. Monolithic works with all registries.
            monolithic_push: dockdash::MonolithicPushPolicy::Always,
            ..Default::default()
        },
    })
}

/// Fetch the repository name prefix from the manager's build-config endpoint.
///
/// Called when the CLI doesn't have a statically-known repository name (standalone
/// and dev modes). The manager resolves the correct prefix based on the target
/// platform's artifact registry configuration.
async fn fetch_build_config_repo_name(
    manager: &ManagerContext,
    platform: &Platform,
) -> Result<String> {
    let url = format!(
        "{}/v1/build-config?platform={}",
        manager.manager_url, platform
    );

    let mut req = manager.http_client.get(&url);
    if let Some(ref token) = manager.auth_token {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    let resp = req
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to call build-config endpoint".to_string(),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "No repository name available for proxy push (build-config returned {}: {}). \
                 Use --image-repo to specify a container registry.",
                status, body
            ),
        }));
    }

    let bc: serde_json::Value = resp
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse build-config response".to_string(),
        })?;

    bc.get("repositoryName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "build-config response missing repositoryName. \
                          Use --image-repo to specify a container registry."
                    .to_string(),
            })
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

fn display_platform_name(platform: &str) -> &str {
    match platform {
        "aws" => "AWS",
        "gcp" => "Google Cloud Platform",
        "azure" => "Azure",
        "kubernetes" => "Kubernetes",
        "local" => "Local",
        other => other,
    }
}

fn format_platform_summary(platforms: &[String]) -> String {
    platforms
        .iter()
        .map(|platform| display_platform_name(platform).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate that all compute resources in the stack have remote image URIs (not local paths).
/// Used with `--prebuilt` to ensure images were already pushed externally.
fn validate_prebuilt_stack(stack: &Stack) -> Result<()> {
    for (_resource_id, resource_entry) in stack.resources() {
        if let Some(func) = resource_entry.config.downcast_ref::<Function>() {
            if let FunctionCode::Image { ref image } = func.code {
                let path = PathBuf::from(image);
                if path.exists() && path.is_dir() {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: "prebuilt".to_string(),
                        message: format!(
                            "Function '{}' has a local image path '{}'. \
                             --prebuilt requires all images to be pre-pushed remote URIs. \
                             Run `alien release` without --prebuilt first.",
                            func.id, image
                        ),
                    }));
                }
            } else {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "prebuilt".to_string(),
                    message: format!(
                        "Function '{}' has source code instead of a built image. \
                         --prebuilt requires pre-built and pre-pushed images.",
                        func.id
                    ),
                }));
            }
        } else if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
            if let ContainerCode::Image { ref image } = container.code {
                let path = PathBuf::from(image);
                if path.exists() && path.is_dir() {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: "prebuilt".to_string(),
                        message: format!(
                            "Container '{}' has a local image path '{}'. \
                             --prebuilt requires all images to be pre-pushed remote URIs. \
                             Run `alien release` without --prebuilt first.",
                            container.id, image
                        ),
                    }));
                }
            } else {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "prebuilt".to_string(),
                    message: format!(
                        "Container '{}' has source code instead of a built image. \
                         --prebuilt requires pre-built and pre-pushed images.",
                        container.id
                    ),
                }));
            }
        }
    }
    Ok(())
}

// --- Push cache ---
//
// Maps artifact directory names (which include content hashes) to previously
// pushed remote image URIs. This lets `alien release` skip pushing when the
// same build artifacts were already pushed in a prior release.

/// Push cache file name, stored at `.alien/build/{platform}/push-cache.json`.
const PUSH_CACHE_FILE: &str = "push-cache.json";

/// Load the push cache for a platform. Returns an empty map on any error.
fn load_push_cache(output_dir: &PathBuf, platform: &str) -> HashMap<String, String> {
    let cache_path = output_dir
        .join("build")
        .join(platform)
        .join(PUSH_CACHE_FILE);
    match fs::read_to_string(&cache_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Save the push cache for a platform.
fn save_push_cache(
    output_dir: &PathBuf,
    platform: &str,
    cache: &HashMap<String, String>,
) -> Result<()> {
    let cache_path = output_dir
        .join("build")
        .join(platform)
        .join(PUSH_CACHE_FILE);
    let content = serde_json::to_string_pretty(cache)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize push cache".to_string(),
        })?;
    fs::write(&cache_path, content)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: cache_path.display().to_string(),
            reason: "Failed to write push cache".to_string(),
        })?;
    Ok(())
}

/// Extract the directory name from a local image path for use as a cache key.
/// For example, `/path/to/.alien/build/aws/worker-a1b2c3d4` → `worker-a1b2c3d4`.
fn cache_key_from_path(path: &str) -> Option<String> {
    PathBuf::from(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// Apply cached push URIs to a stack, replacing local image paths with
/// previously-pushed remote URIs where the content hash matches.
/// Returns the number of cache hits.
fn apply_push_cache(stack: &mut Stack, cache: &HashMap<String, String>) -> usize {
    if cache.is_empty() {
        return 0;
    }

    let mut hits = 0;

    for (_resource_id, resource_entry) in stack.resources_mut() {
        if let Some(func) = resource_entry.config.downcast_mut::<Function>() {
            if let FunctionCode::Image { ref image } = func.code {
                if let Some(key) = cache_key_from_path(image) {
                    if let Some(cached_uri) = cache.get(&key) {
                        info!(
                            "Push cache hit for function '{}': {} → {}",
                            func.id, key, cached_uri
                        );
                        func.code = FunctionCode::Image {
                            image: cached_uri.clone(),
                        };
                        hits += 1;
                    }
                }
            }
        } else if let Some(container) = resource_entry.config.downcast_mut::<Container>() {
            if let ContainerCode::Image { ref image } = container.code {
                if let Some(key) = cache_key_from_path(image) {
                    if let Some(cached_uri) = cache.get(&key) {
                        info!(
                            "Push cache hit for container '{}': {} → {}",
                            container.id, key, cached_uri
                        );
                        container.code = ContainerCode::Image {
                            image: cached_uri.clone(),
                        };
                        hits += 1;
                    }
                }
            }
        }
    }

    hits
}

/// Collect pushed image URIs from the stack into the cache map.
/// Only collects remote URIs (not local paths).
fn collect_push_cache_entries(
    stack: &Stack,
    pre_push_stack: &Stack,
    cache: &mut HashMap<String, String>,
) {
    // Iterate both stacks together to map original local paths → pushed URIs
    let pre_push_images: HashMap<String, String> = pre_push_stack
        .resources()
        .filter_map(|(id, entry)| {
            if let Some(func) = entry.config.downcast_ref::<Function>() {
                if let FunctionCode::Image { ref image } = func.code {
                    return Some((id.clone(), image.clone()));
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                if let ContainerCode::Image { ref image } = container.code {
                    return Some((id.clone(), image.clone()));
                }
            }
            None
        })
        .collect();

    for (resource_id, resource_entry) in stack.resources() {
        let pushed_uri = if let Some(func) = resource_entry.config.downcast_ref::<Function>() {
            if let FunctionCode::Image { ref image } = func.code {
                Some(image.clone())
            } else {
                None
            }
        } else if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
            if let ContainerCode::Image { ref image } = container.code {
                Some(image.clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(uri) = pushed_uri {
            // Only cache if the URI looks like a remote URI (not a local path)
            let path = PathBuf::from(&uri);
            if path.exists() && path.is_dir() {
                continue; // Still local, wasn't pushed
            }

            // Find the original local path for this resource to use as cache key
            if let Some(original_path) = pre_push_images.get(resource_id) {
                if let Some(key) = cache_key_from_path(original_path) {
                    cache.insert(key, uri);
                }
            }
        }
    }
}

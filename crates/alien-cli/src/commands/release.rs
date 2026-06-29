use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::get_current_dir;
use crate::git_utils::collect_git_metadata;
use crate::output::print_json;
use crate::ui::{command, contextual_heading, dim_label, success_line};
use crate::{ErrorData, Result};
use alien_build::settings::PushSettings;
use alien_core::{
    alien_event, AlienEvent, Container, ContainerCode, Daemon, DaemonCode, Platform, Stack,
    StackInputDefinition, StackInputKind, StackInputProvider, Worker, WorkerCode,
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
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;
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

    # Skip the build and release the existing build output (still pushes local artifacts)
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

    /// Base cloud platform for Kubernetes auto-builds. This keeps the release
    /// stack under Kubernetes while using the managed cluster's default
    /// architecture when auto-building missing artifacts.
    #[arg(long)]
    pub base_platform: Option<String>,

    /// Skip the build and release the existing `.alien` output. Still pushes any local
    /// artifacts it contains, and reuses images that are already remote URIs.
    #[arg(long)]
    pub prebuilt: bool,

    /// Target OS/architecture combinations to build for (comma-separated).
    /// Same format as `alien build --targets` — e.g. `linux-x64`,
    /// `linux-arm64`. When omitted, the default is picked from the
    /// platform AND the stack: AWS deploys with a daemon that declares
    /// `nestedVirtualization(true)` automatically build for `linux-x64`
    /// (nested virt isn't available on Graviton); all other AWS deploys
    /// keep AWS's `linux-arm64` default.
    #[arg(long, value_delimiter = ',')]
    pub targets: Option<Vec<String>>,

    /// Override the runtime base image used for source-built cloud containers.
    #[arg(long, env = "ALIEN_OVERRIDE_BASE_IMAGE", hide = true)]
    pub override_base_image: Option<String>,

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

    match release_task_core(args, config, &ctx).await {
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
    /// Manager context — used for release creation in standalone/dev mode.
    /// In platform mode, releases are created directly on the platform API.
    manager: Option<ManagerContext>,
    workspace_name: String,
    project_link: crate::project_link::ProjectLink,
    git_metadata: Option<GitMetadata>,
    platforms: Vec<String>,
    stack: Stack,
}

#[derive(Clone)]
struct AutoBuildPlan {
    platform: String,
    settings: alien_build::settings::BuildSettings,
}

/// Load and validate all release configuration from args + execution context.
/// This is the single place where auth, workspace, project, and platform discovery happen.
async fn load_release_config(
    args: &ReleaseArgs,
    ctx: &ExecutionMode,
    allow_bootstrap: bool,
    show_human_output: bool,
) -> Result<ReleaseConfig> {
    let config_started = Instant::now();
    let current_dir = get_current_dir()?;
    let output_dir = current_dir.join(".alien");

    let workspace_name = ctx
        .resolve_workspace_with_bootstrap(allow_bootstrap)
        .await?;

    // Resolve project
    let (_project_id, project_link) = ctx
        .resolve_project(args.project.as_deref(), allow_bootstrap)
        .await?;

    let is_dev = ctx.is_dev();

    // Load stack config (needed for supported_platforms validation and auto-build)
    let stack = crate::config::load_configuration(current_dir.clone())
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to load configuration".to_string(),
        })?;

    // Determine target platforms:
    // 1. Explicit --platforms flag takes priority (validated against stack.supported_platforms)
    // 2. In dev mode without explicit platforms, default to ["local"]
    // 3. stack.supported_platforms if declared
    // 4. Otherwise, discover from build artifacts
    let target_platforms = if let Some(ref platforms) = args.platforms {
        // Validate against stack's supported platforms
        validate_platforms_against_stack(platforms, &stack)?;
        platforms.clone()
    } else if is_dev {
        // Dev mode defaults to local platform
        vec!["local".to_string()]
    } else if let Some(supported) = stack.supported_platforms() {
        // Use declared supported platforms from alien.ts
        supported.iter().map(|p| p.as_str().to_string()).collect()
    } else {
        let discovered = discover_built_platforms(&output_dir)?;
        if !discovered.is_empty() {
            discovered
        } else {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platforms".to_string(),
                message: "No platforms found. Declare .platforms() in alien.ts, run `alien build --platform <aws|gcp|azure>` first, or specify --platforms explicitly.".to_string(),
            }));
        }
    };

    let has_kubernetes_platform = target_platforms
        .iter()
        .any(|platform| platform.eq_ignore_ascii_case(Platform::Kubernetes.as_str()));
    let kubernetes_base_platform =
        parse_kubernetes_base_platform(has_kubernetes_platform, args.base_platform.as_deref())?;

    // Build for every platform unless --prebuilt is set.
    // Content-hash dedup in the build layer makes this fast when nothing changed.
    if !args.prebuilt {
        auto_build_for_platforms(
            &target_platforms,
            &stack,
            &output_dir,
            show_human_output,
            args.override_base_image.clone(),
            kubernetes_base_platform,
            args.targets.as_deref(),
        )
        .await?;
    }

    // Re-discover platforms after potential auto-build
    let platforms = if let Some(ref platforms) = args.platforms {
        platforms.clone()
    } else if is_dev {
        target_platforms.clone()
    } else if stack.supported_platforms().is_some() {
        // Stack declares its platforms — use them directly (already validated above)
        target_platforms.clone()
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

    // Resolve manager for standalone/dev mode (needed for release creation).
    // In platform mode, releases are created directly on the platform API.
    let manager = if ctx.is_standalone() || ctx.is_dev() {
        Some(
            ctx.resolve_manager(&project_link.project_id, &platforms[0])
                .await?,
        )
    } else {
        None
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

    info!(
        "Release configuration loaded in {:.2}s",
        config_started.elapsed().as_secs_f64()
    );

    Ok(ReleaseConfig {
        output_dir,
        manager,
        workspace_name,
        project_link,
        git_metadata,
        platforms,
        stack,
    })
}

/// Core release logic for console mode.
async fn release_task(args: ReleaseArgs, ctx: ExecutionMode) -> Result<ReleaseResult> {
    let is_dev = ctx.is_dev();
    let config = AlienEvent::LoadingConfiguration
        .in_scope(|_| async { load_release_config(&args, &ctx, true, true).await })
        .await?;

    let platforms_label = format_platform_summary(&config.platforms);
    let onboard_hint = onboard_command_hint(&config);
    println!(
        "{}",
        contextual_heading(
            "Releasing",
            &config.project_link.project_name,
            &[("for", &platforms_label)],
        )
    );

    let release_id = release_task_core(args, config, &ctx).await?;
    println!("{}", success_line("Release created."));
    println!("{} {}", dim_label("Release"), release_id);
    if !is_dev {
        println!("{}", dim_label("Next create a deployment link:"));
        println!("  {}", command(&onboard_hint));
        println!("{} {}", dim_label("Then"), command("alien deployments ls"));
    }
    Ok(release_id)
}

/// Core release logic shared by all output modes.
///
/// Always uses the manager SDK to create the release — no mode branching.
/// `ctx` is needed to resolve per-platform repository names in platform mode.
async fn release_task_core(
    args: ReleaseArgs,
    config: ReleaseConfig,
    ctx: &ExecutionMode,
) -> Result<ReleaseResult> {
    let release_started = Instant::now();
    let ReleaseConfig {
        output_dir,
        manager,
        project_link,
        git_metadata,
        platforms: platforms_to_release,
        stack: _stack,
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
        let platform_started = Instant::now();
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

        // Push images if needed (the test platform skips pushing). --prebuilt skips the build,
        // not the push: it reuses already-pushed artifacts via the cache and pushes any local ones.
        // Local platform pushes to a cloud registry — the alien-agent pulls from it.
        let pushed_stack = if platform != Platform::Test {
            if args.prebuilt {
                rebase_prebuilt_stack_image_paths(&mut built_stack, &output_dir)?;
            }

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

            // Get push settings per-platform — each cloud platform has its own
            // registry prefix (ECR, GAR, ACR, local Docker). In platform mode,
            // resolve_manager calls the platform API to get the per-project
            // repo name for this specific platform.
            let push_settings = if let Some(ref image_repo) = args.image_repo {
                create_manual_push_settings(&args, image_repo)?
            } else {
                let per_platform = ctx
                    .resolve_manager(&project_link.project_id, platform_str)
                    .await?;
                build_proxy_push_settings(&per_platform, &platform).await?
            };

            info!("   Pushing images to {}...", push_settings.repository);

            let push_started = Instant::now();
            let pushed = alien_build::push_stack(built_stack, platform.clone(), &push_settings)
                .await
                .context(ErrorData::ReleaseFailed {
                    message: format!("Failed to push images for {} platform", platform_str),
                })?;
            info!(
                "Push for platform '{}' completed in {:.2}s",
                platform_str,
                push_started.elapsed().as_secs_f64()
            );

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

        info!(
            "   ✓ {} platform ready in {:.2}s",
            platform_str,
            platform_started.elapsed().as_secs_f64()
        );
    }

    // Create release
    let create_release_started = Instant::now();
    let release_id = if let Some(ref manager) = manager {
        // Standalone/Dev mode: create release on the manager
        let sdk_git_metadata = git_metadata.and_then(|m| {
            m.0.map(|inner| alien_manager_api::types::GitMetadata {
                commit_sha: inner.commit_sha.map(|s| s.to_string()),
                commit_ref: inner.commit_ref.map(|s| s.to_string()),
                commit_message: inner.commit_message.map(|s| s.to_string()),
            })
        });
        create_manager_release(
            manager,
            &project_link.project_id,
            stack_by_platform,
            sdk_git_metadata,
        )
        .await?
    } else {
        // Platform mode: create release directly on the platform API
        #[cfg(feature = "platform")]
        {
            create_platform_release(
                ctx,
                &project_link.project_id,
                &project_link.workspace,
                stack_by_platform,
                git_metadata,
            )
            .await?
        }
        #[cfg(not(feature = "platform"))]
        {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Platform mode requires the 'platform' feature".to_string(),
            }));
        }
    };
    info!(
        "Release creation API call completed in {:.2}s",
        create_release_started.elapsed().as_secs_f64()
    );
    info!(
        "Release task core completed in {:.2}s",
        release_started.elapsed().as_secs_f64()
    );

    Ok(release_id)
}

/// Create a release on the manager
#[alien_event(AlienEvent::CreatingRelease {
    project: "release".to_string(),
})]
async fn create_manager_release(
    manager: &ManagerContext,
    project_id: &str,
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
            project_id: project_id.to_string(),
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

/// Create a release directly on the platform API (platform mode).
///
/// In platform mode, releases go directly to the platform API because
/// different platforms may push through different managers. The platform API
/// stores the full multi-platform release.
#[cfg(feature = "platform")]
async fn create_platform_release(
    ctx: &ExecutionMode,
    project_id: &str,
    workspace: &str,
    stack: ManagerStackByPlatform,
    git_metadata: Option<GitMetadata>,
) -> Result<String> {
    use alien_platform_api::SdkResultExt as PlatformSdkResultExt;

    info!("Creating release on platform API...");

    let http = ctx.auth_http().await?;
    let platform_client = http.sdk_client();

    // Convert manager SDK StackByPlatform to platform SDK StackByPlatform (serde roundtrip)
    let stack_json =
        serde_json::to_value(&stack)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to serialize stack".to_string(),
                url: None,
            })?;
    let platform_stack: alien_platform_api::types::StackByPlatform =
        serde_json::from_value(stack_json)
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to convert stack to platform format".to_string(),
                url: None,
            })?;

    let workspace_param = alien_platform_api::types::CreateReleaseWorkspace::try_from(workspace)
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "workspace".to_string(),
                message: format!("Invalid workspace: {}", e),
            })
        })?;

    let body = alien_platform_api::types::CreateReleaseRequest::builder()
        .project(project_id.to_string())
        .stack(platform_stack)
        .git_metadata(git_metadata);

    let body = alien_platform_api::types::CreateReleaseRequest::try_from(body).map_err(|e| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("Failed to build release request: {}", e),
            url: None,
        })
    })?;

    let response = platform_client
        .create_release()
        .workspace(&workspace_param)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create release on platform API".to_string(),
            url: None,
        })?;

    let release_id = response.id.to_string();
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
    for platform in Platform::DEPLOYABLE {
        let stack_file = build_dir.join(platform.as_str()).join("stack.json");
        if stack_file.exists() {
            platforms.push(platform.as_str().to_string());
        }
    }

    Ok(platforms)
}

/// True if any ComputeCluster in this stack has a capacity group with
/// `nestedVirtualization: true`. On AWS this implies x86_64 — nested
/// virtualization is not available on Graviton, so a build targeting
/// `linux-arm64` (the AWS default) for such a stack would produce an image
/// the deploy can't actually run.
fn stack_requires_x86_64_on_aws(stack: &Stack) -> bool {
    use alien_core::ComputeCluster;
    stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<ComputeCluster>()
            .is_some_and(|cluster| {
                cluster
                    .capacity_groups
                    .iter()
                    .any(|g| g.nested_virtualization == Some(true))
            })
    })
}

async fn auto_build_for_platforms(
    platform_strs: &[String],
    stack: &Stack,
    output_dir: &PathBuf,
    _show_human_output: bool,
    override_base_image: Option<String>,
    kubernetes_base_platform: Option<Platform>,
    user_targets: Option<&[String]>,
) -> Result<()> {
    let stack_needs_x86_64_on_aws = stack_requires_x86_64_on_aws(stack);

    let mut plans = Vec::new();
    for platform_str in platform_strs {
        // Resolve targets in priority order:
        //   1. Explicit --targets always wins
        //   2. AWS + nested-virt in the stack → linux-x64 (the only AWS
        //      target that supports nested virt today)
        //   3. None → let alien-build pick the platform default
        let effective_targets = if let Some(targets) = user_targets {
            Some(targets.to_vec())
        } else if platform_str.eq_ignore_ascii_case("aws") && stack_needs_x86_64_on_aws {
            tracing::info!(
                "AWS daemon requires nested virtualization; defaulting target to linux-x64 \
                 (override with `alien release --targets <...>`)."
            );
            Some(vec!["linux-x64".to_string()])
        } else {
            None
        };

        plans.push(AutoBuildPlan {
            platform: platform_str.clone(),
            settings: auto_build_settings_for_platform(
                platform_str,
                output_dir,
                override_base_image.clone(),
                kubernetes_base_platform,
                effective_targets,
            )?,
        });
    }

    let groups = group_auto_builds(plans);
    if groups.len() > 1 {
        info!(
            "Auto-building {} independent target group(s) in parallel",
            groups.len()
        );
    }

    let group_futures = groups.into_iter().map(|group| {
        let stack = stack.clone();
        async move {
            for plan in group {
                let platform_build_started = Instant::now();
                alien_build::build_stack(stack.clone(), &plan.settings)
                    .await
                    .context(ErrorData::BuildFailed)?;
                info!(
                    "Auto-build for platform '{}' completed in {:.2}s",
                    plan.platform,
                    platform_build_started.elapsed().as_secs_f64()
                );
            }
            Ok::<_, AlienError<ErrorData>>(())
        }
    });

    futures::future::try_join_all(group_futures).await?;
    Ok(())
}

fn auto_build_settings_for_platform(
    platform_str: &str,
    output_dir: &PathBuf,
    override_base_image: Option<String>,
    kubernetes_base_platform: Option<Platform>,
    effective_targets: Option<Vec<String>>,
) -> Result<alien_build::settings::BuildSettings> {
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
        Platform::Kubernetes => alien_build::settings::PlatformBuildSettings::Kubernetes {
            base_platform: kubernetes_base_platform,
        },
        Platform::Local => alien_build::settings::PlatformBuildSettings::Local {},
        Platform::Test => alien_build::settings::PlatformBuildSettings::Test {},
    };

    // Resolve targets in priority order:
    //   1. The caller (release_task) passed an effective_targets list (either
    //      --targets verbatim or a stack-aware default like x86_64-for-nested-virt).
    //   2. Local releases target Linux install hosts. Same-host development
    //      builds can still use `alien build --platform local` defaults.
    //   3. Otherwise pass None and let alien-build apply its platform default.
    let targets = if let Some(targets) = effective_targets {
        Some(
            targets
                .iter()
                .map(|t| parse_target(t))
                .collect::<Result<Vec<_>>>()?,
        )
    } else if matches!(platform, Platform::Local) {
        Some(alien_core::BinaryTarget::LINUX.to_vec())
    } else {
        None
    };

    Ok(alien_build::settings::BuildSettings {
        output_directory: output_dir.to_str().unwrap().to_string(),
        platform: platform_build_settings,
        targets,
        cache_url: None,
        override_base_image,
        debug_mode: false,
    })
}

fn parse_target(target_str: &str) -> Result<alien_core::BinaryTarget> {
    use alien_core::BinaryTarget;
    match target_str.to_ascii_lowercase().as_str() {
        "windows-x64" => Ok(BinaryTarget::WindowsX64),
        "linux-x64" => Ok(BinaryTarget::LinuxX64),
        "linux-arm64" => Ok(BinaryTarget::LinuxArm64),
        "darwin-arm64" => Ok(BinaryTarget::DarwinArm64),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "targets".to_string(),
            message: format!(
                "Unknown target '{target_str}'. Supported targets: \
                 windows-x64, linux-x64, linux-arm64, darwin-arm64"
            ),
        })),
    }
}

fn group_auto_builds(plans: Vec<AutoBuildPlan>) -> Vec<Vec<AutoBuildPlan>> {
    let mut groups: Vec<(String, Vec<AutoBuildPlan>)> = Vec::new();

    for plan in plans {
        let key = auto_build_target_group_key(&plan.settings);
        if let Some((_, group)) = groups.iter_mut().find(|(group_key, _)| *group_key == key) {
            group.push(plan);
        } else {
            groups.push((key, vec![plan]));
        }
    }

    groups.into_iter().map(|(_, group)| group).collect()
}

fn auto_build_target_group_key(settings: &alien_build::settings::BuildSettings) -> String {
    settings
        .get_targets()
        .iter()
        .map(|target| target.runtime_platform_id())
        .collect::<Vec<_>>()
        .join(",")
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
        destination_label: Some(format!("custom registry {}", image_repo)),
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
        // Standalone mode: call the manager's build-config endpoint directly
        // to discover the repository name for this platform.
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

        let bc: serde_json::Value =
            resp.json()
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
            })?
    };

    // Strip scheme to get the registry host (OCI clients use host:port, not URLs).
    let registry_host = alien_core::image_rewrite::strip_url_scheme(manager_url);

    // Translate host.docker.internal → localhost for CLI access (dev mode).
    let (registry_host, protocol) =
        translate_registry_url_for_cli(&registry_host, &Platform::Local)?;

    // Full repository: host/repo_name (e.g., "manager.alien.dev/alien-e2e")
    let repository = format!("{}/{}", registry_host, repo_name);

    // OCI speaks Basic — the token rides in the password slot, the
    // workspace rides in the username slot. OCI clients can't add custom
    // headers, and the username slot is exactly where cloud registries
    // pass tenant/identity info (GCR uses `oauth2accesstoken`, ECR uses
    // `AWS`). Without a workspace, the username stays as the existing
    // "token" placeholder.
    let auth = match (&manager.auth_token, &manager.workspace) {
        (Some(token), Some(workspace)) => RegistryAuth::Basic(workspace.clone(), token.clone()),
        (Some(token), None) => RegistryAuth::Basic("token".to_string(), token.clone()),
        (None, _) => RegistryAuth::Anonymous,
    };

    info!("   Pushing through manager proxy at {}", repository);

    Ok(PushSettings {
        repository,
        destination_label: Some(manager_push_destination_label(manager)),
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

fn manager_push_destination_label(manager: &ManagerContext) -> String {
    match (
        manager.manager_name.as_deref(),
        manager.manager_is_system,
        manager.manager_cloud.as_deref(),
    ) {
        (Some(name), Some(true), _) => format!("{name} (Alien-hosted)"),
        (Some(name), Some(false), Some(cloud)) => format!("{name} private manager ({cloud})"),
        (Some(name), Some(false), None) => format!("{name} private manager"),
        (Some(name), _, _) => name.to_string(),
        (None, _, _) => manager.manager_url.clone(),
    }
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

fn onboard_command_hint(config: &ReleaseConfig) -> String {
    let selected_platforms = config
        .platforms
        .iter()
        .filter_map(|platform| Platform::from_str(platform).ok())
        .collect::<Vec<_>>();
    let mut command_parts = vec!["alien onboard <customer-name>".to_string()];

    if !config.platforms.is_empty() {
        command_parts.push(format!("--platforms {}", config.platforms.join(",")));
    }

    let required_inputs = config
        .stack
        .inputs
        .iter()
        .filter(|input| input.required)
        .filter(|input| input.provided_by.contains(&StackInputProvider::Developer))
        .filter(|input| input_applies_to_any_platform(input, &selected_platforms))
        .collect::<Vec<_>>();

    for input in required_inputs.iter().take(3) {
        command_parts.push(format!(
            "{} {}={}",
            if matches!(input.kind, StackInputKind::Secret) {
                "--secret-input"
            } else {
                "--input"
            },
            input.id,
            stack_input_placeholder(input),
        ));
    }

    if required_inputs.len() > 3 {
        command_parts.push("...".to_string());
    }

    command_parts.join(" ")
}

fn input_applies_to_any_platform(input: &StackInputDefinition, platforms: &[Platform]) -> bool {
    let Some(input_platforms) = &input.platforms else {
        return true;
    };
    platforms
        .iter()
        .any(|platform| input_platforms.contains(platform))
}

fn stack_input_placeholder(input: &StackInputDefinition) -> &'static str {
    if matches!(input.kind, StackInputKind::Boolean) {
        "true"
    } else {
        "..."
    }
}

fn format_platform_summary(platforms: &[String]) -> String {
    platforms
        .iter()
        .map(|platform| display_platform_name(platform).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validate that all requested platforms are supported by the stack.
/// Returns Ok(()) if stack has no supported_platforms (all allowed) or all platforms are in the list.
fn validate_platforms_against_stack(platforms: &[String], stack: &Stack) -> Result<()> {
    let supported = match stack.supported_platforms() {
        Some(s) => s,
        None => return Ok(()),
    };

    for p in platforms {
        if let Ok(platform) = Platform::from_str(p) {
            if !supported.contains(&platform) {
                let supported_list: Vec<&str> = supported.iter().map(|p| p.as_str()).collect();
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platforms".to_string(),
                    message: format!(
                        "Platform '{}' is not supported by this stack. Declared platforms: [{}]",
                        p,
                        supported_list.join(", ")
                    ),
                }));
            }
        }
    }

    Ok(())
}

fn parse_kubernetes_base_platform(
    has_kubernetes_platform: bool,
    base_platform: Option<&str>,
) -> Result<Option<Platform>> {
    let Some(base_platform) = base_platform else {
        return Ok(None);
    };

    let parsed = Platform::from_str(base_platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: e,
        })
    })?;

    if !has_kubernetes_platform {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: "--base-platform is only supported when releasing --platforms kubernetes"
                .to_string(),
        }));
    }

    match parsed {
        Platform::Aws | Platform::Gcp | Platform::Azure => Ok(Some(parsed)),
        Platform::Kubernetes | Platform::Local | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "base-platform".to_string(),
                message: "--base-platform must be one of: aws, gcp, azure".to_string(),
            }))
        }
    }
}

/// Rebase copied prebuilt artifact paths to the current checkout.
///
/// `alien build` writes local artifact directories into stack.json. CI may build
/// those artifacts in one checkout and copy `.alien/build` into another image,
/// so a prebuilt release must resolve stale `.alien/build/{platform}/{artifact}`
/// paths before pushing. The artifact path may point at a different platform
/// than the release currently being pushed when platforms share a built image.
fn rebase_prebuilt_stack_image_paths(stack: &mut Stack, output_dir: &Path) -> Result<()> {
    for (_resource_id, resource_entry) in stack.resources_mut() {
        if let Some(func) = resource_entry.config.downcast_ref::<Worker>() {
            match &func.code {
                WorkerCode::Image { image } => {
                    if let Some(rebased) =
                        rebase_prebuilt_image_path("worker", &func.id, image, output_dir)?
                    {
                        let mut updated = func.clone();
                        updated.code = WorkerCode::Image { image: rebased };
                        resource_entry.config = alien_core::Resource::new(updated);
                    }
                }
                WorkerCode::Source { .. } => {
                    return Err(prebuilt_source_error("Worker", &func.id));
                }
            }
        } else if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => {
                    if let Some(rebased) =
                        rebase_prebuilt_image_path("container", &container.id, image, output_dir)?
                    {
                        let mut updated = container.clone();
                        updated.code = ContainerCode::Image { image: rebased };
                        resource_entry.config = alien_core::Resource::new(updated);
                    }
                }
                ContainerCode::Source { .. } => {
                    return Err(prebuilt_source_error("Container", &container.id));
                }
            }
        } else if let Some(daemon) = resource_entry.config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Image { image } => {
                    if let Some(rebased) =
                        rebase_prebuilt_image_path("daemon", &daemon.id, image, output_dir)?
                    {
                        let mut updated = daemon.clone();
                        updated.code = DaemonCode::Image { image: rebased };
                        resource_entry.config = alien_core::Resource::new(updated);
                    }
                }
                DaemonCode::Source { .. } => {
                    return Err(prebuilt_source_error("Daemon", &daemon.id));
                }
            }
        }
    }
    Ok(())
}

fn rebase_prebuilt_image_path(
    resource_type: &str,
    resource_id: &str,
    image: &str,
    output_dir: &Path,
) -> Result<Option<String>> {
    let image_path = PathBuf::from(image);
    if image_path.exists() {
        return Ok(None);
    }

    if let Some((artifact_platform, artifact_dir)) = artifact_location_from_build_path(&image_path)
    {
        let rebased_path = output_dir
            .join("build")
            .join(&artifact_platform)
            .join(&artifact_dir);
        if rebased_path.exists() && rebased_path.is_dir() {
            info!(
                "Rebased prebuilt {} '{}' image artifact from '{}' to '{}'",
                resource_type,
                resource_id,
                image,
                rebased_path.display()
            );
            return Ok(Some(rebased_path.to_string_lossy().into_owned()));
        }

        return Err(AlienError::new(ErrorData::ValidationError {
            field: "prebuilt".to_string(),
            message: format!(
                "{} '{}' references prebuilt artifact '{}', but '{}' does not exist. \
                 Rebuild the prebuilt artifacts or run without --prebuilt.",
                resource_type,
                resource_id,
                image,
                rebased_path.display()
            ),
        }));
    }

    if image_path.is_absolute() || image.starts_with("./") || image.starts_with("../") {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "prebuilt".to_string(),
            message: format!(
                "{} '{}' references local image path '{}', but it does not exist. \
                 Rebuild the prebuilt artifacts or run without --prebuilt.",
                resource_type, resource_id, image
            ),
        }));
    }

    Ok(None)
}

fn artifact_location_from_build_path(path: &Path) -> Option<(String, String)> {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();

    components.windows(4).find_map(|window| {
        if window[0] == ".alien" && window[1] == "build" {
            Some((window[2].to_string(), window[3].to_string()))
        } else {
            None
        }
    })
}

fn prebuilt_source_error(resource_type: &str, resource_id: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: "prebuilt".to_string(),
        message: format!(
            "{} '{}' has source code instead of a built image. \
             --prebuilt requires .alien/build artifacts.",
            resource_type, resource_id
        ),
    })
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
        if let Some(func) = resource_entry.config.downcast_mut::<Worker>() {
            if let WorkerCode::Image { ref image } = func.code {
                if let Some(key) = cache_key_from_path(image) {
                    if let Some(cached_uri) = cache.get(&key) {
                        info!(
                            "Push cache hit for function '{}': {} → {}",
                            func.id, key, cached_uri
                        );
                        func.code = WorkerCode::Image {
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
        } else if let Some(daemon) = resource_entry.config.downcast_mut::<Daemon>() {
            if let DaemonCode::Image { ref image } = daemon.code {
                if let Some(key) = cache_key_from_path(image) {
                    if let Some(cached_uri) = cache.get(&key) {
                        info!(
                            "Push cache hit for daemon '{}': {} → {}",
                            daemon.id, key, cached_uri
                        );
                        daemon.code = DaemonCode::Image {
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
            if let Some(func) = entry.config.downcast_ref::<Worker>() {
                if let WorkerCode::Image { ref image } = func.code {
                    return Some((id.clone(), image.clone()));
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                if let ContainerCode::Image { ref image } = container.code {
                    return Some((id.clone(), image.clone()));
                }
            }
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                if let DaemonCode::Image { ref image } = daemon.code {
                    return Some((id.clone(), image.clone()));
                }
            }
            None
        })
        .collect();

    for (resource_id, resource_entry) in stack.resources() {
        let pushed_uri = if let Some(func) = resource_entry.config.downcast_ref::<Worker>() {
            if let WorkerCode::Image { ref image } = func.code {
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
        } else if let Some(daemon) = resource_entry.config.downcast_ref::<Daemon>() {
            if let DaemonCode::Image { ref image } = daemon.code {
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::ResourceLifecycle;

    fn daemon_with_image(image: &str) -> Daemon {
        Daemon::new("agent".to_string())
            .permissions("execution".to_string())
            .code(DaemonCode::Image {
                image: image.to_string(),
            })
            .build()
    }

    #[test]
    fn push_cache_applies_and_collects_for_daemons() {
        let local_dir = tempfile::tempdir().unwrap();
        let artifact_dir = local_dir.path().join("agent-a1b2c3d4");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let local_path = artifact_dir.to_string_lossy().into_owned();

        // apply: a cached URI for the artifact dir's key replaces the daemon's local path.
        let mut stack = Stack::new("cache-test".to_string())
            .add(daemon_with_image(&local_path), ResourceLifecycle::Live)
            .build();
        let cache = HashMap::from([(
            "agent-a1b2c3d4".to_string(),
            "registry.example.com/agent:tag".to_string(),
        )]);
        let hits = apply_push_cache(&mut stack, &cache);
        assert_eq!(hits, 1, "daemon local path should hit the cache");
        let daemon = stack
            .resources()
            .find_map(|(_, e)| e.config.downcast_ref::<Daemon>().cloned())
            .expect("daemon should exist");
        assert_eq!(
            daemon.code,
            DaemonCode::Image {
                image: "registry.example.com/agent:tag".to_string()
            }
        );

        // collect: pushed daemon URI lands in the cache keyed by the original dir name.
        let pre_push = Stack::new("cache-test".to_string())
            .add(daemon_with_image(&local_path), ResourceLifecycle::Live)
            .build();
        let pushed = Stack::new("cache-test".to_string())
            .add(
                daemon_with_image("registry.example.com/agent:pushed"),
                ResourceLifecycle::Live,
            )
            .build();
        let mut collected = HashMap::new();
        collect_push_cache_entries(&pushed, &pre_push, &mut collected);
        assert_eq!(
            collected.get("agent-a1b2c3d4").map(String::as_str),
            Some("registry.example.com/agent:pushed")
        );
    }

    #[test]
    fn stack_with_no_daemons_does_not_require_x86_64() {
        let stack = Stack::new("nothing".to_string()).build();
        assert!(!stack_requires_x86_64_on_aws(&stack));
    }

    #[test]
    fn stack_with_cluster_no_nested_virt_does_not_require_x86_64() {
        use alien_core::{CapacityGroup, ComputeCluster};
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 1,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();
        let stack = Stack::new("no-nested".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .add(
                daemon_with_image("ghcr.io/test/agent:1"),
                ResourceLifecycle::Live,
            )
            .build();
        assert!(!stack_requires_x86_64_on_aws(&stack));
    }

    #[test]
    fn stack_with_cluster_capacity_group_nested_virt_requires_x86_64() {
        use alien_core::{CapacityGroup, ComputeCluster};
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m8i.xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 1,
                scale_policy: None,
                nested_virtualization: Some(true),
            })
            .build();
        let stack = Stack::new("nested".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .add(
                daemon_with_image("ghcr.io/test/agent:1"),
                ResourceLifecycle::Live,
            )
            .build();
        assert!(stack_requires_x86_64_on_aws(&stack));
    }

    #[test]
    fn parse_kubernetes_base_platform_accepts_clouds_for_kubernetes_releases() {
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("aws")).unwrap(),
            Some(Platform::Aws)
        );
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("gcp")).unwrap(),
            Some(Platform::Gcp)
        );
        assert_eq!(
            parse_kubernetes_base_platform(true, Some("azure")).unwrap(),
            Some(Platform::Azure)
        );
    }

    #[test]
    fn parse_kubernetes_base_platform_rejects_releases_without_kubernetes() {
        assert!(parse_kubernetes_base_platform(false, Some("aws")).is_err());
    }

    #[test]
    fn parse_kubernetes_base_platform_rejects_non_cloud_platforms() {
        assert!(parse_kubernetes_base_platform(true, Some("kubernetes")).is_err());
        assert!(parse_kubernetes_base_platform(true, Some("local")).is_err());
    }

    #[test]
    fn group_auto_builds_keeps_equivalent_targets_in_order() {
        let plans = vec![
            test_plan(
                "aws",
                alien_build::settings::PlatformBuildSettings::Aws {
                    managing_account_id: None,
                },
            ),
            test_plan("gcp", alien_build::settings::PlatformBuildSettings::Gcp {}),
            test_plan(
                "azure",
                alien_build::settings::PlatformBuildSettings::Azure {},
            ),
        ];

        let groups = group_auto_builds(plans);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0][0].platform, "aws");
        assert_eq!(
            groups[1]
                .iter()
                .map(|plan| plan.platform.as_str())
                .collect::<Vec<_>>(),
            vec!["gcp", "azure"]
        );
    }

    #[test]
    fn local_release_auto_build_defaults_to_linux_targets() {
        let temp = tempfile::tempdir().unwrap();
        let settings = auto_build_settings_for_platform(
            "local",
            &temp.path().join(".alien"),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            settings.targets,
            Some(alien_core::BinaryTarget::LINUX.to_vec())
        );
    }

    #[test]
    fn rebase_prebuilt_stack_image_paths_rewrites_copied_artifact_path() {
        let temp = tempfile::tempdir().unwrap();
        let output_dir = temp.path().join(".alien");
        let artifact_dir = output_dir.join("build").join("gcp").join("writer-12345678");
        std::fs::create_dir_all(&artifact_dir).unwrap();

        let original_image = "/tmp/original-checkout/.alien/build/gcp/writer-12345678";
        let mut stack = Stack::new("demo".to_string())
            .add(
                test_container("writer", original_image.to_string()),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        rebase_prebuilt_stack_image_paths(&mut stack, &output_dir).unwrap();

        let (_, entry) = stack.resources().next().unwrap();
        let container = entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(
            container_image(container),
            artifact_dir.to_string_lossy().as_ref()
        );
    }

    #[test]
    fn rebase_prebuilt_stack_image_paths_rewrites_cross_platform_artifact_path() {
        let temp = tempfile::tempdir().unwrap();
        let output_dir = temp.path().join(".alien");
        let artifact_dir = output_dir.join("build").join("aws").join("web-12345678");
        std::fs::create_dir_all(&artifact_dir).unwrap();

        let original_image = "/tmp/original-checkout/.alien/build/aws/web-12345678";
        let mut stack = Stack::new("demo".to_string())
            .add(
                test_container("web", original_image.to_string()),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        rebase_prebuilt_stack_image_paths(&mut stack, &output_dir).unwrap();

        let (_, entry) = stack.resources().next().unwrap();
        let container = entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(
            container_image(container),
            artifact_dir.to_string_lossy().as_ref()
        );
    }

    #[test]
    fn rebase_prebuilt_stack_image_paths_keeps_remote_image_refs() {
        let temp = tempfile::tempdir().unwrap();
        let output_dir = temp.path().join(".alien");
        let image = "registry.example.com/demo/writer:latest";
        let mut stack = Stack::new("demo".to_string())
            .add(
                test_container("writer", image.to_string()),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        rebase_prebuilt_stack_image_paths(&mut stack, &output_dir).unwrap();

        let (_, entry) = stack.resources().next().unwrap();
        let container = entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(container_image(container), image);
    }

    fn test_plan(
        platform: &str,
        platform_settings: alien_build::settings::PlatformBuildSettings,
    ) -> AutoBuildPlan {
        AutoBuildPlan {
            platform: platform.to_string(),
            settings: alien_build::settings::BuildSettings {
                output_directory: ".alien".to_string(),
                platform: platform_settings,
                targets: None,
                cache_url: None,
                override_base_image: None,
                debug_mode: false,
            },
        }
    }

    fn test_container(name: &str, image: String) -> Container {
        Container::new(name.to_string())
            .code(ContainerCode::Image { image })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("execution".to_string())
            .build()
    }

    fn container_image(container: &Container) -> &str {
        match &container.code {
            ContainerCode::Image { image } => image,
            ContainerCode::Source { .. } => panic!("expected image container"),
        }
    }
}

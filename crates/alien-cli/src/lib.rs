//! Alien CLI library
//!
//! This exposes TUI components and other utilities for use by binaries.

pub mod auth;
pub mod commands;
pub mod config;
pub mod deployment_tracking;
pub mod error;
pub mod execution_context;
pub mod git_utils;
pub mod project_link;
pub mod tui;

#[cfg(test)]
pub mod test_utils;

use crate::commands::{
    build_and_post_release_simple, build_command, create_initial_deployment, deploy_task,
    deployments_task, destroy_task, ensure_server_running_with_env, link_task, login_task,
    logout_task, onboard_task, project_task, release_command, unlink_task, vault_task, whoami_task,
    workspace_task, BuildArgs, CliEnvVar, DeployArgs, DeploymentsArgs, DestroyArgs, LinkArgs,
    LoginArgs, LogoutArgs, OnboardArgs, ProjectArgs, ReleaseArgs, UnlinkArgs, WhoamiArgs,
    WorkspaceArgs,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::tui::state::BuildState;
use crate::tui::{run_app, AppConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use std::env;
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// CLI argument structure
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Run as if alien was started in <path> instead of the current working directory
    #[arg(short = 'C', long, name = "path")]
    pub dir: Option<String>,

    /// Disable TUI and use console output instead
    #[arg(long, global = true)]
    pub no_tui: bool,

    /// Project to manage (defaults to linked project or prompts)
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Platform base URL (defaults to https://api.alien.dev)
    #[arg(long, env = "ALIEN_BASE_URL", global = true)]
    pub base_url: Option<String>,

    /// Platform API key
    #[arg(long, env = "ALIEN_API_KEY", global = true)]
    pub api_key: Option<String>,

    /// Don't open browser for authentication
    #[arg(long, global = true)]
    pub no_browser: bool,

    /// Workspace name
    #[arg(long, env = "ALIEN_WORKSPACE", global = true)]
    pub workspace: Option<String>,
}

/// CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Build the Alien application
    Build(BuildArgs),

    /// Local development mode
    Dev(DevCommand),

    /// Perform login & select default workspace
    Login(LoginArgs),
    /// Remove saved tokens & workspace
    Logout(LogoutArgs),
    /// Workspace commands
    #[command(alias = "workspace")]
    Workspaces(WorkspaceArgs),
    /// Project commands
    #[command(alias = "project")]
    Projects(ProjectArgs),
    /// Create a release from built platforms
    Release(ReleaseArgs),
    /// Link directory to an Alien project
    Link(LinkArgs),
    /// Unlink directory from an Alien project
    Unlink(UnlinkArgs),
    /// Show current authenticated user information
    Whoami(WhoamiArgs),
    /// Deployment commands
    #[command(alias = "deployment")]
    Deployments(DeploymentsArgs),
    /// Create a deployment group and generate a deployment link
    Onboard(OnboardArgs),
    /// Deploy to a cloud platform
    Deploy(DeployArgs),
    /// Destroy resources from a deployment
    Destroy(DestroyArgs),
}

/// Dev command with optional subcommands
#[derive(Parser, Debug, Clone)]
pub struct DevCommand {
    /// Dev server port
    #[arg(long, default_value = "9090", global = true)]
    pub port: u16,

    /// Target platform (local, aws, gcp, azure, kubernetes)
    #[arg(long, default_value = "local")]
    pub platform: String,

    /// Path to configuration file (default: auto-discover alien.config.ts in current directory)
    #[arg(long, short = 'c')]
    pub config: Option<PathBuf>,

    /// Skip the build step (use existing build artifacts)
    #[arg(long)]
    pub skip_build: bool,

    /// Path to write status file (JSON with DevStatus type)
    /// The status file includes API URL, resource URLs, and deployment status
    /// Type definition: alien_core::DevStatus (auto-generated to @alienplatform/core)
    #[arg(long)]
    pub status_file: Option<PathBuf>,

    /// Deployment name for the initial deployment (default: "default")
    /// Useful when running multiple alien dev instances to avoid conflicts
    #[arg(long, default_value = "default")]
    pub deployment_name: String,

    /// Plain environment variables (KEY=VALUE or KEY=VALUE:target1,target2)
    /// Can be used multiple times. Without targets, applies to all resources.
    /// Example: --env LOG_LEVEL=debug --env API_KEY=test:api-handler,worker
    #[arg(long = "env")]
    pub env_vars: Vec<String>,

    /// Secret environment variables (KEY=VALUE or KEY=VALUE:target1,target2)
    /// Secrets are loaded from vault at function startup (in production).
    /// In dev mode, they're injected directly like plain vars but marked as secrets.
    /// Example: --secret DATABASE_PASSWORD=secret123 --secret API_KEY=key:processor
    #[arg(long = "secret")]
    pub secret_vars: Vec<String>,

    #[command(subcommand)]
    pub subcommand: Option<DevSubcommand>,
}

/// Dev subcommands - either run dev mode versions of platform commands or dev-specific commands
#[derive(Subcommand, Debug, Clone)]
pub enum DevSubcommand {
    /// Start dev server only (no TUI)
    Server,

    /// Deployment commands (dev mode)
    #[command(alias = "deployment")]
    Deployments(DeploymentsArgs),

    /// Show dev whoami
    Whoami(WhoamiArgs),

    /// Deploy to dev server
    Deploy(DeployArgs),

    /// Destroy from dev server
    Destroy(DestroyArgs),

    /// Create release on dev server
    Release(ReleaseArgs),

    /// Manage vault secrets for local dev deployments
    Vault(commands::VaultArgs),
}

/// Get the current working directory as a CLI result
pub fn get_current_dir() -> Result<std::path::PathBuf> {
    std::env::current_dir()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "get current directory".to_string(),
            file_path: ".".to_string(),
            reason: "Failed to get current directory".to_string(),
        })
}

/// Setup tracing for the CLI
///
/// ## Environment Variables
///
/// - `ALIEN_LOG`: Set log level filter (e.g., "debug", "alien_cli=debug,alien_manager=trace")
///   Falls back to `RUST_LOG` if not set.
///
/// - `ALIEN_LOG_FILE`: Enable file logging. Set to a file path to write logs.
///   In TUI mode, this is the only way to see logs without breaking the UI.
///   Example: `ALIEN_LOG_FILE=/tmp/alien.log`
///
/// ## Behavior
///
/// - **No TUI mode (`--no-tui`)**: Logs to stderr by default, optionally to file if `ALIEN_LOG_FILE` is set.
/// - **TUI mode**: Console output is disabled to prevent breaking the TUI. Set `ALIEN_LOG_FILE` to debug.
pub fn setup_tracing(no_tui: bool) {
    // Get log level from ALIEN_LOG, falling back to RUST_LOG
    let env_filter = std::env::var("ALIEN_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|filter| EnvFilter::try_new(&filter).ok())
        .unwrap_or_else(|| {
            if no_tui {
                // Default info level for non-TUI mode
                EnvFilter::new("alien_cli=info,alien_core=info,alien_infra=info,alien_build=info,alien_manager=info")
            } else {
                // Default off for TUI mode unless file logging is enabled
                let has_file_log = std::env::var("ALIEN_LOG_FILE").is_ok();
                if has_file_log {
                    EnvFilter::new("alien_cli=debug,alien_core=debug,alien_infra=debug,alien_build=debug,alien_manager=debug")
                } else {
                    EnvFilter::new("off")
                }
            }
        });

    // Check for file logging
    let file_path = std::env::var("ALIEN_LOG_FILE").ok();

    // Initialize OTLP if configured (for embedded alien-runtime functions)
    // Dev server sets OTEL_EXPORTER_OTLP_LOGS_ENDPOINT when it starts
    #[cfg(feature = "otlp")]
    let otlp_layer = alien_runtime::init_otlp_logging().ok().flatten();

    #[cfg(not(feature = "otlp"))]
    let _otlp_layer: Option<()> = None;

    let registry = tracing_subscriber::registry().with(env_filter);

    match (no_tui, file_path) {
        // Non-TUI mode with file logging: write to both stderr and file
        (true, Some(path)) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect(&format!("Failed to open log file: {}", path));

            let layers = registry
                .with(fmt::layer().with_ansi(true)) // stderr
                .with(fmt::layer().with_ansi(false).with_writer(file)); // file

            #[cfg(feature = "otlp")]
            if let Some(otlp) = otlp_layer {
                layers.with(otlp).init();
            } else {
                layers.init();
            }

            #[cfg(not(feature = "otlp"))]
            layers.init();
        }
        // Non-TUI mode without file logging: write to stderr only
        (true, None) => {
            let layers = registry.with(fmt::layer().with_ansi(true));

            #[cfg(feature = "otlp")]
            if let Some(otlp) = otlp_layer {
                layers.with(otlp).init();
            } else {
                layers.init();
            }

            #[cfg(not(feature = "otlp"))]
            layers.init();
        }
        // TUI mode with file logging: write to file only (no stderr to avoid breaking TUI)
        (false, Some(path)) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect(&format!("Failed to open log file: {}", path));

            let layers = registry.with(fmt::layer().with_ansi(false).with_writer(file));

            #[cfg(feature = "otlp")]
            if let Some(otlp) = otlp_layer {
                layers.with(otlp).init();
            } else {
                layers.init();
            }

            #[cfg(not(feature = "otlp"))]
            layers.init();
        }
        // TUI mode without file logging: no output (but OTLP if configured)
        (false, None) => {
            let layers = registry.with(fmt::layer().with_writer(std::io::sink));

            #[cfg(feature = "otlp")]
            if let Some(otlp) = otlp_layer {
                layers.with(otlp).init();
            } else {
                layers.init();
            }

            #[cfg(not(feature = "otlp"))]
            layers.init();
        }
    }
}

/// Handle dev command and its subcommands
/// Parse --env and --secret flags from CLI
/// Format: KEY=VALUE or KEY=VALUE:target1,target2
fn parse_env_and_secret_vars(
    env_vars: &[String],
    secret_vars: &[String],
) -> Result<Vec<CliEnvVar>> {
    let mut parsed = Vec::new();

    // Parse plain env vars
    for env in env_vars {
        parsed.push(parse_single_env_var(env, false)?);
    }

    // Parse secret vars
    for secret in secret_vars {
        parsed.push(parse_single_env_var(secret, true)?);
    }

    Ok(parsed)
}

/// Parse a single environment variable
/// Format: KEY=VALUE or KEY=VALUE:target1,target2
fn parse_single_env_var(input: &str, is_secret: bool) -> Result<CliEnvVar> {
    // First split on '=' to get KEY and VALUE_WITH_TARGETS
    let parts: Vec<&str> = input.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Invalid {} format: '{}'. Expected KEY=VALUE or KEY=VALUE:target1,target2",
                if is_secret { "--secret" } else { "--env" },
                input
            ),
        }));
    }

    let name = parts[0].to_string();
    let value_with_targets = parts[1];

    // Check if there are targets after the value (VALUE:target1,target2)
    // Use rfind to find the LAST colon, since targets come at the end.
    // This prevents URLs like "http://example.com:8080" from being incorrectly split.
    let (value, target_resources) = if let Some(colon_pos) = value_with_targets.rfind(':') {
        let potential_value = &value_with_targets[..colon_pos];
        let potential_targets_str = &value_with_targets[colon_pos + 1..];

        // Check if what comes after the colon looks like targets (comma-separated identifiers)
        // If it looks like a port number or URL path, treat the whole thing as a value
        let looks_like_targets = !potential_targets_str.is_empty()
            && !potential_targets_str.chars().all(|c| c.is_ascii_digit())  // Not just a port number
            && !potential_targets_str.starts_with('/'); // Not a URL path

        if looks_like_targets {
            let targets: Vec<String> = potential_targets_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if targets.is_empty() {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Invalid {} format: '{}'. Targets list is empty after ':'",
                        if is_secret { "--secret" } else { "--env" },
                        input
                    ),
                }));
            }

            (potential_value.to_string(), Some(targets))
        } else {
            // The colon is part of the value (like a URL or port), not a target separator
            (value_with_targets.to_string(), None)
        }
    } else {
        (value_with_targets.to_string(), None)
    };

    Ok(CliEnvVar {
        name,
        value,
        is_secret,
        target_resources,
    })
}

/// Convert CLI env vars to core EnvironmentVariable format for deployment creation.
fn cli_env_vars_to_core(cli_vars: &[CliEnvVar]) -> Option<Vec<alien_core::EnvironmentVariable>> {
    if cli_vars.is_empty() {
        return None;
    }
    Some(
        cli_vars
            .iter()
            .map(|v| alien_core::EnvironmentVariable {
                name: v.name.clone(),
                value: v.value.clone(),
                var_type: if v.is_secret {
                    alien_core::EnvironmentVariableType::Secret
                } else {
                    alien_core::EnvironmentVariableType::Plain
                },
                target_resources: v.target_resources.clone(),
            })
            .collect(),
    )
}

async fn handle_dev_command(dev_cmd: DevCommand, global_no_tui: bool) -> Result<()> {
    let port = dev_cmd.port;
    let platform = dev_cmd.platform;
    let skip_build = dev_cmd.skip_build;
    let status_file = dev_cmd.status_file;
    let deployment_name = dev_cmd.deployment_name;
    let config_file = dev_cmd.config;
    let ctx = ExecutionMode::Dev { port };

    // Parse --env and --secret flags
    let parsed_env_vars = parse_env_and_secret_vars(&dev_cmd.env_vars, &dev_cmd.secret_vars)?;

    match dev_cmd.subcommand {
        None => {
            // No subcommand: run dev TUI
            run_dev_tui(
                port,
                global_no_tui,
                skip_build,
                status_file,
                deployment_name,
                parsed_env_vars,
                config_file,
                platform.clone(),
            )
            .await?;
        }
        Some(DevSubcommand::Server) => {
            run_dev_server_only(port, status_file, deployment_name, parsed_env_vars).await?;
        }
        Some(DevSubcommand::Deployments(args)) => {
            deployments_task(args, ctx).await?;
        }
        Some(DevSubcommand::Whoami(args)) => {
            whoami_task(args, ctx).await?;
        }
        Some(DevSubcommand::Deploy(args)) => {
            deploy_task(args, ctx).await?;
        }
        Some(DevSubcommand::Destroy(args)) => {
            destroy_task(args, ctx).await?;
        }
        Some(DevSubcommand::Release(mut args)) => {
            // Apply global no_tui
            args.no_tui = global_no_tui || args.no_tui;
            release_command(args, ctx).await?;
        }
        Some(DevSubcommand::Vault(args)) => {
            vault_task(args, port).await?;
        }
    }

    Ok(())
}

/// Run the TUI dashboard for managing platform resources
async fn run_tui_dashboard(
    base_url: Option<String>,
    api_key: Option<String>,
    _workspace: Option<String>,
    project: Option<String>,
) -> Result<()> {
    use crate::auth::{get_auth_http, AuthOpts};
    use crate::project_link::{get_project_link_status, ProjectLinkStatus};

    let current_dir = get_current_dir()?;

    // Get authentication
    let auth_opts = AuthOpts {
        api_key: api_key.clone(),
        base_url: base_url.clone(),
        no_browser: true, // Don't open browser for TUI
    };
    let auth = get_auth_http(&auth_opts)
        .await
        .context(ErrorData::AuthenticationFailed {
            reason: "Not logged in. Run 'alien login' first.".to_string(),
        })?;

    // Validate we have a project (either specified or linked)
    // The SDK client is already scoped by auth, but we want to ensure we're in a project context
    let project_id = if let Some(p) = project {
        p
    } else {
        // Check if current directory is linked to a project
        match get_project_link_status(&current_dir) {
            ProjectLinkStatus::Linked(link) => link.project_id,
            ProjectLinkStatus::NotLinked => {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "No project linked. Run 'alien link' first or specify --project."
                        .to_string(),
                }));
            }
            ProjectLinkStatus::Error(e) => {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Project link error: {}", e),
                }));
            }
        }
    };

    let config = AppConfig::platform(auth.sdk_client().clone()).with_project(project_id);

    run_app(config).await.map_err(|e| {
        AlienError::new(ErrorData::TuiOperationFailed {
            message: e.to_string(),
        })
    })
}

/// Run dev TUI
async fn run_dev_tui(
    port: u16,
    no_tui: bool,
    skip_build: bool,
    status_file: Option<PathBuf>,
    deployment_name: String,
    user_env_vars: Vec<CliEnvVar>,
    config_file: Option<PathBuf>,
    platform: String,
) -> Result<()> {
    let current_dir = get_current_dir()?;

    // Start dev server if not running
    ensure_server_running_with_env(port, status_file.clone(), user_env_vars.clone()).await?;

    // Convert CLI env vars to core EnvironmentVariable for deployment creation
    let core_env_vars = cli_env_vars_to_core(&user_env_vars);

    if no_tui {
        // Console mode
        info!("Starting dev environment...");
        let _release_id = build_and_post_release_simple(
            &current_dir,
            port,
            skip_build,
            config_file.as_ref(),
            &platform,
        )
        .await?;
        create_initial_deployment(&deployment_name, &platform, port, core_env_vars).await?;

        info!("Dev environment ready!");
        info!("   Press Ctrl+C to stop.");

        tokio::signal::ctrl_c().await.into_alien_error().context(
            ErrorData::TuiOperationFailed {
                message: "Failed to wait for ctrl-c".to_string(),
            },
        )?;
        return Ok(());
    }

    // TUI mode
    let (build_status_tx, build_status_rx) = tokio::sync::mpsc::channel(10);
    let (rebuild_tx, rebuild_rx) = tokio::sync::mpsc::channel(1);
    let (terminal_ready_tx, terminal_ready_rx) = tokio::sync::oneshot::channel();

    // Spawn build task - it will wait for terminal initialization
    let env_vars_for_task = cli_env_vars_to_core(&user_env_vars);
    let build_task = tokio::spawn(run_build_task(
        current_dir.clone(),
        port,
        deployment_name.clone(),
        platform.clone(),
        env_vars_for_task,
        build_status_tx,
        rebuild_rx,
        terminal_ready_rx,
    ));

    // Run TUI - this will initialize terminal first, then signal readiness
    let sdk = alien_platform_api::Client::new(&format!("http://localhost:{}", port));
    let config = AppConfig::dev(sdk)
        .with_build_channels(build_status_rx, rebuild_tx)
        .with_terminal_ready_signal(terminal_ready_tx);

    let result = run_app(config).await.map_err(|e| {
        AlienError::new(ErrorData::TuiOperationFailed {
            message: e.to_string(),
        })
    });

    // Cleanup
    build_task.abort();
    result
}

/// Build task - runs initial build and listens for rebuild requests
///
/// This task waits for terminal initialization before starting.
async fn run_build_task(
    current_dir: PathBuf,
    port: u16,
    deployment_name: String,
    platform: String,
    environment_variables: Option<Vec<alien_core::EnvironmentVariable>>,
    build_status_tx: tokio::sync::mpsc::Sender<BuildState>,
    mut rebuild_rx: tokio::sync::mpsc::Receiver<()>,
    terminal_ready_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Wait for terminal to be initialized in raw mode
    // This ensures no build output can corrupt terminal state
    if terminal_ready_rx.await.is_err() {
        // Terminal initialization failed or was cancelled
        return;
    }

    // Initial build
    build_status_tx.send(BuildState::Initializing).await.ok();
    let build_start = Instant::now();
    match build_and_post_release_simple(&current_dir, port, false, None, &platform).await {
        Ok(_release_id) => {
            let duration = build_start.elapsed();
            build_status_tx
                .send(BuildState::Built { duration })
                .await
                .ok();

            // Create initial deployment after successful build
            if let Err(e) =
                create_initial_deployment(&deployment_name, &platform, port, environment_variables)
                    .await
            {
                build_status_tx
                    .send(BuildState::Failed {
                        error: format!("Failed to create deployment: {}", e),
                    })
                    .await
                    .ok();
            }
        }
        Err(e) => {
            build_status_tx
                .send(BuildState::Failed {
                    error: e.to_string(),
                })
                .await
                .ok();
        }
    }

    // Continue listening for rebuild requests
    while rebuild_rx.recv().await.is_some() {
        build_status_tx.send(BuildState::Building).await.ok();
        let build_start = Instant::now();
        match build_and_post_release_simple(&current_dir, port, false, None, &platform).await {
            Ok(_) => {
                let duration = build_start.elapsed();
                build_status_tx
                    .send(BuildState::Built { duration })
                    .await
                    .ok();
            }
            Err(e) => {
                build_status_tx
                    .send(BuildState::Failed {
                        error: e.to_string(),
                    })
                    .await
                    .ok();
            }
        }
    }
}

/// Run dev server only mode
async fn run_dev_server_only(
    port: u16,
    _status_file: Option<PathBuf>,
    _deployment_name: String,
    _user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    info!("Starting dev server on port {}...", port);

    let current_dir = get_current_dir()?;
    let state_dir = current_dir.join(".alien");
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
        dev_mode: true,
        ..Default::default()
    };

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let server = alien_manager::AlienManager::builder(config)
        .build()
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to initialize dev server".to_string(),
        })?;

    info!("Dev server ready");
    info!("   API: http://localhost:{}/v1/deployments", port);
    info!("   Press Ctrl+C to stop");

    server
        .start(addr)
        .await
        .into_alien_error()
        .context(ErrorData::ServerStartFailed {
            reason: "Server stopped unexpectedly".to_string(),
        })
}

/// Main CLI entry point
pub async fn run_cli(cli: Cli) -> Result<()> {
    // Change working directory if specified
    if let Some(ref dir) = cli.dir {
        env::set_current_dir(dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "change directory".to_string(),
                file_path: dir.clone(),
                reason: "Failed to change to specified directory".to_string(),
            })?;
    }

    // Set up tracing early based on the no_tui flag or individual command settings
    let global_no_tui = cli.no_tui;
    let command_no_tui = match &cli.command {
        Some(Commands::Build(args)) => args.no_tui,
        Some(Commands::Dev(_)) => false, // Dev TUI is handled in handle_dev_command
        _ => false,                      // Other commands don't have TUI, or no command (TUI mode)
    };
    setup_tracing(global_no_tui || command_no_tui);

    // Construct execution context once from global flags
    // Mode resolution: ALIEN_SERVER → self-hosted, else → platform
    let ctx = if let Ok(server_url) = env::var("ALIEN_SERVER") {
        let api_key = cli
            .api_key
            .clone()
            .or_else(|| env::var("ALIEN_API_KEY").ok())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "ALIEN_API_KEY is required when ALIEN_SERVER is set. \
                    Set ALIEN_API_KEY or use --api-key."
                        .to_string(),
                })
            })?;
        ExecutionMode::SelfHosted {
            server_url,
            api_key,
        }
    } else {
        ExecutionMode::Platform {
            base_url: cli
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.alien.dev".to_string()),
            api_key: cli.api_key.clone(),
            no_browser: cli.no_browser,
            workspace: cli.workspace.clone(),
            project: cli.project.clone(),
        }
    };

    match cli.command {
        // No subcommand: launch the TUI dashboard
        None => {
            run_tui_dashboard(cli.base_url, cli.api_key, cli.workspace, cli.project).await?;
        }
        Some(Commands::Build(mut build_args)) => {
            build_args.no_tui = global_no_tui || build_args.no_tui;
            build_command(build_args).await?;
        }
        Some(Commands::Dev(dev_cmd)) => {
            handle_dev_command(dev_cmd, global_no_tui).await?;
        }

        // Platform commands — all receive ctx
        Some(Commands::Login(args)) => login_task(args, ctx).await?,
        Some(Commands::Workspaces(args)) => workspace_task(args, ctx).await?,
        Some(Commands::Projects(args)) => project_task(args, ctx).await?,
        Some(Commands::Link(args)) => link_task(args, ctx).await?,
        Some(Commands::Onboard(args)) => onboard_task(args, ctx).await?,
        Some(Commands::Deployments(args)) => deployments_task(args, ctx).await?,
        Some(Commands::Whoami(args)) => whoami_task(args, ctx).await?,
        Some(Commands::Deploy(args)) => deploy_task(args, ctx).await?,
        Some(Commands::Destroy(args)) => destroy_task(args, ctx).await?,
        Some(Commands::Release(mut args)) => {
            args.no_tui = global_no_tui || args.no_tui;
            release_command(args, ctx).await?;
        }

        // Local-only commands — no ctx needed
        Some(Commands::Logout(args)) => logout_task(args).await?,
        Some(Commands::Unlink(args)) => unlink_task(args).await?,
    }

    Ok(())
}

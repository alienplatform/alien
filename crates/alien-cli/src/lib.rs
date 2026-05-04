//! Alien CLI library.

pub mod auth;
pub mod commands;
pub mod config;
pub mod deployment_tracking;
pub mod error;
pub mod execution_context;
pub mod git_utils;
pub mod interaction;
pub mod output;
pub mod project_link;
pub mod ui;

#[cfg(test)]
pub mod test_utils;

#[cfg(feature = "platform")]
use crate::commands::manager::{manager_task, ManagerArgs};
#[cfg(feature = "platform")]
use crate::commands::platform::{
    link_task, login_task, logout_task, project_task, unlink_task, workspace_task, PlatformCommand,
};
use crate::commands::{
    build_and_post_release_simple, build_command, build_dev_status, commands_task,
    commands_task_dev, deploy_task, deployments_task, destroy_task,
    ensure_server_running_for_dev_session, ensure_server_running_with_env,
    fetch_all_dev_deployment_live_states, init_task, onboard_task, prepare_dev_session_deployment,
    release_command, vault_remote_task, vault_task, whoami_task, write_dev_status, BuildArgs,
    CliEnvVar, CommandsArgs, DeployArgs, DeploymentsArgs, DestroyArgs, InitArgs, OnboardArgs,
    ReleaseArgs, WhoamiArgs,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{
    accent, command, contextual_heading, dim_label, event_bus_for_command,
    format_deployment_status, print_cli_banner, success_line, DevCardScreen, DevDeploymentCard,
    DevResourceEntry, FixedSteps, UiCommandKind,
};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager::AlienManager;
use clap::{CommandFactory, Parser, Subcommand};
use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "alien", author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Run as if alien was started in <path> instead of the current working directory
    #[arg(short = 'C', long, name = "path")]
    pub dir: Option<String>,

    /// Project to manage (defaults to linked project or interactive bootstrap)
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

impl Cli {
    pub fn wants_json_output(&self) -> bool {
        match &self.command {
            Some(Commands::Build(args)) => args.json,
            Some(Commands::Release(args)) => args.json,
            Some(Commands::Onboard(args)) => args.json,
            Some(Commands::Whoami(args)) => args.json,
            Some(Commands::Dev(dev)) => match &dev.subcommand {
                Some(DevSubcommand::Release(args)) => args.json,
                Some(DevSubcommand::Whoami(args)) => args.json,
                _ => false,
            },
            #[cfg(feature = "platform")]
            Some(Commands::Platform(PlatformCommand::Link(args))) => args.json,
            #[cfg(feature = "platform")]
            Some(Commands::Platform(PlatformCommand::Login(args))) => args.json,
            #[cfg(feature = "platform")]
            Some(Commands::Platform(PlatformCommand::Workspaces(args))) => args.json,
            #[cfg(feature = "platform")]
            Some(Commands::Platform(PlatformCommand::Projects(args))) => args.json,
            #[cfg(feature = "platform")]
            Some(Commands::Manager(args)) => args.json,
            _ => false,
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scaffold a new project from a template
    Init(InitArgs),
    /// Build the Alien application
    Build(BuildArgs),
    /// Push images and create a release
    Release(ReleaseArgs),
    /// Create a deployment group and generate a deployment link
    Onboard(OnboardArgs),
    /// Deployment commands
    #[command(alias = "deployment")]
    Deployments(DeploymentsArgs),
    /// Deploy to a cloud platform
    Deploy(DeployArgs),
    /// Destroy resources from a deployment
    Destroy(DestroyArgs),
    /// Manage vault secrets for a deployment
    Vault(commands::VaultRemoteArgs),
    /// Invoke remote commands on deployments
    #[command(alias = "command")]
    Commands(CommandsArgs),
    /// Start a standalone alien-manager server
    Serve(ServeArgs),
    /// Local development commands
    Dev(DevCommand),
    /// Show current authenticated user information
    Whoami(WhoamiArgs),

    #[cfg(feature = "platform")]
    #[command(flatten)]
    Platform(PlatformCommand),

    /// Manage private managers deployed to your cloud
    #[cfg(feature = "platform")]
    Manager(ManagerArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct ServeArgs {
    /// Path to TOML configuration file (default: alien-manager.toml in CWD).
    #[arg(long, short = 'c')]
    pub config: Option<PathBuf>,

    /// Generate a template alien-manager.toml and exit.
    #[arg(long)]
    pub init: bool,

    /// Override the HTTP server port.
    #[arg(long)]
    pub port: Option<u16>,

    /// Override the HTTP server bind address.
    #[arg(long)]
    pub host: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct DevCommand {
    /// Dev server port
    #[arg(long, default_value = "9090", global = true)]
    pub port: u16,

    /// Path to configuration file (default: auto-discover alien.ts in current directory)
    #[arg(long, short = 'c')]
    pub config: Option<PathBuf>,

    /// Skip the build step (use existing build artifacts)
    #[arg(long)]
    pub skip_build: bool,

    /// Path to write status file (JSON with alien_core::DevStatus)
    #[arg(long)]
    pub status_file: Option<PathBuf>,

    /// Deployment name for the initial deployment
    #[arg(long, default_value = "default")]
    pub deployment_name: String,

    /// Plain environment variables (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "env")]
    pub env_vars: Vec<String>,

    /// Secret environment variables (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "secret")]
    pub secret_vars: Vec<String>,

    #[command(subcommand)]
    pub subcommand: Option<DevSubcommand>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DevSubcommand {
    /// Start the local manager only
    Server,
    /// Deployment commands against the local manager
    #[command(alias = "deployment")]
    Deployments(DeploymentsArgs),
    /// Show local manager identity information
    Whoami(WhoamiArgs),
    /// Deploy to the local manager
    Deploy(DeployArgs),
    /// Destroy from the local manager
    Destroy(DestroyArgs),
    /// Create a release on the local manager
    Release(ReleaseArgs),
    /// Manage vault secrets for local dev deployments
    Vault(commands::VaultArgs),
    /// Invoke remote commands on local dev deployments
    #[command(alias = "command")]
    Commands(CommandsArgs),
}

pub fn get_current_dir() -> Result<std::path::PathBuf> {
    std::env::current_dir()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "get current directory".to_string(),
            file_path: ".".to_string(),
            reason: "Failed to get current directory".to_string(),
        })
}

pub fn setup_tracing() {
    let file_path = std::env::var("ALIEN_LOG_FILE").ok();
    let env_filter = std::env::var("ALIEN_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| {
            if file_path.is_some() {
                EnvFilter::new(
                    "alien_cli=debug,alien_core=debug,alien_infra=debug,alien_build=debug,alien_manager=debug,oci_tar_builder=error",
                )
            } else {
                EnvFilter::new("off")
            }
        });

    #[cfg(feature = "otlp")]
    let otlp_layer = alien_runtime::init_otlp_logging().ok().flatten();

    #[cfg(not(feature = "otlp"))]
    let _otlp_layer: Option<()> = None;

    let registry = tracing_subscriber::registry().with(env_filter);
    let stderr_layer = fmt::layer()
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(std::io::stderr);

    match file_path {
        Some(path) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("Failed to open ALIEN_LOG_FILE");
            let layers = registry
                .with(stderr_layer)
                .with(fmt::layer().with_ansi(false).with_writer(file));

            #[cfg(feature = "otlp")]
            if let Some(otlp) = otlp_layer {
                layers.with(otlp).init();
            } else {
                layers.init();
            }

            #[cfg(not(feature = "otlp"))]
            layers.init();
        }
        None => {
            let layers = registry.with(stderr_layer);

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

fn parse_env_and_secret_vars(
    env_vars: &[String],
    secret_vars: &[String],
) -> Result<Vec<CliEnvVar>> {
    let mut parsed = Vec::new();
    for env in env_vars {
        parsed.push(parse_single_env_var(env, false)?);
    }
    for secret in secret_vars {
        parsed.push(parse_single_env_var(secret, true)?);
    }
    Ok(parsed)
}

fn parse_single_env_var(input: &str, is_secret: bool) -> Result<CliEnvVar> {
    let parts: Vec<&str> = input.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Invalid {} format: '{input}'. Expected KEY=VALUE or KEY=VALUE:target1,target2",
                if is_secret { "--secret" } else { "--env" }
            ),
        }));
    }

    let name = parts[0].to_string();
    let value_with_targets = parts[1];
    let (value, target_resources) = if let Some(colon_pos) = value_with_targets.rfind(':') {
        let potential_value = &value_with_targets[..colon_pos];
        let potential_targets = &value_with_targets[colon_pos + 1..];
        let looks_like_targets = !potential_targets.is_empty()
            && !potential_targets.chars().all(|c| c.is_ascii_digit())
            && !potential_targets.starts_with('/');

        if looks_like_targets {
            let targets: Vec<String> = potential_targets
                .split(',')
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();

            if targets.is_empty() {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Invalid {} format: '{input}'. Targets list is empty after ':'.",
                        if is_secret { "--secret" } else { "--env" }
                    ),
                }));
            }

            (potential_value.to_string(), Some(targets))
        } else {
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

fn cli_env_vars_to_core(cli_vars: &[CliEnvVar]) -> Option<Vec<alien_core::EnvironmentVariable>> {
    if cli_vars.is_empty() {
        return None;
    }

    Some(
        cli_vars
            .iter()
            .map(|var| alien_core::EnvironmentVariable {
                name: var.name.clone(),
                value: var.value.clone(),
                var_type: if var.is_secret {
                    alien_core::EnvironmentVariableType::Secret
                } else {
                    alien_core::EnvironmentVariableType::Plain
                },
                target_resources: var.target_resources.clone(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::EnvironmentVariableType;

    #[test]
    fn parse_single_env_var_supports_plain_value() {
        let parsed = parse_single_env_var("LOG_LEVEL=info", false).unwrap();
        assert_eq!(parsed.name, "LOG_LEVEL");
        assert_eq!(parsed.value, "info");
        assert!(!parsed.is_secret);
        assert_eq!(parsed.target_resources, None);
    }

    #[test]
    fn parse_single_env_var_preserves_urls_without_targets() {
        let parsed = parse_single_env_var("API_URL=https://example.com:8080", false).unwrap();
        assert_eq!(parsed.value, "https://example.com:8080");
        assert_eq!(parsed.target_resources, None);
    }

    #[test]
    fn parse_single_env_var_extracts_targets() {
        let parsed = parse_single_env_var("API_URL=https://example.com:api,worker", true).unwrap();
        assert_eq!(parsed.value, "https://example.com");
        assert_eq!(
            parsed.target_resources,
            Some(vec!["api".to_string(), "worker".to_string()])
        );
        assert!(parsed.is_secret);
    }

    #[test]
    fn parse_single_env_var_rejects_invalid_input() {
        let err = parse_single_env_var("MISSING_EQUALS", false).unwrap_err();
        assert!(err.to_string().contains("Invalid --env format"));
    }

    #[test]
    fn cli_env_vars_to_core_maps_secret_and_targets() {
        let vars = vec![
            CliEnvVar {
                name: "LOG_LEVEL".to_string(),
                value: "debug".to_string(),
                is_secret: false,
                target_resources: None,
            },
            CliEnvVar {
                name: "API_KEY".to_string(),
                value: "shh".to_string(),
                is_secret: true,
                target_resources: Some(vec!["api".to_string()]),
            },
        ];

        let converted = cli_env_vars_to_core(&vars).unwrap();
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].var_type, EnvironmentVariableType::Plain);
        assert_eq!(converted[1].var_type, EnvironmentVariableType::Secret);
        assert_eq!(converted[1].target_resources, Some(vec!["api".to_string()]));
    }
}

async fn serve_task(args: ServeArgs) -> Result<()> {
    use alien_manager::standalone_config::ManagerTomlConfig;
    use alien_manager::traits::TokenType;

    // Handle --init: generate template and exit
    if args.init {
        print!("{}", ManagerTomlConfig::generate_template());
        return Ok(());
    }

    let toml_config = ManagerTomlConfig::load(args.config.as_deref()).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to load config: {}", e),
        })
    })?;

    let mut config = toml_config.to_manager_config();

    // Apply CLI overrides
    if let Some(port) = args.port {
        config.port = port;
    }
    if let Some(ref host) = args.host {
        config.host = host.clone();
    }

    let addr: std::net::SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Invalid bind address '{}:{}'", config.host, config.port),
        })?;

    let state_dir = config
        .state_dir
        .as_ref()
        .expect("state_dir is required for standalone mode");
    std::fs::create_dir_all(state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to create state directory".to_string(),
        })?;

    let db_path = config
        .db_path
        .as_ref()
        .expect("db_path is required for standalone mode");

    // Create SQLite database and token store first (needed for token bootstrap)
    let db = std::sync::Arc::new(
        alien_manager::stores::sqlite::SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to initialize database".to_string(),
            })?,
    );
    let token_store: std::sync::Arc<dyn alien_manager::traits::TokenStore> =
        std::sync::Arc::new(alien_manager::stores::sqlite::SqliteTokenStore::new(db));

    // Bootstrap admin token — DB is the source of truth.
    // The plaintext token is only shown on first generation (like AWS/Stripe API keys).
    let legacy_token_path = state_dir.join("admin-token");
    let existing_tokens = token_store.list_tokens().await.context(
        ErrorData::ServerStartFailed {
            reason: "Failed to list tokens".to_string(),
        },
    )?;
    let existing_admin = existing_tokens
        .iter()
        .find(|t| t.token_type == TokenType::Admin);

    let generated_token = if existing_admin.is_some() {
        // Existing admin token in DB — migrate away from legacy plaintext file
        if legacy_token_path.exists() {
            let _ = std::fs::remove_file(&legacy_token_path);
        }
        None
    } else if legacy_token_path.exists() {
        // Legacy migration: read plaintext file, hash into DB, delete file
        let raw = std::fs::read_to_string(&legacy_token_path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: legacy_token_path.display().to_string(),
                reason: "Failed to read legacy admin token".to_string(),
            })?;
        let raw = raw.trim().to_string();
        let key_hash = hash_token(&raw);
        let key_prefix = raw[..12.min(raw.len())].to_string();
        token_store
            .create_token(alien_manager::traits::CreateTokenParams {
                token_type: TokenType::Admin,
                key_prefix,
                key_hash,
                deployment_group_id: None,
                deployment_id: None,
            })
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to migrate admin token".to_string(),
            })?;
        let _ = std::fs::remove_file(&legacy_token_path);
        // Show the token one last time during migration
        Some(raw)
    } else {
        // First run: generate new token, hash into DB, show once
        let raw = format!(
            "ax_admin_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let key_hash = hash_token(&raw);
        let key_prefix = raw[..12.min(raw.len())].to_string();
        token_store
            .create_token(alien_manager::traits::CreateTokenParams {
                token_type: TokenType::Admin,
                key_prefix,
                key_hash,
                deployment_group_id: None,
                deployment_id: None,
            })
            .await
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to bootstrap admin token".to_string(),
            })?;
        Some(raw)
    };

    // Re-read the admin token record for the prefix (used in subsequent-run display)
    let admin_prefix = if generated_token.is_none() {
        let tokens = token_store.list_tokens().await.unwrap_or_default();
        tokens
            .iter()
            .find(|t| t.token_type == TokenType::Admin)
            .map(|t| t.key_prefix.clone())
    } else {
        None
    };

    // Build the server
    let server = AlienManager::builder(config.clone())
        .token_store(token_store)
        .with_standalone_defaults(&toml_config)
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to set up standalone defaults".to_string(),
        })?
        .build()
        .await
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to build alien-manager".to_string(),
        })?;

    // Clean up stale deployment locks. Startup hook — `Subject::system()` is
    // the synthetic operator (single-tenant standalone mode).
    let deployment_store = server.deployment_store().clone();
    let startup_caller = alien_manager::auth::Subject::system();
    match deployment_store.cleanup_stale_locks(&startup_caller).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(count = n, "Cleaned up stale deployment locks"),
        Err(e) => tracing::warn!(error = %e, "Failed to clean up stale deployment locks"),
    }

    let bind_url = format!("http://{}:{}", config.host, config.port);
    let manager_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://localhost:{}", config.port));

    // Print styled output
    println!(
        "{}",
        contextual_heading("Serving", "Alien Manager", &[("on", bind_url.as_str())])
    );

    if let Some(ref token) = generated_token {
        println!();
        println!(
            "{}",
            success_line("Admin token generated (save this securely):")
        );
        println!("  {}", accent(token));
    }

    println!();
    println!("  {} {}", dim_label("Manager URL"), accent(&manager_url));
    if generated_token.is_none() {
        if let Some(ref prefix) = admin_prefix {
            println!(
                "  {} {}…",
                dim_label("Admin token"),
                accent(prefix),
            );
        }
    }

    println!();
    if let Some(ref token) = generated_token {
        println!(
            "  {}",
            dim_label(&format!("export ALIEN_MANAGER_URL={manager_url}"))
        );
        println!(
            "  {}",
            dim_label(&format!("export ALIEN_API_KEY={token}"))
        );
    } else {
        println!(
            "  {}",
            dim_label(&format!("export ALIEN_MANAGER_URL={manager_url}"))
        );
    }

    println!();
    println!(
        "  {} {}",
        dim_label("Next"),
        command("alien release --platform aws")
    );
    println!(
        "        {}",
        command("alien onboard <customer-name>")
    );
    println!();

    // Spawn server in background so we can run the deployment watch loop
    let server_handle = tokio::spawn(async move {
        server.start(addr).await
    });

    // Watch deployments until Ctrl+C or server exit
    watch_serve_deployments(deployment_store, server_handle).await
}

fn hash_token(token: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Poll deployment store and render auto-updating deployment cards.
async fn watch_serve_deployments(
    deployment_store: std::sync::Arc<dyn alien_manager::traits::DeploymentStore>,
    server_handle: tokio::task::JoinHandle<
        std::result::Result<(), alien_error::AlienError<alien_manager::error::ErrorData>>,
    >,
) -> Result<()> {
    use alien_manager::traits::DeploymentFilter;
    use crate::ui::{
        render_deployment_cards, render_serve_actions_footer, supports_ansi, LiveRegion,
    };

    let is_tty = supports_ansi();
    let live = if is_tty {
        Some(std::sync::Arc::new(LiveRegion::new()))
    } else {
        None
    };

    let mut last_printed_statuses: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    tokio::pin!(server_handle);

    loop {
        tokio::select! {
            result = &mut ctrl_c => {
                result.into_alien_error().context(ErrorData::ServerStartFailed {
                    reason: "Failed to wait for Ctrl+C".to_string(),
                })?;
                return Ok(());
            }
            result = &mut server_handle => {
                // Server exited — propagate the error
                match result {
                    Ok(Ok(())) => return Ok(()),
                    Ok(Err(e)) => return Err(AlienError::new(ErrorData::ServerStartFailed {
                        reason: format!("Server exited with error: {}", e),
                    })),
                    Err(e) => return Err(AlienError::new(ErrorData::ServerStartFailed {
                        reason: format!("Server task panicked: {}", e),
                    })),
                }
            }
            _ = interval.tick() => {
                // Standalone CLI dev loop — single-tenant, synthetic operator.
                let dev_caller = alien_manager::auth::Subject::system();
                let deployments = match deployment_store
                    .list_deployments(&dev_caller, &DeploymentFilter::default())
                    .await
                {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                if deployments.is_empty() {
                    // Clear cards if there are no deployments
                    if let Some(live) = &live {
                        live.set_section("cards", vec![]);
                    }
                    continue;
                }

                // Build group_id → group_name map for card labels
                let group_names: std::collections::HashMap<String, String> =
                    match deployment_store.list_deployment_groups(&dev_caller).await {
                        Ok(groups) => groups
                            .into_iter()
                            .map(|g| (g.id, g.name))
                            .collect(),
                        Err(_) => std::collections::HashMap::new(),
                    };

                let cards: Vec<DevDeploymentCard> = deployments
                    .iter()
                    .map(|record| deployment_record_to_card(record, &group_names))
                    .collect();

                if let Some(live) = &live {
                    let cards_text = render_deployment_cards(&cards);
                    let footer_text = render_serve_actions_footer();
                    let mut lines: Vec<String> = Vec::new();
                    for line in cards_text.lines() {
                        lines.push(line.to_string());
                    }
                    for line in footer_text.lines() {
                        lines.push(line.to_string());
                    }
                    live.set_section("cards", lines);
                } else {
                    // Non-TTY: print status changes as log lines
                    for record in &deployments {
                        let prev = last_printed_statuses.get(&record.name);
                        if prev.is_none() || prev != Some(&record.status) {
                            eprintln!(
                                "{}: {}",
                                record.name,
                                record.status,
                            );
                            last_printed_statuses
                                .insert(record.name.clone(), record.status.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Convert a DeploymentRecord to a DevDeploymentCard for rendering.
fn deployment_record_to_card(
    record: &alien_manager::traits::DeploymentRecord,
    group_names: &std::collections::HashMap<String, String>,
) -> DevDeploymentCard {
    let status: alien_core::DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(record.status.clone()))
            .unwrap_or(alien_core::DeploymentStatus::Pending);

    let resources = record
        .stack_state
        .as_ref()
        .map(|state| {
            let mut entries: Vec<DevResourceEntry> = state
                .resources
                .iter()
                .map(|(name, res)| DevResourceEntry {
                    name: name.clone(),
                    url: Some(
                        crate::ui::format_resource_status(res.status)
                            .to_ascii_lowercase(),
                    ),
                })
                .collect();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            entries
        })
        .unwrap_or_default();

    let error = record
        .error
        .as_ref()
        .and_then(deployment_error_message);

    // Show group_name/deployment_name so the user knows which project a deployment belongs to
    let card_name = match group_names.get(&record.deployment_group_id) {
        Some(group_name) => format!("{}/{}", group_name, record.name),
        None => record.name.clone(),
    };

    DevDeploymentCard {
        name: card_name,
        status,
        platform: Some(record.platform),
        resources,
        error,
    }
}

async fn handle_dev_command(dev_cmd: DevCommand) -> Result<()> {
    let port = dev_cmd.port;
    let ctx = ExecutionMode::Dev { port };
    let parsed_env_vars = parse_env_and_secret_vars(&dev_cmd.env_vars, &dev_cmd.secret_vars)?;

    match dev_cmd.subcommand {
        None => {
            run_dev_session(
                port,
                dev_cmd.skip_build,
                dev_cmd.status_file,
                dev_cmd.deployment_name,
                parsed_env_vars,
                dev_cmd.config,
            )
            .await?;
        }
        Some(DevSubcommand::Server) => {
            run_dev_server_only(port, dev_cmd.status_file, parsed_env_vars).await?;
        }
        Some(DevSubcommand::Deployments(args)) => deployments_task(args, ctx).await?,
        Some(DevSubcommand::Whoami(args)) => whoami_task(args, ctx).await?,
        Some(DevSubcommand::Deploy(args)) => deploy_task(args, ctx).await?,
        Some(DevSubcommand::Destroy(args)) => destroy_task(args, ctx).await?,
        Some(DevSubcommand::Release(args)) => release_command(args, ctx).await?,
        Some(DevSubcommand::Vault(args)) => vault_task(args, port).await?,
        Some(DevSubcommand::Commands(args)) => commands_task_dev(args, port).await?,
    }

    Ok(())
}

async fn run_dev_session(
    port: u16,
    skip_build: bool,
    status_file: Option<PathBuf>,
    deployment_name: String,
    user_env_vars: Vec<CliEnvVar>,
    config_file: Option<PathBuf>,
) -> Result<()> {
    let current_dir = get_current_dir()?;
    let core_env_vars = cli_env_vars_to_core(&user_env_vars);
    let app_name = current_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("project")
        .to_string();

    println!("{}", crate::ui::heading("Local Development"));
    println!("{} {}", dim_label("Project"), accent(&app_name));
    println!();

    let steps = FixedSteps::new(&["Build local release", "Start local deployment"]);

    if let Some(status_file) = &status_file {
        write_dev_status(
            status_file,
            &build_dev_status(port, alien_core::DevStatusState::Initializing, None, None),
        )?;
    }

    let result = async {
        // Start local services (invisible step — fast, no user-facing progress)
        ensure_server_running_for_dev_session(port, status_file.clone(), user_env_vars).await?;

        // Step 0: Building
        let is_tty = steps.is_enabled();
        if skip_build {
            steps.skip(0, None::<String>);
        } else {
            steps.activate(0, None::<String>);
            if !is_tty {
                eprint!("Building... ");
            }
        }
        build_and_post_release_simple(&current_dir, port, skip_build, config_file.as_ref()).await?;
        if !skip_build {
            steps.complete(0, None::<String>);
            if !is_tty {
                eprintln!("done");
            }
        }

        // Step 1: Deploying — completed in the watch loop when deployment reaches Running
        steps.activate(1, None::<String>);
        if !is_tty {
            eprint!("Starting... ");
        }
        prepare_dev_session_deployment(&deployment_name, port, core_env_vars.clone()).await?;

        let screen = DevCardScreen::new(steps.live_region());
        watch_dev_deployments_until_ctrl_c(
            port,
            &deployment_name,
            status_file.as_ref(),
            &screen,
            &steps,
            1,
        )
        .await?;

        Ok::<(), alien_error::AlienError<ErrorData>>(())
    }
    .await;

    if let Some(status_file) = &status_file {
        let status = match &result {
            Ok(()) => build_dev_status(port, alien_core::DevStatusState::ShuttingDown, None, None),
            Err(error) => build_dev_status(
                port,
                alien_core::DevStatusState::Error,
                None,
                Some(error.clone().into_generic()),
            ),
        };
        write_dev_status(status_file, &status)?;
    }

    result
}

async fn run_dev_server_only(
    port: u16,
    status_file: Option<PathBuf>,
    user_env_vars: Vec<CliEnvVar>,
) -> Result<()> {
    println!(
        "{}",
        contextual_heading(
            "Local Development",
            "server",
            &[("on", &format!("http://localhost:{port}"))],
        )
    );
    ensure_server_running_with_env(port, status_file.clone(), user_env_vars).await?;

    if let Some(status_file) = &status_file {
        write_dev_status(
            status_file,
            &build_dev_status(port, alien_core::DevStatusState::Ready, None, None),
        )?;
    }

    println!("{}", success_line("Local manager ready."));
    println!(
        "{} {}",
        dim_label("Manager"),
        accent(&format!("http://localhost:{port}"))
    );
    println!(
        "{} run {} for the full local app flow.",
        dim_label("Next"),
        command("alien dev")
    );

    tokio::signal::ctrl_c()
        .await
        .into_alien_error()
        .context(ErrorData::ServerStartFailed {
            reason: "Failed to wait for Ctrl+C".to_string(),
        })?;

    if let Some(status_file) = &status_file {
        write_dev_status(
            status_file,
            &build_dev_status(port, alien_core::DevStatusState::ShuttingDown, None, None),
        )?;
    }

    Ok(())
}

fn deployment_error_message(error: &serde_json::Value) -> Option<String> {
    // For AGENT_DEPLOYMENT_FAILED errors, extract per-resource root causes
    // instead of the generic "Deployment failed: N resource error(s)..." summary.
    if error.get("code").and_then(|v| v.as_str()) == Some("AGENT_DEPLOYMENT_FAILED") {
        if let Some(resource_errors) = error
            .get("context")
            .and_then(|c| c.get("resource_errors"))
            .and_then(|v| v.as_array())
        {
            let details: Vec<String> = resource_errors
                .iter()
                .filter_map(|re| {
                    let resource_id = re.get("resourceId").and_then(|v| v.as_str())?;
                    let err = re.get("error")?;
                    let msg = root_cause_message(err)?;
                    Some(format!("{resource_id}: {msg}"))
                })
                .collect();

            if !details.is_empty() {
                return Some(details.join("; "));
            }
        }
    }

    error
        .get("message")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| error.as_str().map(ToOwned::to_owned))
}

/// Walk the source chain of a serialized AlienError to find the root cause message.
/// Prefers the deepest non-internal error; falls back to the deepest error overall.
fn root_cause_message(error: &serde_json::Value) -> Option<String> {
    let mut deepest_non_internal: Option<&str> = None;
    let mut deepest: Option<&str> = None;
    let mut current = error;

    loop {
        let msg = current.get("message").and_then(|v| v.as_str());
        let is_internal = current
            .get("internal")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if let Some(m) = msg {
            deepest = Some(m);
            if !is_internal {
                deepest_non_internal = Some(m);
            }
        }

        match current.get("source") {
            Some(source) if source.is_object() => current = source,
            _ => break,
        }
    }

    deepest_non_internal.or(deepest).map(ToOwned::to_owned)
}

async fn watch_dev_deployments_until_ctrl_c(
    port: u16,
    primary_deployment_name: &str,
    status_file: Option<&PathBuf>,
    screen: &DevCardScreen,
    steps: &FixedSteps,
    deploy_step: usize,
) -> Result<()> {
    let mut deploy_step_completed = false;
    let mut last_printed_statuses: std::collections::HashMap<String, alien_core::DeploymentStatus> =
        std::collections::HashMap::new();
    let mut printed_ready_resources: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut printed_non_tty_footer = false;
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            result = &mut ctrl_c => {
                result.into_alien_error().context(ErrorData::ServerStartFailed {
                    reason: "Failed to wait for Ctrl+C".to_string(),
                })?;
                return Ok(());
            }
            _ = interval.tick() => {
                let states = fetch_all_dev_deployment_live_states(port).await?;
                let Some(primary_state) = states
                    .iter()
                    .find(|state| state.deployment_name == primary_deployment_name)
                else {
                    continue;
                };

                // Update status file (unchanged — machine-readable contract)
                if let Some(status_file) = status_file {
                    let snapshot = commands::DevDeploymentSnapshot {
                        deployment_id: primary_state.deployment_id.clone(),
                        deployment_name: primary_state.deployment_name.clone(),
                        status: primary_state.status,
                        commands_url: format!("http://localhost:{port}"),
                        resources: primary_state.resources.clone(),
                    };
                    let dev_status_state = if primary_state.status == alien_core::DeploymentStatus::Running {
                        alien_core::DevStatusState::Ready
                    } else {
                        alien_core::DevStatusState::Initializing
                    };
                    write_dev_status(
                        status_file,
                        &build_dev_status(port, dev_status_state, Some(&snapshot), None),
                    )?;
                }

                // Complete the deploy step on first ready
                if !deploy_step_completed && primary_state.status == alien_core::DeploymentStatus::Running {
                    deploy_step_completed = true;
                    steps.complete(deploy_step, None::<String>);
                    if !screen.is_enabled() {
                        eprintln!("done");
                    }
                }

                // Mark deploy step as failed if deployment failed
                if !deploy_step_completed && primary_state.status.is_failed() {
                    deploy_step_completed = true;
                    let error_msg = primary_state.error.as_ref().and_then(deployment_error_message);
                    steps.fail(deploy_step, error_msg.clone());
                    if !screen.is_enabled() {
                        match error_msg {
                            Some(msg) => eprintln!("failed ({msg})"),
                            None => eprintln!("failed"),
                        }
                    }
                }

                // Only render cards after the deploy step is done — avoids
                // overlapping with the FixedSteps spinner.
                if deploy_step_completed {
                    let cards = build_deployment_cards(&states);

                    if screen.is_enabled() {
                        screen.update(&cards);
                    } else {
                        print_deployment_log_updates(
                            &states,
                            &mut last_printed_statuses,
                            &mut printed_ready_resources,
                        );
                        if !printed_non_tty_footer
                            && states
                                .iter()
                                .any(|state| state.status == alien_core::DeploymentStatus::Running)
                        {
                            print_dev_actions_footer_non_tty();
                            printed_non_tty_footer = true;
                        }
                    }
                }
            }
        }
    }
}

fn build_deployment_cards(states: &[commands::DevDeploymentLiveState]) -> Vec<DevDeploymentCard> {
    states
        .iter()
        .map(|state| {
            let mut resource_names = std::collections::BTreeSet::new();
            resource_names.extend(state.resources.keys().cloned());
            if let Some(stack_state) = &state.stack_state {
                resource_names.extend(stack_state.resources.keys().cloned());
            }

            let mut resources: Vec<DevResourceEntry> = resource_names
                .into_iter()
                .map(|name| {
                    let public_resource = state.resources.get(&name);
                    let stack_resource = state
                        .stack_state
                        .as_ref()
                        .and_then(|stack| stack.resources.get(&name));
                    DevResourceEntry {
                        name: name.clone(),
                        url: Some(format_dev_resource_value(
                            &name,
                            public_resource,
                            stack_resource,
                        )),
                    }
                })
                .collect();
            resources.sort_by(|a, b| a.name.cmp(&b.name));

            let error = state.error.as_ref().and_then(deployment_error_message);

            DevDeploymentCard {
                name: state.deployment_name.clone(),
                status: state.status,
                platform: Some(Platform::Local),
                resources,
                error,
            }
        })
        .collect()
}

fn format_dev_resource_value(
    resource_name: &str,
    public_resource: Option<&alien_core::DevResourceInfo>,
    stack_resource: Option<&alien_core::StackResourceState>,
) -> String {
    if let Some(public_resource) = public_resource {
        if resource_name == "worker" && is_local_private_url(&public_resource.url) {
            return "running (private)".to_string();
        }
        return public_resource.url.clone();
    }

    let Some(stack_resource) = stack_resource else {
        return "running".to_string();
    };

    let resource_type = stack_resource.resource_type.to_ascii_lowercase();
    match (resource_type.as_str(), stack_resource.status) {
        ("storage", alien_core::ResourceStatus::Running) => "local filesystem".to_string(),
        (_, alien_core::ResourceStatus::Running) => "running (private)".to_string(),
        _ => crate::ui::format_resource_status(stack_resource.status)
            .to_ascii_lowercase()
            .replace(' ', "-"),
    }
}

fn is_local_private_url(url: &str) -> bool {
    url.starts_with("http://localhost:")
        || url.starts_with("https://localhost:")
        || url.starts_with("http://127.0.0.1:")
        || url.starts_with("https://127.0.0.1:")
}

/// Non-TTY: print one line per deployment state transition. Print full resource
/// listing on initial ready, then just status changes afterward.
fn print_deployment_log_updates(
    states: &[commands::DevDeploymentLiveState],
    last_printed: &mut std::collections::HashMap<String, alien_core::DeploymentStatus>,
    printed_ready_resources: &mut std::collections::HashSet<String>,
) {
    for state in states {
        let prev = last_printed.get(&state.deployment_name).copied();
        let changed = prev.map_or(true, |prev| prev != state.status);

        if !changed {
            continue;
        }

        last_printed.insert(state.deployment_name.clone(), state.status);

        if state.status == alien_core::DeploymentStatus::Running
            && printed_ready_resources.insert(state.deployment_name.clone())
        {
            // First time this deployment is ready — print full resource listing
            println!(
                "{}: {}",
                state.deployment_name,
                format_deployment_status(state.status)
            );
            let mut resource_names = std::collections::BTreeSet::new();
            resource_names.extend(state.resources.keys().cloned());
            if let Some(stack_state) = &state.stack_state {
                resource_names.extend(stack_state.resources.keys().cloned());
            }
            for name in resource_names {
                let public_resource = state.resources.get(&name);
                let stack_resource = state
                    .stack_state
                    .as_ref()
                    .and_then(|stack| stack.resources.get(&name));
                println!(
                    "  {}: {}",
                    name,
                    format_dev_resource_value(&name, public_resource, stack_resource)
                );
            }
        } else if state.status.is_failed() {
            let error = state.error.as_ref().and_then(deployment_error_message);
            match error {
                Some(msg) => println!(
                    "{}: {} ({})",
                    state.deployment_name,
                    format_deployment_status(state.status),
                    msg
                ),
                None => println!(
                    "{}: {}",
                    state.deployment_name,
                    format_deployment_status(state.status)
                ),
            }
        } else {
            println!(
                "{}: {}",
                state.deployment_name,
                format_deployment_status(state.status)
            );
        }
    }
}

fn print_dev_actions_footer_non_tty() {
    println!();
    println!(
        "{} {} {}  {} {} {}  {} {} {}",
        command("alien dev release"),
        dim_label("→"),
        dim_label("push changes"),
        command("alien dev deploy"),
        dim_label("→"),
        dim_label("new deployment"),
        dim_label("Ctrl+C"),
        dim_label("→"),
        dim_label("stop")
    );
}

pub async fn run_cli(cli: Cli) -> Result<()> {
    let wants_json_output = cli.wants_json_output();
    let ui_command = match &cli.command {
        Some(Commands::Build(_)) => Some(UiCommandKind::Build),
        Some(Commands::Release(_)) => Some(UiCommandKind::Release),
        _ => None,
    };

    if let Some(dir) = &cli.dir {
        env::set_current_dir(dir)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "change directory".to_string(),
                file_path: dir.clone(),
                reason: "Failed to change to specified directory".to_string(),
            })?;
    }

    setup_tracing();

    // Handle init command early — it doesn't need execution context
    if let Some(Commands::Init(args)) = cli.command {
        return init_task(args).await;
    }

    // Handle serve command early — it starts a standalone manager and doesn't
    // need the CLI execution context.
    if let Some(Commands::Serve(args)) = cli.command {
        return serve_task(args).await;
    }

    // Handle dev command early — it creates its own execution context.
    if let Some(Commands::Dev(dev_cmd)) = cli.command {
        return handle_dev_command(dev_cmd).await;
    }

    let ctx = if let Ok(server_url) = env::var("ALIEN_MANAGER_URL") {
        let api_key = cli
            .api_key
            .clone()
            .or_else(|| env::var("ALIEN_API_KEY").ok())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "ALIEN_API_KEY is required when ALIEN_MANAGER_URL is set.".to_string(),
                })
            })?;

        ExecutionMode::Standalone {
            server_url,
            api_key,
        }
    } else {
        #[cfg(feature = "platform")]
        {
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
        }
        #[cfg(not(feature = "platform"))]
        {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "No manager URL configured. Export ALIEN_MANAGER_URL=http://localhost:8080 to target a standalone manager.".to_string(),
            }));
        }
    };

    let run = || async {
        match cli.command {
            None => {
                let current_dir = get_current_dir()?;
                print_cli_banner(&current_dir);
                let mut command = Cli::command();
                command.print_long_help().into_alien_error().context(
                    ErrorData::FileOperationFailed {
                        operation: "write".to_string(),
                        file_path: "stdout".to_string(),
                        reason: "Failed to print CLI help".to_string(),
                    },
                )?;
                println!();
            }
            Some(Commands::Init(_)) => unreachable!("handled before ctx resolution"),
            Some(Commands::Serve(_)) => unreachable!("handled before ctx resolution"),
            Some(Commands::Build(args)) => build_command(args).await?,
            Some(Commands::Release(args)) => release_command(args, ctx).await?,
            Some(Commands::Onboard(args)) => onboard_task(args, ctx).await?,
            Some(Commands::Deployments(args)) => deployments_task(args, ctx).await?,
            Some(Commands::Deploy(args)) => deploy_task(args, ctx).await?,
            Some(Commands::Destroy(args)) => destroy_task(args, ctx).await?,
            Some(Commands::Vault(args)) => vault_remote_task(args, ctx).await?,
            Some(Commands::Commands(args)) => commands_task(args, ctx).await?,
            Some(Commands::Dev(dev_cmd)) => handle_dev_command(dev_cmd).await?,
            Some(Commands::Whoami(args)) => whoami_task(args, ctx).await?,
            #[cfg(feature = "platform")]
            Some(Commands::Platform(command)) => match command {
                PlatformCommand::Login(args) => login_task(args, ctx).await?,
                PlatformCommand::Logout(args) => logout_task(args).await?,
                PlatformCommand::Workspaces(args) => workspace_task(args, ctx).await?,
                PlatformCommand::Projects(args) => project_task(args, ctx).await?,
                PlatformCommand::Link(args) => link_task(args, ctx).await?,
                PlatformCommand::Unlink(args) => unlink_task(args).await?,
            },
            #[cfg(feature = "platform")]
            Some(Commands::Manager(args)) => manager_task(args, ctx).await?,
        }

        Ok(())
    };

    if let Some(event_bus) = event_bus_for_command(ui_command, wants_json_output) {
        event_bus.run(run).await
    } else {
        run().await
    }
}

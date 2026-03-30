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
    build_and_post_release_simple, build_command, build_dev_status, deploy_task, deployments_task,
    destroy_task, ensure_server_running_for_dev_session, ensure_server_running_with_env,
    onboard_task, prepare_dev_session_deployment, release_command, vault_remote_task, vault_task,
    wait_for_dev_deployment_ready_with_progress, whoami_task, write_dev_status, BuildArgs,
    CliEnvVar, DeployArgs, DeploymentsArgs, DestroyArgs, OnboardArgs, ReleaseArgs, WhoamiArgs,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{
    accent, command, contextual_heading, dim_label, event_bus_for_command,
    format_deployment_status, make_table, print_cli_banner, print_table, success_line, FixedSteps,
    UiCommandKind,
};
use alien_error::{AlienError, Context, IntoAlienError};
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
                EnvFilter::new("warn,oci_tar_builder=error")
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

    println!(
        "{}",
        contextual_heading("Starting local development for", &app_name, &[])
    );
    println!(
        "{} {}",
        dim_label("Manager"),
        accent(&format!("http://localhost:{port}"))
    );

    let steps = FixedSteps::new(&[
        "Manager",
        "Release",
        "Prepare deployment",
        "Wait for deployment",
    ]);

    if let Some(status_file) = &status_file {
        write_dev_status(
            status_file,
            &build_dev_status(port, alien_core::DevStatusState::Initializing, None, None),
        )?;
    }

    let result = async {
        steps.activate(0, Some("starting".to_string()));
        ensure_server_running_for_dev_session(port, status_file.clone(), user_env_vars).await?;
        steps.complete(0, Some("ready".to_string()));

        steps.activate(
            1,
            Some(if skip_build {
                "using existing build artifacts".to_string()
            } else {
                "building local app".to_string()
            }),
        );
        let release_id =
            build_and_post_release_simple(&current_dir, port, skip_build, config_file.as_ref())
                .await?;
        steps.complete(1, Some(release_id.clone()));

        steps.activate(2, Some(deployment_name.clone()));
        let deployment_id =
            prepare_dev_session_deployment(&deployment_name, port, core_env_vars.clone()).await?;
        steps.complete(2, Some(format!("{deployment_name} ({deployment_id})")));

        steps.activate(3, Some(deployment_name.clone()));
        let snapshot = wait_for_dev_deployment_ready_with_progress(
            port,
            &deployment_name,
            status_file.as_ref(),
            |status| {
                steps.activate(
                    3,
                    Some(format!(
                        "{} ({})",
                        deployment_name,
                        format_deployment_status(status).to_ascii_lowercase()
                    )),
                );
            },
        )
        .await?;
        steps.complete(3, Some(format!("{deployment_name} ready")));

        if let Some(status_file) = &status_file {
            write_dev_status(
                status_file,
                &build_dev_status(
                    port,
                    alien_core::DevStatusState::Ready,
                    Some(&snapshot),
                    None,
                ),
            )?;
        }

        print_dev_ready_summary(port, &snapshot);

        tokio::signal::ctrl_c()
            .await
            .into_alien_error()
            .context(ErrorData::ServerStartFailed {
                reason: "Failed to wait for Ctrl+C".to_string(),
            })?;

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
            "Starting",
            "local manager",
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

fn print_dev_ready_summary(port: u16, snapshot: &commands::DevDeploymentSnapshot) {
    println!(
        "{}",
        contextual_heading(
            "Local development ready for",
            &snapshot.deployment_name,
            &[]
        )
    );
    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        snapshot.deployment_name,
        snapshot.deployment_id
    );

    let manager_url = format!("http://localhost:{port}");
    if snapshot.commands_url == manager_url {
        println!("{} {}", dim_label("Manager"), accent(&manager_url));
    } else {
        println!(
            "{} {}",
            dim_label("Commands"),
            accent(&snapshot.commands_url)
        );
        println!("{} {}", dim_label("Manager"), accent(&manager_url));
    }

    if snapshot.resources.is_empty() {
        println!(
            "{}",
            dim_label("No public resource URLs were reported yet.")
        );
    } else {
        println!("{}", dim_label("Resources"));
        let mut table = make_table(&["Name", "Type", "URL"]);
        let mut resources: Vec<_> = snapshot.resources.iter().collect();
        resources.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
        for (name, resource) in resources {
            table.add_row(vec![
                name.to_string(),
                resource
                    .resource_type
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
                resource.url.clone(),
            ]);
        }
        print_table(table);
    }

    println!("{}", dim_label("Try next"));
    println!("  {}", command("alien dev deployments ls"));
    println!("  {}", command("alien dev release"));
    println!(
        "  {}",
        command("alien dev deploy --name <deployment> --platform local")
    );
    println!("{}", dim_label("Press Ctrl+C to stop this session."));
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
            Some(Commands::Build(args)) => build_command(args).await?,
            Some(Commands::Release(args)) => release_command(args, ctx).await?,
            Some(Commands::Onboard(args)) => onboard_task(args, ctx).await?,
            Some(Commands::Deployments(args)) => deployments_task(args, ctx).await?,
            Some(Commands::Deploy(args)) => deploy_task(args, ctx).await?,
            Some(Commands::Destroy(args)) => destroy_task(args, ctx).await?,
            Some(Commands::Vault(args)) => vault_remote_task(args, ctx).await?,
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

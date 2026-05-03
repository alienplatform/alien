//! Command-line entry point for the agent. Extracted into the lib so
//! downstream distributions can register additional controllers via
//! [`init_hook`] and then call [`cli_main`] without duplicating CLI
//! parsing, signal handling, panic hooks, or the Windows service shim.

use crate::error::{ErrorData, Result};
use crate::{run_agent_with_cancel, AgentConfig, InstanceLock};
use alien_core::embedded_config::{load_embedded_config, AgentConfig as EmbeddedAgentConfig};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser, Debug)]
#[command(
    name = "alien-agent",
    about = "Alien Agent - Continuous deployment agent (pull model)",
    long_about = "Run the Agent for continuous deployment using the pull model.

The Agent:
- Syncs with the manager every 30 seconds
- Runs alien-deployment::step() locally when updates are available
- Collects and forwards telemetry to the manager
- Supports offline/airgapped operation with state persistence",
    after_help = "Secrets are loaded from files or environment variables only — \
                  CLI flags for tokens and encryption keys were removed because \
                  argv is visible in `ps` / `/proc/<pid>/cmdline`."
)]
pub struct Args {
    #[arg(long, env = "PLATFORM", value_parser = parse_platform)]
    pub platform: Platform,

    #[arg(long, env = "SYNC_URL")]
    pub sync_url: Option<String>,

    #[arg(long, env = "SYNC_TOKEN_FILE")]
    pub sync_token_file: Option<PathBuf>,

    #[arg(long, env = "AGENT_NAME")]
    pub agent_name: Option<String>,

    #[arg(long, env = "DATA_DIR")]
    pub data_dir: Option<String>,

    #[arg(long, env = "AGENT_ENCRYPTION_KEY_FILE")]
    pub encryption_key_file: Option<PathBuf>,

    #[arg(long, env = "KUBERNETES_NAMESPACE")]
    pub namespace: Option<String>,

    #[arg(long, env = "EXTERNAL_BINDINGS")]
    pub external_bindings: Option<String>,

    #[arg(long, env = "PUBLIC_URLS")]
    pub public_urls: Option<String>,

    #[arg(long, env = "STACK_SETTINGS")]
    pub stack_settings: Option<String>,

    #[arg(long, env = "SYNC_INTERVAL", default_value = "30")]
    pub sync_interval: u64,

    #[arg(long, env = "OTLP_PORT", default_value = "4318")]
    pub otlp_port: u16,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long, hide = true)]
    pub service: bool,
}

/// Hook callback that runs once before the agent's deployment loop starts.
/// Downstream distributions register additional controller factories here.
pub type InitHook = fn();

const NOOP_INIT: InitHook = || {};

/// CLI entry point. Parses args, sets up tracing/panic hooks, runs the
/// agent until SIGTERM/SIGINT/Ctrl-C. Calls `init_hook` once before the
/// deployment loop starts. The OSS `alien-agent` binary passes a no-op
/// hook; downstream binaries that wrap this entry point pass a hook that
/// registers their additional controllers.
pub fn cli_main_with_hook(init_hook: InitHook) {
    let args = Args::parse();

    #[cfg(windows)]
    if args.service {
        windows_entry::run_as_service(init_hook);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    if let Err(e) = rt.block_on(run(args, init_hook)) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Convenience wrapper: [`cli_main_with_hook`] with a no-op init hook.
pub fn cli_main() {
    cli_main_with_hook(NOOP_INIT);
}

async fn run(args: Args, init_hook: InitHook) -> Result<()> {
    let embedded_config: Option<EmbeddedAgentConfig> = load_embedded_config().ok().flatten();

    setup_tracing(args.verbose);

    // Run the extension hook before any agent state is touched. Idempotent.
    init_hook();

    let data_dir = args
        .data_dir
        .unwrap_or_else(|| "/var/lib/alien-agent".to_string());
    let data_dir_path = PathBuf::from(&data_dir);

    let _lock = InstanceLock::acquire(&data_dir_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to acquire instance lock in '{}'", data_dir),
        })?;

    install_panic_hook(&data_dir_path);

    info!("Starting alien-agent (pull model)");
    info!("   Platform: {}", args.platform);
    info!("   Data directory: {}", data_dir);

    let encryption_key = load_encryption_key(args.encryption_key_file.as_deref()).await?;
    let db = crate::db::AgentDb::new(&data_dir, &encryption_key).await?;

    let cli_sync_token = load_sync_token(args.sync_token_file.as_deref()).await?;

    let effective_sync_url = args
        .sync_url
        .or_else(|| embedded_config.as_ref().and_then(|c| c.manager_url.clone()));
    let effective_sync_token = cli_sync_token
        .or_else(|| embedded_config.as_ref().and_then(|c| c.token.clone()));

    let sync_config = match (effective_sync_url, effective_sync_token) {
        (Some(sync_url_str), Some(mut sync_token)) => {
            let sync_url = sync_url_str
                .parse::<url::Url>()
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid sync URL".to_string(),
                })?;

            info!("   Sync URL: {}", sync_url);

            if let Some(stored_deployment_id) = db.get_deployment_id().await? {
                info!("   Using stored deployment ID: {}", stored_deployment_id);
            } else {
                info!("   First startup, initializing with manager...");

                let (initialized_deployment_id, deployment_token) = initialize_with_manager(
                    &sync_url,
                    &sync_token,
                    args.platform,
                    args.agent_name.as_deref(),
                )
                .await?;

                db.set_deployment_id(&initialized_deployment_id).await?;

                if let Some(ref dt) = deployment_token {
                    info!("   Received deployment-scoped token from manager");
                    sync_token = dt.clone();
                }

                info!(
                    "   Initialized successfully, deployment ID: {}",
                    initialized_deployment_id
                );
            }

            Some(crate::SyncConfig {
                url: sync_url,
                token: sync_token,
            })
        }
        (None, None) => {
            warn!("   Running in airgapped mode (no sync server connection)");
            None
        }
        (Some(_), None) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Sync token is required when sync URL is provided".to_string(),
            }));
        }
        (None, Some(_)) => {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "Sync URL is required when sync token is provided".to_string(),
            }));
        }
    };

    let external_bindings = parse_json_opt::<alien_core::ExternalBindings>(
        args.external_bindings,
        "external bindings",
    )?;
    let public_urls = parse_json_opt::<HashMap<String, String>>(args.public_urls, "public URLs")?;
    let mut stack_settings =
        parse_json_opt::<alien_core::StackSettings>(args.stack_settings, "stack settings")?
            .unwrap_or_default();

    if let Some(bindings) = external_bindings {
        stack_settings.external_bindings = Some(bindings);
    }

    let agent_config = AgentConfig::builder()
        .platform(args.platform)
        .maybe_sync(sync_config)
        .data_dir(data_dir)
        .encryption_key(encryption_key)
        .sync_interval_seconds(args.sync_interval)
        .otlp_server_port(args.otlp_port)
        .maybe_namespace(args.namespace)
        .maybe_public_urls(public_urls)
        .stack_settings(stack_settings)
        .build();

    let cancel = CancellationToken::new();

    let signal_cancel = cancel.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        info!("Received shutdown signal");
        signal_cancel.cancel();
    });

    let service_provider: Option<std::sync::Arc<dyn alien_infra::PlatformServiceProvider>> =
        if args.platform == alien_core::Platform::Local {
            let data_path = std::path::Path::new(&agent_config.data_dir);
            let local_bindings =
                alien_local::LocalBindingsProvider::new(data_path).context(
                    ErrorData::ConfigurationError {
                        message: "Failed to create LocalBindingsProvider".to_string(),
                    },
                )?;
            Some(std::sync::Arc::new(
                alien_infra::DefaultPlatformServiceProvider::with_local_bindings(local_bindings),
            ))
        } else {
            None
        };

    run_agent_with_cancel(agent_config, service_provider, cancel).await?;

    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => {},
        _ = sigint.recv() => {},
    }
}

#[cfg(windows)]
async fn wait_for_shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}

fn install_panic_hook(data_dir: &PathBuf) {
    let panic_log_path = data_dir.join("panic.log");
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let timestamp = chrono::Utc::now().to_rfc3339();
        let msg = format!("[{}] PANIC at {}: {}\n", timestamp, location, payload);

        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_log_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(msg.as_bytes())
            });

        eprintln!("{}", msg);
    }));
}

#[cfg(windows)]
mod windows_entry {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use tracing::error;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    const SERVICE_NAME: &str = "alien-agent";

    define_windows_service!(ffi_service_main, service_main);

    /// Per-process slot holding the init hook the binary registered before
    /// entering the service dispatcher. Only one agent runs per process so a
    /// single static slot is sufficient.
    static INIT_HOOK: std::sync::Mutex<Option<InitHook>> = std::sync::Mutex::new(None);

    pub fn run_as_service(init_hook: InitHook) -> ! {
        *INIT_HOOK.lock().expect("init hook lock") = Some(init_hook);
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .expect("failed to start service dispatcher");
        std::process::exit(0);
    }

    fn service_main(_args: Vec<std::ffi::OsString>) {
        let (stop_tx, stop_rx) = mpsc::channel();

        let status_handle =
            service_control_handler::register(SERVICE_NAME, move |control| match control {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = stop_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            })
            .expect("failed to register service control handler");

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .expect("failed to set running status");

        let init_hook = INIT_HOOK
            .lock()
            .expect("init hook lock")
            .unwrap_or(super::NOOP_INIT);
        let args = Args::parse();
        let cancel = CancellationToken::new();
        let cancel_for_stop = cancel.clone();

        std::thread::spawn(move || {
            let _ = stop_rx.recv();
            cancel_for_stop.cancel();
        });

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        let exit_code = match rt.block_on(super::run(args, init_hook)) {
            Ok(()) => 0,
            Err(e) => {
                error!(error = %e, "Agent exited with error");
                1
            }
        };

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(exit_code),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    }
}

async fn initialize_with_manager(
    sync_url: &url::Url,
    token: &str,
    platform: Platform,
    agent_name: Option<&str>,
) -> Result<(String, Option<String>)> {
    use alien_manager_api::types::Platform as SdkPlatform;
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
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-agent"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    let base_url = sync_url.as_str().trim_end_matches('/');
    let client = alien_manager_api::Client::new_with_client(base_url, http_client);

    let sdk_platform = match platform {
        Platform::Aws => SdkPlatform::Aws,
        Platform::Gcp => SdkPlatform::Gcp,
        Platform::Azure => SdkPlatform::Azure,
        Platform::Kubernetes => SdkPlatform::Kubernetes,
        Platform::Local => SdkPlatform::Local,
        Platform::Test => SdkPlatform::Test,
    };

    let default_name = agent_name.map(|s| s.to_string()).or_else(|| {
        std::env::var("HOSTNAME")
            .ok()
            .or_else(|| hostname::get().ok().and_then(|h| h.into_string().ok()))
    });

    let mut builder = client
        .initialize()
        .body_map(|b| b.platform(Some(sdk_platform)));

    if let Some(name) = default_name {
        builder = builder.body_map(|b| b.name(name));
    }

    let response = builder
        .send()
        .await
        .map_err(alien_manager_api::convert_sdk_error)
        .context(ErrorData::ConfigurationError {
            message: "Failed to call initialize endpoint".to_string(),
        })?;

    let init_response = response.into_inner();

    Ok((init_response.deployment_id, init_response.token))
}

fn setup_tracing(verbose: bool) {
    let filter = if verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("debug,turso_core=warn,hyper_util=warn"))
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,turso_core=warn,hyper_util=warn"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}

fn parse_platform(s: &str) -> std::result::Result<Platform, String> {
    match s.to_lowercase().as_str() {
        "aws" => Ok(Platform::Aws),
        "gcp" => Ok(Platform::Gcp),
        "azure" => Ok(Platform::Azure),
        "kubernetes" | "k8s" => Ok(Platform::Kubernetes),
        "local" => Ok(Platform::Local),
        "test" => Ok(Platform::Test),
        _ => Err(format!(
            "Invalid platform: {}. Must be one of: aws, gcp, azure, kubernetes, local, test",
            s
        )),
    }
}

fn parse_json_opt<T: serde::de::DeserializeOwned>(
    json_str: Option<String>,
    label: &str,
) -> Result<Option<T>> {
    match json_str {
        Some(json) => {
            let value: T = serde_json::from_str(&json).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: format!("Invalid {} JSON", label),
                },
            )?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

async fn read_secret_file(path: &std::path::Path, label: &str) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata =
            tokio::fs::metadata(path)
                .await
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: format!("Failed to stat {} file '{}'", label, path.display()),
                })?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "{} file '{}' has permissions {:o}; required 0600",
                    label,
                    path.display(),
                    mode
                ),
            }));
        }
    }

    let contents =
        tokio::fs::read_to_string(path)
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to read {} file '{}'", label, path.display()),
            })?;
    let trimmed = contents.trim().to_string();
    if trimmed.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("{} file '{}' is empty", label, path.display()),
        }));
    }
    Ok(trimmed)
}

async fn load_encryption_key(file: Option<&std::path::Path>) -> Result<String> {
    if let Some(path) = file {
        return read_secret_file(path, "encryption key").await;
    }
    match std::env::var("AGENT_ENCRYPTION_KEY") {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Encryption key required: pass --encryption-key-file <PATH> (mode 0600) or set AGENT_ENCRYPTION_KEY".to_string(),
        })),
    }
}

async fn load_sync_token(file: Option<&std::path::Path>) -> Result<Option<String>> {
    if let Some(path) = file {
        return Ok(Some(read_secret_file(path, "sync token").await?));
    }
    match std::env::var("SYNC_TOKEN") {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        _ => Ok(None),
    }
}

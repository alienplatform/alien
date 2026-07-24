//! Alien Worker Runtime - translates platform invocations into Worker tasks.
//!
//! The runtime:
//! 1. Starts the Worker app protocol server (Control + WaitUntil)
//! 2. Loads application secrets and runtime-only telemetry credentials
//! 3. Starts the application subprocess
//! 4. Waits for app to register HTTP port
//! 5. Enables authenticated Worker command push when configured
//! 6. Starts the appropriate transport

use std::{collections::HashMap, process::Stdio, sync::Arc};

use alien_bindings::BindingsProvider;
use alien_core::{
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_CURRENT_WORKER_BINDING_NAME, ENV_ALIEN_DEPLOYMENT_ID,
    ENV_ALIEN_RUNTIME_SECRETS, ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT,
    ENV_ALIEN_WORKER_GRPC_ADDRESS,
};
use alien_error::{AlienError, Context};
use alien_worker_protocol::{run_grpc_server, ControlGrpcServer, WaitUntilGrpcServer};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    signal,
    sync::{broadcast, OnceCell},
    task::JoinHandle,
};
use tracing::{debug, error, info, warn};

use crate::{
    config::{RuntimeConfig, TransportType},
    error::{ErrorData, Result},
    otlp::{flush_otlp_logs, init_otlp_logging_from_config, shutdown_otlp_logs},
    transports::{
        cloudrun::CloudRunTransport, containerapp::ContainerAppTransport, local::LocalTransport,
    },
};

const ENV_ALIEN_BINDINGS_GRPC_ADDRESS: &str = "ALIEN_BINDINGS_GRPC_ADDRESS";
const ENV_ALIEN_BINDINGS_MODE: &str = "ALIEN_BINDINGS_MODE";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSecretsConfig {
    otlp_logs_auth_header: Option<String>,
    otlp_metrics_auth_header: Option<String>,
    hash: String,
}

#[derive(Clone)]
struct CommandPushConfig {
    token: String,
    deployment_id: String,
    worker_resource_id: String,
}

/// Global state for WaitUntilGrpcServer
static WAIT_UNTIL_SERVER: OnceCell<Arc<WaitUntilGrpcServer>> = OnceCell::const_new();

/// Global state for ControlGrpcServer
static CONTROL_SERVER: OnceCell<Arc<ControlGrpcServer>> = OnceCell::const_new();

/// Get the global WaitUntilGrpcServer handle
pub fn get_wait_until_server() -> Option<Arc<WaitUntilGrpcServer>> {
    WAIT_UNTIL_SERVER.get().cloned()
}

/// Get the global ControlGrpcServer handle
pub fn get_control_server() -> Option<Arc<ControlGrpcServer>> {
    CONTROL_SERVER.get().cloned()
}

/// Dependencies used to start the Worker runtime.
pub enum RuntimeDependencies {
    /// Create the secret provider from environment variables and start the
    /// Worker app protocol server (cloud platform).
    FromEnvironment,

    /// Use a custom direct provider for Worker secret projection and start the
    /// Worker app protocol server (local platform).
    Provider(Arc<dyn alien_bindings::BindingsProviderApi>),

    /// Use externally provided Worker app protocol handles (testing).
    ExternalWorkerProtocol {
        wait_until_server: Arc<WaitUntilGrpcServer>,
        control_server: Arc<ControlGrpcServer>,
    },
}

struct PrestartedTransport {
    shutdown_tx: broadcast::Sender<()>,
    handle: JoinHandle<Result<()>>,
}

fn prestart_lambda_transport(
    config: &RuntimeConfig,
    control_server: Arc<ControlGrpcServer>,
) -> Result<PrestartedTransport> {
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let handle = spawn_transport(
        TransportType::Lambda,
        config.transport_port,
        config.lambda_mode,
        config.command_timeout,
        control_server,
        None,
        None,
        shutdown_rx,
    )?;
    Ok(PrestartedTransport {
        shutdown_tx,
        handle,
    })
}

async fn stop_prestarted_transport(mut transport: PrestartedTransport) {
    let _ = transport.shutdown_tx.send(());
    if tokio::time::timeout(std::time::Duration::from_secs(1), &mut transport.handle)
        .await
        .is_err()
    {
        transport.handle.abort();
        let _ = transport.handle.await;
    }
}

/// Run the Alien Worker Runtime.
///
/// # Arguments
/// * `config` - Runtime configuration
/// * `shutdown_rx` - Shutdown signal receiver
/// * `dependencies` - Secret-provider and Worker-protocol dependencies
pub async fn run(
    config: RuntimeConfig,
    shutdown_rx: broadcast::Receiver<()>,
    dependencies: RuntimeDependencies,
) -> Result<()> {
    config.validate()?;

    // Note: For standalone binary (main.rs), init_tracing() is called before run()
    // For embedded runtime (LocalFunctionManager), parent already initialized tracing
    // LogExporter config tells stream_output() how to handle logs

    info!(
        transport = ?config.transport,
        command = ?config.command,
        log_exporter = ?config.log_exporter,
        "Starting Alien Worker Runtime"
    );

    // 1. Start the Worker app protocol server (or use external handles for testing).
    let (wait_until_server, control_server, bindings_provider) = match dependencies {
        RuntimeDependencies::ExternalWorkerProtocol {
            wait_until_server,
            control_server,
        } => {
            info!("Using externally provided Worker app protocol handles (testing mode)");
            // No provider available in test mode
            (wait_until_server, control_server, None)
        }
        RuntimeDependencies::Provider(provider) => {
            info!("Using custom provider for Worker secret projection (local platform)");
            let (wait, control, prov) =
                start_worker_protocol_server(&config.worker_grpc_address, provider).await?;
            (wait, control, Some(prov))
        }
        RuntimeDependencies::FromEnvironment => {
            info!("Creating lazy provider for Worker secret projection (cloud platform)");
            let provider = Arc::new(
                BindingsProvider::from_env_lazy(std::env::vars().collect()).context(
                    ErrorData::SecretProviderInitializationFailed {
                        message: "Failed to create bindings provider".to_string(),
                    },
                )?,
            );
            let (wait, control, prov) =
                start_worker_protocol_server(&config.worker_grpc_address, provider).await?;
            (wait, control, Some(prov))
        }
    };

    // Store in global state
    let _ = WAIT_UNTIL_SERVER.set(wait_until_server.clone());
    let _ = CONTROL_SERVER.set(control_server.clone());

    // Lambda must register its extension and begin Runtime API polling before
    // secret loading and application startup. AWS gives on-demand functions
    // only 10 seconds for Init, including container/bootstrap overhead outside
    // this process. Invocation handlers wait for application readiness within
    // the invocation deadline, so transport startup must never wait for it.
    let prestarted_transport = if config.transport == TransportType::Lambda {
        Some(prestart_lambda_transport(&config, control_server.clone())?)
    } else {
        None
    };

    let application_startup: Result<(Child, Option<CommandPushConfig>)> = async {
        // 2. Load user secrets from vault if ALIEN_SECRETS is present.
        let secrets = if let Some(ref provider) = bindings_provider {
            if let Some(alien_secrets_json) = config.env_vars.get(ENV_ALIEN_SECRETS) {
                info!("Loading secrets from vault before starting application");
                crate::secrets::load_secrets_from_vault(&**provider, alien_secrets_json).await?
            } else if let Ok(alien_secrets_json) = std::env::var(ENV_ALIEN_SECRETS) {
                info!("Loading secrets from vault before starting application (from process env)");
                crate::secrets::load_secrets_from_vault(&**provider, &alien_secrets_json).await?
            } else {
                debug!("No ALIEN_SECRETS found in config or process env");
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };

        let runtime_secrets = if let Some(ref provider) = bindings_provider {
            load_runtime_secrets(&config, &**provider).await?
        } else {
            HashMap::new()
        };

        let log_exporter = config
            .log_exporter
            .clone()
            .with_runtime_secrets(&runtime_secrets);
        if let Some(otlp_config) = log_exporter.to_otlp_config() {
            init_otlp_logging_from_config(otlp_config)?;
        }

        let command_push = command_push_config(&config, &secrets)?;
        let child = start_application(&config, &secrets, log_exporter).await?;
        Ok((child, command_push))
    }
    .await;

    let (mut child, command_push) = match application_startup {
        Ok(started) => started,
        Err(error) => {
            if let Some(transport) = prestarted_transport {
                stop_prestarted_transport(transport).await;
            }
            return Err(error);
        }
    };

    // Non-Lambda transports still take a startup snapshot. Lambda resolves
    // readiness dynamically for every invocation.
    let app_http_port = if config.transport == TransportType::Lambda {
        None
    } else {
        info!("Waiting for application to register HTTP server...");
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            control_server.wait_for_http_server(),
        )
        .await
        {
            Ok(Some(port)) => {
                info!(port = port, "Application registered HTTP server");
                Some(port)
            }
            Ok(None) => {
                warn!("Application did not register HTTP server");
                None
            }
            Err(_) => {
                warn!("Timeout waiting for HTTP server registration");
                None
            }
        }
    };

    if config.transport != TransportType::Lambda {
        info!("Waiting for application to subscribe to task stream...");
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            control_server.wait_for_task_subscriber(),
        )
        .await
        {
            Ok(_) => {
                info!("Application subscribed to task stream");
            }
            Err(_) => {
                warn!("Timeout waiting for task stream subscriber — commands may fail");
            }
        }
    }

    // 5. Local/Kubernetes Workers receive authenticated command pushes on a
    // runtime-owned HTTP path. Absence of the token leaves that path disabled.
    // 6. Start transport and run main loop
    let result = run_transport(
        &config,
        control_server,
        app_http_port,
        command_push,
        wait_until_server,
        &mut child,
        shutdown_rx,
        prestarted_transport,
    )
    .await;

    if let Err(e) = shutdown_otlp_logs().await {
        warn!(error = %e, "Failed to shutdown OTLP logs");
    }

    result
}

/// Spawn and run the appropriate transport, then wait for completion.
async fn run_transport(
    config: &RuntimeConfig,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_push: Option<CommandPushConfig>,
    wait_until_server: Arc<WaitUntilGrpcServer>,
    child: &mut Child,
    mut shutdown_rx: broadcast::Receiver<()>,
    prestarted_transport: Option<PrestartedTransport>,
) -> Result<()> {
    // Own the transport shutdown channel here. This lets every branch that
    // wins the lifecycle race stop intake and await the transport task instead
    // of dropping its JoinHandle (which would detach it in Tokio).
    let (transport_shutdown_tx, mut transport_handle) =
        if let Some(prestarted) = prestarted_transport {
            (prestarted.shutdown_tx, prestarted.handle)
        } else {
            let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
            let handle = spawn_transport(
                config.transport,
                config.transport_port,
                config.lambda_mode,
                config.command_timeout,
                control_server,
                app_http_port,
                command_push,
                shutdown_rx,
            )?;
            (shutdown_tx, handle)
        };

    // Wait for shutdown, child exit, or transport completion
    tokio::select! {
        shutdown_result = shutdown_rx.recv() => {
            let _ = transport_shutdown_tx.send(());
            let transport_result = await_transport(&mut transport_handle).await;
            let child_result = handle_shutdown(shutdown_result, wait_until_server, child).await;
            transport_result.and(child_result)
        }

        child_status = child.wait() => {
            let child_result = handle_child_exit(child_status);
            let _ = transport_shutdown_tx.send(());
            let transport_result = await_transport(&mut transport_handle).await;
            child_result.and(transport_result)
        }

        transport_result = &mut transport_handle => {
            let transport_result = transport_result_value(transport_result);
            // A transport startup failure or unexpected exit must not leave
            // the application child alive after the runtime returns.
            let child_result = graceful_shutdown(wait_until_server, child).await;
            transport_result.and(child_result)
        }
    }
}

async fn await_transport(transport_handle: &mut JoinHandle<Result<()>>) -> Result<()> {
    transport_result_value(transport_handle.await)
}

fn transport_result_value(
    transport_result: std::result::Result<Result<()>, tokio::task::JoinError>,
) -> Result<()> {
    match transport_result {
        Ok(Ok(_)) => {
            info!("Transport exited");
            Ok(())
        }
        Ok(Err(e)) => {
            error!(error = %e, "Transport error");
            Err(e)
        }
        Err(e) => {
            error!(error = %e, "Transport task panicked");
            Err(AlienError::new(ErrorData::Other {
                message: format!("Transport panicked: {}", e),
            }))
        }
    }
}

/// Spawn the appropriate transport as a tokio task.
fn spawn_transport(
    transport_type: TransportType,
    transport_port: u16,
    lambda_mode: crate::config::LambdaMode,
    command_timeout: std::time::Duration,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_push: Option<CommandPushConfig>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<JoinHandle<Result<()>>> {
    match transport_type {
        TransportType::CloudRun => {
            let mut transport = CloudRunTransport::new(transport_port, control_server, shutdown_rx)
                .with_command_timeout(command_timeout);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::ContainerApp => {
            let mut transport =
                ContainerAppTransport::new(transport_port, control_server, shutdown_rx)
                    .with_command_timeout(command_timeout);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::Http => {
            let mut transport =
                LocalTransport::exposed(transport_port, control_server, shutdown_rx)
                    .with_command_timeout(command_timeout);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            if let Some(config) = command_push {
                transport = transport.with_command_push(
                    config.token,
                    config.deployment_id,
                    config.worker_resource_id,
                );
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::Local => {
            let mut transport = LocalTransport::new(transport_port, control_server, shutdown_rx)
                .with_command_timeout(command_timeout);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            if let Some(config) = command_push {
                transport = transport.with_command_push(
                    config.token,
                    config.deployment_id,
                    config.worker_resource_id,
                );
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        #[cfg(feature = "aws")]
        TransportType::Lambda => {
            use crate::transports::lambda::LambdaTransport;

            let transport = LambdaTransport::new(lambda_mode, control_server);
            // Lambda polls the Runtime API and has no native shutdown receiver.
            // Race that owned future against the same internal shutdown signal
            // so run_transport can still join it without detaching/aborting.
            let mut shutdown_rx = shutdown_rx;
            Ok(tokio::spawn(async move {
                tokio::select! {
                    result = transport.run() => result,
                    _ = shutdown_rx.recv() => Ok(()),
                }
            }))
        }

        #[cfg(not(feature = "aws"))]
        TransportType::Lambda => Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: "Lambda transport requires 'aws' feature".to_string(),
            field: Some("transport".to_string()),
        })),
    }
}

/// Handle shutdown signal
async fn handle_shutdown(
    shutdown_result: std::result::Result<(), broadcast::error::RecvError>,
    wait_until_server: Arc<WaitUntilGrpcServer>,
    child: &mut Child,
) -> Result<()> {
    match shutdown_result {
        Ok(_) | Err(broadcast::error::RecvError::Closed) => {
            info!("Shutdown signal received");
        }
        Err(broadcast::error::RecvError::Lagged(_)) => {
            info!("Shutdown signal received (lagged)");
        }
    }
    graceful_shutdown(wait_until_server, child).await
}

/// Handle child process exit
fn handle_child_exit(
    child_status: std::result::Result<std::process::ExitStatus, std::io::Error>,
) -> Result<()> {
    match child_status {
        Ok(status) if status.success() => {
            info!("Application exited successfully");
            Ok(())
        }
        Ok(status) => {
            let code = status.code().unwrap_or(-1);
            Err(AlienError::new(ErrorData::ProcessFailed {
                exit_code: Some(code),
                message: format!("Application exited with code {}", code),
            }))
        }
        Err(e) => Err(AlienError::new(ErrorData::ProcessFailed {
            exit_code: None,
            message: format!("Failed to wait for process: {}", e),
        })),
    }
}

/// Start the Worker app protocol server and retain the provider used for
/// Worker secret projection.
async fn start_worker_protocol_server(
    address: &str,
    secret_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
) -> Result<(
    Arc<WaitUntilGrpcServer>,
    Arc<ControlGrpcServer>,
    Arc<dyn alien_bindings::BindingsProviderApi>,
)> {
    info!(address = %address, "Starting Worker app protocol server");

    let handles = run_grpc_server(address)
        .await
        .context(ErrorData::HandlerStartupFailed {
            message: format!("Failed to start Worker app protocol server at {address}"),
            handler_type: Some("worker-app-protocol".to_string()),
        })?;

    // A closed readiness channel means the protocol server did not start.
    if handles.readiness_receiver.await.is_err() {
        return Err(AlienError::new(ErrorData::HandlerStartupFailed {
            message: format!(
                "Worker app protocol readiness channel closed before listening at {address}"
            ),
            handler_type: Some("worker-app-protocol".to_string()),
        }));
    }
    info!(address = %address, "Worker app protocol server ready");

    // Spawn server task
    let addr = address.to_string();
    tokio::spawn(async move {
        match handles.server_task.await {
            Ok(Ok(_)) => info!(address = %addr, "Worker app protocol server exited"),
            Ok(Err(e)) => {
                error!(error = %e, address = %addr, "Worker app protocol server error")
            }
            Err(e) => {
                error!(error = %e, address = %addr, "Worker app protocol server panicked")
            }
        }
    });

    info!(address = %address, "Worker app protocol server started");
    Ok((
        handles.wait_until_server,
        handles.control_server,
        secret_provider,
    ))
}

/// Start the application subprocess with secrets loaded from vault.
///
/// Secrets are passed explicitly to avoid std::env::set_var() races in embedded runtime mode.
async fn start_application(
    config: &RuntimeConfig,
    secrets: &HashMap<String, String>,
    log_exporter: crate::config::LogExporter,
) -> Result<Child> {
    if config.command.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: "Application command is empty".to_string(),
            field: Some("command".to_string()),
        }));
    }

    info!(command = ?config.command, working_dir = ?config.working_dir, "Starting application");

    // Resolve the executable path. On Windows, CreateProcessW does NOT resolve
    // the executable relative to `current_dir` — only relative to the parent's
    // working directory. Convert relative paths (e.g. "./app") to absolute by
    // joining with the working directory.
    let program = {
        let raw = std::path::Path::new(&config.command[0]);
        if raw.is_relative() {
            if let Some(ref wd) = config.working_dir {
                wd.join(raw).to_string_lossy().to_string()
            } else {
                config.command[0].clone()
            }
        } else {
            config.command[0].clone()
        }
    };

    let mut cmd = Command::new(&program);
    // The runtime owns this subprocess. If its task is cancelled during local
    // startup or process shutdown, dropping the Child must not orphan the app.
    cmd.kill_on_drop(true);
    if config.command.len() > 1 {
        cmd.args(&config.command[1..]);
    }

    // Set working directory if specified
    if let Some(ref working_dir) = config.working_dir {
        cmd.current_dir(working_dir);
    }

    // Runtime-only credentials may be present in this process's inherited
    // environment (the normal standalone Kubernetes path). Skipping an
    // explicit `cmd.env` is not enough: child processes inherit every parent
    // variable unless it is removed from the command environment.
    for name in [ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_RUNTIME_SECRETS] {
        cmd.env_remove(name);
    }

    // Set custom environment variables
    for (key, value) in &config.env_vars {
        if runtime_only_env(key) {
            continue;
        }
        cmd.env(key, value);
    }

    // Set secrets loaded from vault
    for (key, value) in secrets {
        if runtime_only_env(key) {
            continue;
        }
        cmd.env(key, value);
    }

    // User applications launched by alien-worker-runtime must use the transport and
    // bindings selected by this runtime process. Apply these last so deployment
    // env or secrets cannot leave the child with a stale runtime contract.
    configure_application_runtime_env(&mut cmd, config);

    // Always pipe stdout/stderr for telemetry capture
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        AlienError::new(ErrorData::ProcessFailed {
            exit_code: None,
            message: format!("Failed to start application: {}", e),
        })
    })?;

    info!(pid = child.id(), "Application started");

    // Always capture and stream stdout/stderr
    if let Some(stdout) = child.stdout.take() {
        let exporter = log_exporter.clone();
        tokio::spawn(async move {
            stream_output(stdout, true, exporter).await;
        });
    } else {
        warn!("Failed to capture stdout - will miss application logs");
    }

    if let Some(stderr) = child.stderr.take() {
        let exporter = log_exporter;
        tokio::spawn(async move {
            stream_output(stderr, false, exporter).await;
        });
    } else {
        warn!("Failed to capture stderr - will miss application error logs");
    }

    Ok(child)
}

fn runtime_only_env(name: &str) -> bool {
    matches!(name, ENV_ALIEN_RUNTIME_SECRETS | ENV_ALIEN_COMMANDS_TOKEN)
}

fn command_push_config(
    config: &RuntimeConfig,
    secrets: &HashMap<String, String>,
) -> Result<Option<CommandPushConfig>> {
    let Some(token) = secrets
        .get(ENV_ALIEN_COMMANDS_TOKEN)
        .or_else(|| config.env_vars.get(ENV_ALIEN_COMMANDS_TOKEN))
        .cloned()
    else {
        return Ok(None);
    };
    if token.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("{ENV_ALIEN_COMMANDS_TOKEN} must not be empty or whitespace"),
            field: Some(ENV_ALIEN_COMMANDS_TOKEN.to_string()),
        }));
    }

    let worker_resource_id = config
        .env_vars
        .get(ENV_ALIEN_CURRENT_WORKER_BINDING_NAME)
        .filter(|resource_id| !resource_id.is_empty())
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "{ENV_ALIEN_CURRENT_WORKER_BINDING_NAME} is required when command push is enabled"
                ),
                field: Some(ENV_ALIEN_CURRENT_WORKER_BINDING_NAME.to_string()),
            })
        })?;

    let deployment_id = config
        .env_vars
        .get(ENV_ALIEN_DEPLOYMENT_ID)
        .filter(|deployment_id| !deployment_id.is_empty())
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "{ENV_ALIEN_DEPLOYMENT_ID} is required when command push is enabled"
                ),
                field: Some(ENV_ALIEN_DEPLOYMENT_ID.to_string()),
            })
        })?;

    Ok(Some(CommandPushConfig {
        token,
        deployment_id,
        worker_resource_id,
    }))
}

async fn load_runtime_secrets(
    config: &RuntimeConfig,
    provider: &dyn alien_bindings::BindingsProviderApi,
) -> Result<HashMap<String, String>> {
    let runtime_secrets_json = config
        .env_vars
        .get(ENV_ALIEN_RUNTIME_SECRETS)
        .cloned()
        .or_else(|| std::env::var(ENV_ALIEN_RUNTIME_SECRETS).ok());

    let Some(runtime_secrets_json) = runtime_secrets_json else {
        debug!("No ALIEN_RUNTIME_SECRETS found");
        return Ok(HashMap::new());
    };

    let runtime_secrets_config: RuntimeSecretsConfig = serde_json::from_str(&runtime_secrets_json)
        .map_err(|error| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("Failed to parse ALIEN_RUNTIME_SECRETS: {error}"),
                field: Some(ENV_ALIEN_RUNTIME_SECRETS.to_string()),
            })
        })?;

    let logs_auth_secret = runtime_secrets_config.otlp_logs_auth_header;
    let metrics_auth_secret = runtime_secrets_config.otlp_metrics_auth_header;

    if logs_auth_secret.is_none() && metrics_auth_secret.is_none() {
        return Ok(HashMap::new());
    }

    let vault = provider
        .load_vault("secrets")
        .await
        .context(ErrorData::SecretLoadFailed {
            secret_name: "vault".to_string(),
            message: "Failed to load secrets vault for runtime secrets".to_string(),
        })?;

    let mut runtime_secrets = HashMap::new();
    if let Some(secret_key) = logs_auth_secret {
        let value = vault
            .get_secret(&secret_key)
            .await
            .context(ErrorData::SecretLoadFailed {
                secret_name: secret_key.clone(),
                message: "Failed to load runtime secret".to_string(),
            })?;
        runtime_secrets.insert("OTEL_EXPORTER_OTLP_HEADERS".to_string(), value);
    }
    if let Some(secret_key) = metrics_auth_secret {
        let value = vault
            .get_secret(&secret_key)
            .await
            .context(ErrorData::SecretLoadFailed {
                secret_name: secret_key.clone(),
                message: "Failed to load runtime secret".to_string(),
            })?;
        runtime_secrets.insert("OTEL_EXPORTER_OTLP_METRICS_HEADERS".to_string(), value);
    }

    debug!(
        hash = %runtime_secrets_config.hash,
        count = runtime_secrets.len(),
        "Loaded runtime-only secrets"
    );

    Ok(runtime_secrets)
}

fn configure_application_runtime_env(cmd: &mut Command, config: &RuntimeConfig) {
    // The Worker protocol server only hosts Control + WaitUntil. Legacy SDKs
    // interpret this marker as "send resource bindings over gRPC", so remove
    // it even when the runtime process or deployment environment supplied it.
    // Without the marker, legacy SDKs keep bindings in-process while still
    // using the legacy address below for the Worker protocol services.
    cmd.env_remove(ENV_ALIEN_BINDINGS_MODE);

    for (key, value) in application_runtime_env(config) {
        cmd.env(key, value);
    }
}

fn application_runtime_env(config: &RuntimeConfig) -> Vec<(&'static str, String)> {
    // Both address names point to the same dual-namespace server during the
    // protocol rollout. Current SDKs prefer the Worker name; older SDKs use the
    // Bindings name for Control + WaitUntil.
    vec![
        (
            ENV_ALIEN_WORKER_GRPC_ADDRESS,
            config.worker_grpc_address.clone(),
        ),
        (
            ENV_ALIEN_BINDINGS_GRPC_ADDRESS,
            config.worker_grpc_address.clone(),
        ),
        (
            ENV_ALIEN_TRANSPORT,
            application_transport_env_value(config.transport).to_string(),
        ),
    ]
}

fn application_transport_env_value(transport: TransportType) -> &'static str {
    match transport {
        TransportType::Lambda => "lambda",
        TransportType::CloudRun => "cloud-run",
        TransportType::ContainerApp => "container-app",
        TransportType::Http => "http",
        TransportType::Local => "local",
    }
}

/// Stream stdout or stderr from the application.
///
/// Behavior depends on log_exporter:
/// - LogExporter::Otlp: Emit via tracing → OTLP (parent's tracing has OTLP layer)
/// - LogExporter::None: Print to stdout/stderr (for Containers - orchestrator captures)
async fn stream_output(
    output: impl tokio::io::AsyncRead + Unpin,
    is_stdout: bool,
    log_exporter: crate::config::LogExporter,
) {
    use crate::config::LogExporter;

    let stream_type = if is_stdout { "stdout" } else { "stderr" };
    tracing::debug!(stream = stream_type, exporter = ?log_exporter, "stream_output started");

    let reader = BufReader::new(output);
    let mut lines = reader.lines();

    match log_exporter {
        LogExporter::Otlp { .. } => {
            // Functions: Send logs via OpenTelemetry SDK (uses global provider from init_otlp_logging)
            let stream_name = if is_stdout { "stdout" } else { "stderr" };

            tracing::debug!("Starting OTLP log streaming via SDK");

            while let Ok(Some(line)) = lines.next_line().await {
                let timestamp_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

                // Print to local stdout/stderr for debugging
                if is_stdout {
                    println!("{}", line);
                } else {
                    eprintln!("{}", line);
                }

                // Emit normalized text via OpenTelemetry SDK (batched, proper protobuf format).
                let body = crate::log_text::normalize_log_body(&line);
                crate::otlp::emit_log(stream_name, &body, timestamp_nanos);
            }

            tracing::debug!("OTLP log streaming ended");
        }
        LogExporter::None => {
            // Containers: Pass through to stdout/stderr (orchestrator captures)
            // No prefix needed; the runtime adds resource_id when sending to OTLP/LogBuffer.
            tracing::debug!("Starting forwarded log streaming");

            while let Ok(Some(line)) = lines.next_line().await {
                if is_stdout {
                    println!("{}", line);
                } else {
                    eprintln!("{}", line);
                }
            }

            tracing::debug!("Forwarded log streaming ended");
        }
    }
}

/// Graceful shutdown.
async fn graceful_shutdown(
    wait_until_server: Arc<WaitUntilGrpcServer>,
    child: &mut Child,
) -> Result<()> {
    info!("Initiating graceful shutdown");

    // Trigger wait_until drain
    if let Err(e) = wait_until_server.trigger_drain_all("shutdown", 10).await {
        warn!(error = %e, "Failed to trigger drain");
    }

    // Wait for tasks to complete
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(15);

    loop {
        let count = wait_until_server.get_total_task_count().await;
        if count == 0 {
            info!("All tasks completed");
            break;
        }
        if start.elapsed() >= timeout {
            warn!(remaining = count, "Timeout waiting for tasks");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Terminate child
    info!("Terminating application");
    if let Err(e) = child.kill().await {
        warn!(error = %e, "Failed to kill application");
    }

    // Flush logs
    if let Err(e) = flush_otlp_logs().await {
        warn!(error = %e, "Failed to flush OTLP logs");
    }

    Ok(())
}

/// Setup shutdown signal handlers.
pub fn setup_shutdown_on_signals() -> (broadcast::Sender<()>, broadcast::Receiver<()>) {
    let (tx, rx) = broadcast::channel(1);

    // Ctrl+C
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        if signal::ctrl_c().await.is_ok() {
            info!("Received Ctrl+C");
            let _ = tx_clone.send(());
        }
    });

    // SIGTERM (Unix)
    #[cfg(unix)]
    {
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                sigterm.recv().await;
                info!("Received SIGTERM");
                let _ = tx_clone.send(());
            }
        });
    }

    (tx, rx)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

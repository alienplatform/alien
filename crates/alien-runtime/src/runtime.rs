//! Alien Runtime - runs applications on any platform.
//!
//! The runtime:
//! 1. Starts gRPC server (bindings + control service)
//! 2. Loads secrets from vault (includes commands token)
//! 3. Starts the application subprocess
//! 4. Waits for app to register HTTP port
//! 5. Starts commands polling (if enabled)
//! 6. Starts the appropriate transport

use std::{process::Stdio, sync::Arc};

use alien_bindings::{
    grpc::{
        control_service::ControlGrpcServer, run_grpc_server,
        wait_until_service::WaitUntilGrpcServer,
    },
    BindingsProvider,
};
use alien_error::{AlienError, Context};
use reqwest::Url;
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
    otlp::{flush_otlp_logs, shutdown_otlp_logs},
    transports::{
        commands_polling::CommandsPolling, cloudrun::CloudRunTransport, containerapp::ContainerAppTransport,
        local::LocalTransport,
    },
};

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

/// Specifies how the runtime should obtain bindings.
pub enum BindingsSource {
    /// Create bindings provider from environment variables (cloud platform).
    /// This is the default for production deployments.
    FromEnvironment,

    /// Use a custom bindings provider (local platform).
    /// The CLI creates a LocalBindingsProvider and passes it here.
    Provider(Arc<dyn alien_bindings::BindingsProviderApi>),

    /// Use externally-provided gRPC handles (testing).
    /// Tests start their own gRPC server with test bindings and pass the handles here.
    ExternalGrpc {
        wait_until_server: Arc<WaitUntilGrpcServer>,
        control_server: Arc<ControlGrpcServer>,
    },
}

/// Run the Alien runtime.
///
/// # Arguments
/// * `config` - Runtime configuration
/// * `shutdown_rx` - Shutdown signal receiver
/// * `bindings` - How to obtain bindings (environment, custom provider, or external gRPC)
pub async fn run(
    config: RuntimeConfig,
    shutdown_rx: broadcast::Receiver<()>,
    bindings: BindingsSource,
) -> Result<()> {
    // Note: For standalone binary (main.rs), init_tracing() is called before run()
    // For embedded runtime (LocalFunctionManager), parent already initialized tracing
    // LogExporter config tells stream_output() how to handle logs

    info!(
        transport = ?config.transport,
        command = ?config.command,
        log_exporter = ?config.log_exporter,
        "Starting Alien runtime"
    );

    // 1. Start gRPC server (or use external handles for testing)
    let (wait_until_server, control_server, bindings_provider) = match bindings {
        BindingsSource::ExternalGrpc {
            wait_until_server,
            control_server,
        } => {
            info!("Using externally-provided gRPC handles (testing mode)");
            // No provider available in test mode
            (wait_until_server, control_server, None)
        }
        BindingsSource::Provider(provider) => {
            info!("Using custom bindings provider (local platform)");
            let (wait, control, prov) =
                start_grpc_server(&config.bindings_address, Some(provider)).await?;
            (wait, control, Some(prov))
        }
        BindingsSource::FromEnvironment => {
            info!("Creating bindings provider from environment (cloud platform)");
            let (wait, control, prov) = start_grpc_server(&config.bindings_address, None).await?;
            (wait, control, Some(prov))
        }
    };

    // Store in global state
    let _ = WAIT_UNTIL_SERVER.set(wait_until_server.clone());
    let _ = CONTROL_SERVER.set(control_server.clone());

    // 2. Load secrets from vault if ALIEN_SECRETS is present
    // Returns HashMap of secrets to pass to subprocess (avoids std::env::set_var races)
    // For embedded runtimes, ALIEN_SECRETS is in config.env_vars (not process env)
    // For standalone runtimes, ALIEN_SECRETS is in process env (std::env)
    let secrets = if let Some(ref provider) = bindings_provider {
        if let Some(alien_secrets_json) = config.env_vars.get("ALIEN_SECRETS") {
            info!("Loading secrets from vault before starting application");
            crate::secrets::load_secrets_from_vault(&**provider, alien_secrets_json).await?
        } else if let Ok(alien_secrets_json) = std::env::var("ALIEN_SECRETS") {
            info!("Loading secrets from vault before starting application (from process env)");
            crate::secrets::load_secrets_from_vault(&**provider, &alien_secrets_json).await?
        } else {
            debug!("No ALIEN_SECRETS found in config or process env");
            std::collections::HashMap::new()
        }
    } else {
        std::collections::HashMap::new()
    };

    // 3. Start application subprocess with secrets
    let mut child = start_application(&config, &secrets).await?;

    // 4. Wait for app to register HTTP port (if transport needs it)
    let app_http_port = if config.transport != TransportType::Passthrough {
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
    } else {
        None
    };

    // 5. Start commands polling if enabled
    // Two ways to configure commands polling:
    // 1. Programmatic config via RuntimeConfig.commands_polling
    // 2. Environment variables
    let commands_polling_handle = if let Some(ref commands_config) = config.commands_polling {
        // Programmatic config path
        info!(
            url = %commands_config.url,
            deployment_id = %commands_config.deployment_id,
            "Starting commands polling from RuntimeConfig"
        );

        let url = Url::parse(&commands_config.url).map_err(|e| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("Invalid commands polling URL: {}", e),
                field: Some("commands_polling.url".to_string()),
            })
        })?;

        let commands_polling = CommandsPolling::new(
            url,
            commands_config.interval,
            commands_config.deployment_id.clone(),
            commands_config.token.clone(),
            control_server.clone(),
        );

        Some(tokio::spawn(async move {
            if let Err(e) = commands_polling.run().await {
                error!(error = %e, "Commands polling error");
            }
        }))
    } else if let Some(commands_polling) =
        CommandsPolling::from_env(&config.env_vars, &secrets, control_server.clone())?
    {
        // Environment variable config (standard path for all deployments)
        Some(tokio::spawn(async move {
            if let Err(e) = commands_polling.run().await {
                error!(error = %e, "Commands polling error");
            }
        }))
    } else {
        debug!("Commands polling not configured");
        None
    };

    // 6. Start transport and run main loop
    let result = run_transport(
        &config,
        control_server,
        app_http_port,
        wait_until_server,
        &mut child,
        shutdown_rx,
    )
    .await;

    // Cleanup
    if let Some(handle) = commands_polling_handle {
        handle.abort();
    }

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
    wait_until_server: Arc<WaitUntilGrpcServer>,
    child: &mut Child,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    // Create separate shutdown receiver for transport
    let transport_shutdown_rx = shutdown_rx.resubscribe();

    // Spawn the transport as a task
    let transport_handle: JoinHandle<Result<()>> = spawn_transport(
        config.transport,
        config.transport_port,
        config.lambda_mode,
        control_server,
        app_http_port,
        transport_shutdown_rx,
    )?;

    // Wait for shutdown, child exit, or transport completion
    tokio::select! {
        shutdown_result = shutdown_rx.recv() => {
            handle_shutdown(shutdown_result, wait_until_server, child).await
        }

        child_status = child.wait() => {
            handle_child_exit(child_status)
        }

        transport_result = transport_handle => {
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
    }
}

/// Spawn the appropriate transport as a tokio task.
fn spawn_transport(
    transport_type: TransportType,
    transport_port: u16,
    lambda_mode: crate::config::LambdaMode,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<JoinHandle<Result<()>>> {
    match transport_type {
        TransportType::CloudRun => {
            let mut transport = CloudRunTransport::new(transport_port, control_server, shutdown_rx);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::ContainerApp => {
            let mut transport =
                ContainerAppTransport::new(transport_port, control_server, shutdown_rx);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::Local => {
            let mut transport = LocalTransport::new(transport_port, control_server, shutdown_rx);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
        }

        TransportType::Passthrough => {
            info!("Passthrough mode - no transport");
            Ok(tokio::spawn(async move {
                std::future::pending::<Result<()>>().await
            }))
        }

        #[cfg(feature = "aws")]
        TransportType::Lambda => {
            use crate::transports::lambda::LambdaTransport;

            // Lambda transport doesn't use shutdown_rx (polls Lambda Runtime API)
            let mut transport = LambdaTransport::new(lambda_mode, control_server);
            if let Some(port) = app_http_port {
                transport = transport.with_app_port(port);
            }
            Ok(tokio::spawn(async move { transport.run().await }))
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
            error!(exit_code = code, "Application exited with error");
            Err(AlienError::new(ErrorData::ProcessFailed {
                exit_code: Some(code),
                message: format!("Application exited with code {}", code),
            }))
        }
        Err(e) => {
            error!(error = %e, "Failed to wait for application");
            Err(AlienError::new(ErrorData::ProcessFailed {
                exit_code: None,
                message: format!("Failed to wait for process: {}", e),
            }))
        }
    }
}

/// Start gRPC server with bindings, control, and wait_until services.
///
/// If a custom bindings_provider is provided (local platform), use it directly.
/// Otherwise (cloud platform), create bindings provider from environment.
async fn start_grpc_server(
    address: &str,
    bindings_provider: Option<Arc<dyn alien_bindings::BindingsProviderApi>>,
) -> Result<(
    Arc<WaitUntilGrpcServer>,
    Arc<ControlGrpcServer>,
    Arc<dyn alien_bindings::BindingsProviderApi>,
)> {
    info!(address = %address, "Starting gRPC server");

    // Use custom provider if provided, otherwise create from environment
    let provider: Arc<dyn alien_bindings::BindingsProviderApi> =
        if let Some(custom_provider) = bindings_provider {
            custom_provider
        } else {
            Arc::new(
                BindingsProvider::from_env(std::env::vars().collect())
                    .await
                    .context(ErrorData::BindingsOperationFailed {
                        address: address.to_string(),
                        provider: None,
                        message: "Failed to create bindings provider".to_string(),
                    })?,
            )
        };

    let handles = run_grpc_server(provider.clone(), address).await.context(
        ErrorData::BindingsOperationFailed {
            address: address.to_string(),
            provider: None,
            message: "Failed to start gRPC server".to_string(),
        },
    )?;

    // Wait for server ready
    match handles.readiness_receiver.await {
        Ok(_) => info!("gRPC server ready"),
        Err(_) => warn!("gRPC readiness channel closed"),
    }

    // Spawn server task
    let addr = address.to_string();
    tokio::spawn(async move {
        match handles.server_task.await {
            Ok(Ok(_)) => info!(address = %addr, "gRPC server exited"),
            Ok(Err(e)) => error!(error = %e, address = %addr, "gRPC server error"),
            Err(e) => error!(error = %e, address = %addr, "gRPC server panicked"),
        }
    });

    info!(address = %address, "gRPC server started");
    Ok((handles.wait_until_server, handles.control_server, provider))
}

/// Start the application subprocess with secrets loaded from vault.
///
/// Secrets are passed explicitly to avoid std::env::set_var() races in embedded runtime mode.
async fn start_application(
    config: &RuntimeConfig,
    secrets: &std::collections::HashMap<String, String>,
) -> Result<Child> {
    if config.command.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: "Application command is empty".to_string(),
            field: Some("command".to_string()),
        }));
    }

    info!(command = ?config.command, working_dir = ?config.working_dir, "Starting application");

    let mut cmd = Command::new(&config.command[0]);
    if config.command.len() > 1 {
        cmd.args(&config.command[1..]);
    }

    // Set working directory if specified
    if let Some(ref working_dir) = config.working_dir {
        cmd.current_dir(working_dir);
    }

    // Set bindings address
    cmd.env("ALIEN_BINDINGS_GRPC_ADDRESS", &config.bindings_address);

    // Set custom environment variables
    for (key, value) in &config.env_vars {
        cmd.env(key, value);
    }

    // Set secrets loaded from vault
    for (key, value) in secrets {
        cmd.env(key, value);
    }

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
    let exporter = config.log_exporter.clone();
    if let Some(stdout) = child.stdout.take() {
        let exporter = exporter.clone();
        tokio::spawn(async move {
            stream_output(stdout, true, exporter).await;
        });
    } else {
        warn!("Failed to capture stdout - will miss application logs");
    }

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            stream_output(stderr, false, exporter).await;
        });
    } else {
        warn!("Failed to capture stderr - will miss application error logs");
    }

    Ok(child)
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

                // Emit via OpenTelemetry SDK (batched, proper protobuf format)
                crate::otlp::emit_log(stream_name, &line, timestamp_nanos);
            }

            tracing::debug!("OTLP log streaming ended");
        }
        LogExporter::None => {
            // Containers: Pass through to stdout/stderr (orchestrator captures)
            // No prefix needed - Docker/horizond adds resource_id when sending to OTLP/LogBuffer
            tracing::debug!("Starting passthrough log streaming");

            while let Ok(Some(line)) = lines.next_line().await {
                if is_stdout {
                    println!("{}", line);
                } else {
                    eprintln!("{}", line);
                }
            }

            tracing::debug!("Passthrough log streaming ended");
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

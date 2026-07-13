use alien_worker_runtime::{
    init_tracing, run, setup_shutdown_on_signals, BindingsSource, Error, Result, RuntimeConfig,
};
use tracing::{error, info};

fn main() -> std::process::ExitCode {
    // Build tokio runtime manually with a larger worker thread stack size.
    // The default musl stack size (128KB) is too small for the deep async call
    // stacks from gRPC + OTLP initialization, causing stack overflows.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024) // 8 MB
        .build()
        .expect("Failed to build tokio runtime");

    match runtime.block_on(async_main()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_error(error);
            std::process::ExitCode::FAILURE
        }
    }
}

fn report_error(error: Error) {
    let error = error.into_external();
    let report = error.human_report();
    let serialized = serde_json::to_string(&error).unwrap_or_else(|serialization_error| {
        format!("failed to serialize error: {serialization_error}")
    });

    error!(
        message = %report.message,
        error_code = %error.code,
        error_retryable = error.retryable,
        error_internal = error.internal,
        error_http_status_code = i64::from(error.http_status_code.unwrap_or(500)),
        alien_error = %serialized,
    );
}

async fn async_main() -> Result<()> {
    dotenvy::dotenv().ok();

    init_tracing()?;

    info!("Initializing Alien Runtime...");

    // Load configuration from CLI arguments and environment variables
    let config = RuntimeConfig::from_cli()?;

    info!(
        transport = ?config.transport,
        command = ?config.command,
        "Configuration loaded"
    );

    // Set up shutdown handling for Ctrl+C and SIGTERM
    let (_shutdown_tx, shutdown_rx) = setup_shutdown_on_signals();

    // Run the runtime with bindings from environment
    run(config, shutdown_rx, BindingsSource::FromEnvironment).await?;

    Ok(())
}

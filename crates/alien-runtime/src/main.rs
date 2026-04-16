use alien_runtime::{
    init_tracing, run, setup_shutdown_on_signals, BindingsSource, Result, RuntimeConfig,
};
use tracing::{error, info};

fn main() -> Result<()> {
    // Build tokio runtime manually with a larger worker thread stack size.
    // The default musl stack size (128KB) is too small for the deep async call
    // stacks from gRPC + OTLP initialization, causing stack overflows.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024) // 8 MB
        .build()
        .expect("Failed to build tokio runtime");

    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    dotenvy::dotenv().ok();

    init_tracing()?;

    info!("Initializing Alien Runtime...");

    // Load configuration from CLI arguments and environment variables
    let config = RuntimeConfig::from_cli().map_err(|e| {
        error!(error = %e, "Failed to load configuration");
        e
    })?;

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

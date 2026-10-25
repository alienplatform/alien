use alien_cli::{output::json_error_request_id, run_cli, ui::render_human_error, Cli};
use clap::Parser;

fn main() -> std::process::ExitCode {
    // Embedded reconciliation polls deeply nested controller and registry futures.
    // Debug builds need the same stack headroom as the worker runtime.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Failed to start async runtime: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    runtime.block_on(async_main())
}

async fn async_main() -> std::process::ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let wants_json_output = cli.wants_json_output();
    match run_cli(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if wants_json_output {
                let request_id = json_error_request_id(&error);
                let serialized =
                    serde_json::to_value(error.into_external()).and_then(|mut value| {
                        if let Some(request_id) = request_id {
                            value["requestId"] = serde_json::Value::String(request_id);
                        }
                        serde_json::to_string_pretty(&value)
                    });
                match serialized {
                    Ok(json) => eprintln!("{json}"),
                    Err(print_error) => eprintln!("Failed to serialize error: {print_error}"),
                }
            } else {
                eprintln!("{}", render_human_error(&error));
            }
            std::process::ExitCode::from(1)
        }
    }
}

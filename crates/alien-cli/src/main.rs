use alien_cli::{
    output::{json_error_diagnostic, json_error_request_id, print_json},
    run_cli,
    ui::render_human_error,
    Cli,
};
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
                let external_error = error.clone().into_external();
                if let Err(print_error) = print_json(&external_error) {
                    eprintln!("{print_error}");
                }
                eprintln!(
                    "{}",
                    json_error_diagnostic(&external_error, request_id.as_deref())
                );
            } else {
                eprintln!("{}", render_human_error(&error));
            }
            std::process::ExitCode::from(1)
        }
    }
}

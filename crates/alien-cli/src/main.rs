use alien_cli::{
    output::{json_error_diagnostic, print_json},
    run_cli,
    ui::render_human_error,
    Cli,
};
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let wants_json_output = cli.wants_json_output();
    match run_cli(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if wants_json_output {
                let external_error = error.clone().into_external();
                if let Err(print_error) = print_json(&external_error) {
                    eprintln!("{print_error}");
                }
                eprintln!("{}", json_error_diagnostic(&external_error));
            } else {
                eprintln!("{}", render_human_error(&error));
            }
            std::process::ExitCode::from(1)
        }
    }
}

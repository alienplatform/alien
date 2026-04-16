use alien_cli::{output::print_json, run_cli, ui::render_human_error, Cli};
use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let wants_json_output = cli.wants_json_output();
    match run_cli(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            if wants_json_output {
                if let Err(print_error) = print_json(&error.clone().into_generic()) {
                    eprintln!("{print_error}");
                }
            } else {
                eprintln!("{}", render_human_error(&error));
            }
            std::process::ExitCode::from(1)
        }
    }
}

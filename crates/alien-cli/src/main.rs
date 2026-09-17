use alien_cli::{output::print_json, run_cli, ui::render_human_error, Cli};
use clap::Parser;

#[cfg(not(feature = "local-runtime"))]
fn delegate_to_local_runtime() -> std::io::Error {
    let executable_name = if cfg!(windows) {
        "alien-local-runtime.exe"
    } else {
        "alien-local-runtime"
    };
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(executable_name)))
        .unwrap_or_else(|| executable_name.into());
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec()
    }

    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => error,
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let wants_json_output = cli.wants_json_output();
    #[cfg(not(feature = "local-runtime"))]
    if cli.requires_local_runtime() {
        let error = delegate_to_local_runtime();
        eprintln!(
            "Could not start the Alien local runtime: {error}. Reinstall or upgrade the Alien CLI."
        );
        return std::process::ExitCode::from(1);
    }
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

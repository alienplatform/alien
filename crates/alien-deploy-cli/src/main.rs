use alien_deploy_cli::{parse_cli, run_cli};

#[tokio::main]
async fn main() {
    let cli = parse_cli();
    if let Err(e) = run_cli(cli).await {
        eprintln!("\x1b[31mError:\x1b[0m {}", e);
        std::process::exit(1);
    }
}

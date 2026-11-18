use alien_deploy_cli::{parse_cli, run_cli};

#[tokio::main]
async fn main() {
    let cli = parse_cli();
    if let Err(e) = run_cli(cli).await {
        // The chain quotes text from manifests, stacks and cloud responses.
        eprintln!(
            "\x1b[31mError:\x1b[0m {}",
            alien_preflights::escape_for_display(&e.to_string())
        );
        std::process::exit(1);
    }
}

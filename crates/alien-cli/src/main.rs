use alien_cli::{run_cli, Cli};
use clap::Parser;

#[tokio::main]
async fn main() -> alien_cli::error::Result<()> {
    let cli = Cli::parse();
    run_cli(cli).await
}

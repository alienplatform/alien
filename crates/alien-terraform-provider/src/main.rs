//! OSS `terraform-provider-alien` binary.
//!
//! Plain-vanilla provider entry point with no embedded defaults: customers
//! configure `manager_url` via the standard HCL `provider "alien"` block.
//! The binary also accepts a `--probe` flag that prints the resolved provider
//! schema as JSON, so packaging tooling and CI can verify the binary it built
//! before shipping.
//!
//! White-label distribution (magic-bytes footer + vendor branding) lives in
//! the platform-side `alien-terraform-providerx` crate. The OSS binary
//! deliberately knows nothing about the footer mechanism so the OSS surface
//! stays minimal and the platform packaging detail stays out of public API.

use std::process::ExitCode;

use alien_terraform_provider::{
    provider_schema, resource_schema, serve_terraform_provider, ProviderOptions,
};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "terraform-provider-alien",
    about = "Alien deployment registration provider for Terraform.",
    long_about = "Registers a stack import with an Alien Manager via the typed \
                  /v1/stack/import endpoint. Designed to be invoked by Terraform.",
    version
)]
struct Cli {
    /// Print the resolved provider + resource schema as JSON and exit.
    /// Used by packaging and CI to verify a built binary before shipping.
    #[arg(long)]
    probe: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if cli.probe {
        let payload = serde_json::json!({
            "provider": provider_schema(),
            "resource_alien_deployment": resource_schema(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return ExitCode::SUCCESS;
    }

    match serve_terraform_provider(ProviderOptions::default()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("terraform-provider-alien failed: {err}");
            ExitCode::from(1)
        }
    }
}

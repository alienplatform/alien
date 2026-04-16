//! Export OpenAPI schema for alien-manager.
//!
//! Usage: cargo run --bin alien-manager-schema-exporter --features openapi -- --output openapi.json

use std::{fs::File, io::Write as _};

use clap::Parser;
use utoipa::OpenApi;

use alien_commands::server::axum_handlers::ApiDoc as CommandsApiDoc;
use alien_manager::api::ApiDoc;

#[derive(Parser, Debug)]
#[command(author, version, about = "Export alien-manager OpenAPI schema")]
struct Args {
    /// Output file path
    #[arg(short, long, default_value = "openapi.json")]
    output: String,
}

fn main() {
    let args = Args::parse();

    let mut manager_api = ApiDoc::openapi();
    let mut commands_api = CommandsApiDoc::openapi();

    // Commands routes are mounted under /v1 at runtime via Router::nest("/v1", ...),
    // but the utoipa annotations in alien-commands declare paths without the /v1 prefix
    // (because nesting is the manager's concern, not the commands crate's).
    // Prefix all commands paths with /v1 before merging.
    {
        let old_paths = std::mem::take(&mut commands_api.paths.paths);
        for (path, item) in old_paths {
            commands_api
                .paths
                .paths
                .insert(format!("/v1{}", path), item);
        }
    }

    manager_api.merge(commands_api);

    let spec = manager_api
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec");

    let mut file =
        File::create(&args.output).expect(&format!("Failed to create file: {}", args.output));
    file.write_all(spec.as_bytes())
        .expect("Failed to write OpenAPI spec");

    println!("OpenAPI spec exported to {}", args.output);
}

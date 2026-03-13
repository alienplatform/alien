//! Export OpenAPI schema for alien-manager.
//!
//! Usage: cargo run --bin alien-manager-schema-exporter --features openapi -- --output openapi.json

use std::{fs::File, io::Write as _};

use clap::Parser;
use utoipa::OpenApi;

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

    let spec = ApiDoc::openapi()
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec");

    let mut file =
        File::create(&args.output).expect(&format!("Failed to create file: {}", args.output));
    file.write_all(spec.as_bytes())
        .expect("Failed to write OpenAPI spec");

    println!("OpenAPI spec exported to {}", args.output);
}

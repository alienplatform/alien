use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match generate() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn generate() -> Result<(), String> {
    let manifest = custom_ops::plugin_manifest().map_err(|error| error.to_string())?;
    let generated = serde_json::to_string_pretty(&manifest)
        .map(|json| format!("{json}\n"))
        .map_err(|error| error.to_string())?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("metadata.json");
    if std::env::args().any(|argument| argument == "--check") {
        let current = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if current != generated {
            return Err(
                "metadata.json is stale; run cargo run -p custom-ops --bin generate-metadata"
                    .to_string(),
            );
        }
    } else {
        std::fs::write(path, generated).map_err(|error| error.to_string())?;
    }
    Ok(())
}

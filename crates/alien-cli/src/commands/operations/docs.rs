//! `alien operations docs` — generate MCP tool schemas and a Markdown
//! reference page from a plugin's manifest. Fully offline: pure codegen
//! from `metadata.json`, driven by the canonical operations SDK generators.

use std::path::Path;

use alien_error::{Context, IntoAlienError};
use alien_operations_sdk::{generate_docs_canonical, generate_mcp_tools_canonical};

use super::check::parse_manifest_for_cli;
use crate::error::{ErrorData, Result};

pub fn docs_task(directory: Option<&str>, json: bool) -> Result<()> {
    let manifest_path =
        Path::new(directory.unwrap_or(".")).join(alien_operations_sdk::manifest::MANIFEST_FILENAME);
    let bytes = std::fs::read(&manifest_path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("could not read '{}'", manifest_path.display()),
        },
    )?;
    let manifest = parse_manifest_for_cli(&bytes).context(ErrorData::ConfigurationError {
        message: format!(
            "'{}' is not a valid plugin manifest",
            manifest_path.display()
        ),
    })?;

    let tools = generate_mcp_tools_canonical(&manifest);
    let markdown = generate_docs_canonical(&manifest);

    if json {
        crate::output::print_json(&serde_json::json!({
            "mcpTools": tools,
            "markdown": markdown,
        }))?;
    } else {
        println!("{markdown}");
        println!("---");
        println!(
            "Generated {} MCP tool schema(s). Use --json to get them as structured output.",
            tools.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(directory: &Path, contents: &str) {
        std::fs::write(
            directory.join(alien_operations_sdk::manifest::MANIFEST_FILENAME),
            contents,
        )
        .expect("write test manifest");
    }

    #[test]
    fn generates_docs_for_a_valid_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health", "description": "Check connectivity." }]
            }"#,
        );

        docs_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect("valid manifest should generate docs");
    }

    #[test]
    fn rejects_an_invalid_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            r#"{
                "name": "postgres",
                "version": "1.0.0",
                "tier": "read-only",
                "binaries": { "amd64": "postgres-linux-amd64" },
                "operations": [{ "name": "health" }, { "name": "health" }]
            }"#,
        );

        let err = docs_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect_err("invalid manifest must fail");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }

    #[test]
    fn generates_docs_for_a_released_legacy_manifest() {
        let temp = tempfile::tempdir().expect("create temp dir");
        write_manifest(
            temp.path(),
            super::super::check::legacy_verification_manifest(),
        );

        docs_task(Some(temp.path().to_str().expect("utf8 path")), false)
            .expect("docs must preserve released legacy manifest support");
    }
}

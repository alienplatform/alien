//! `alien operations init` — scaffold a new operations plugin from the
//! public SDK. Fully local: no platform account, no network access.

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_operations_sdk::{Arch, CanonicalPluginManifest, OperationDefinition, RiskTier};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

/// Generate a new plugin crate at `directory` (defaults to `./<name>`) named
/// `name`, implementing [`alien_operations_sdk::Plugin`] against a single
/// `health` read-only operation as a starting point.
pub fn init_task(name: &str, directory: Option<&str>, json: bool) -> Result<()> {
    validate_plugin_name(name)?;
    let target = PathBuf::from(directory.unwrap_or(name));
    if target.exists() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "'{}' already exists; choose a different name or directory",
                target.display()
            ),
        }));
    }

    write_plugin_scaffold(&target, name)?;

    if json {
        crate::output::print_json(&serde_json::json!({
            "name": name,
            "directory": target,
        }))?;
    } else {
        println!(
            "Created operations plugin '{name}' in {}.",
            target.display()
        );
        println!();
        println!("Next steps:");
        println!("  cd {}", target.display());
        println!("  alien operations check");
        println!("  cargo test");
        println!("  alien operations test");
        println!("  alien operations package");
        println!("  alien operations publish <bundle.zip>");
    }
    Ok(())
}

fn validate_plugin_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    if !valid {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: "plugin name must be kebab-case (lowercase letters, digits, hyphens; \
                       starting with a letter), e.g. 'my-plugin'"
                .to_string(),
        }));
    }
    Ok(())
}

fn write_plugin_scaffold(directory: &Path, name: &str) -> Result<()> {
    create_dir(directory)?;
    create_dir(&directory.join("src"))?;
    create_dir(&directory.join("src/bin"))?;

    write_file(&directory.join("Cargo.toml"), &cargo_toml(name))?;
    write_file(&directory.join("metadata.json"), &metadata_json(name)?)?;
    write_file(&directory.join("src/main.rs"), &main_rs(name))?;
    write_file(&directory.join("src/lib.rs"), &lib_rs(name))?;
    write_file(
        &directory.join("src/bin/generate-metadata.rs"),
        &generate_metadata_rs(name),
    )?;
    Ok(())
}

fn create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not create directory '{}'", path.display()),
        })
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not write '{}'", path.display()),
        })
}

/// Pins the scaffold's `alien-operations-sdk` dependency to the exact
/// version of this CLI build — `alien-operations-sdk` publishes to
/// crates.io alongside every `alien-cli` release (see `.github/workflows
/// /release.yml`'s crate-publish list), so this is always a version that
/// exists once the CLI itself has shipped. A bare `"*"` would try to
/// resolve to whatever the latest published version is, which may be
/// incompatible with the manifest schema this CLI's `check`/`test` expect.
fn cargo_toml(name: &str) -> String {
    let lib_name = name.replace('-', "_");
    let sdk_version = env!("CARGO_PKG_VERSION");
    format!(
        r#"# Marks this crate as its own Cargo workspace root. Without it, scaffolding
# a plugin inside (or nested under) an existing Cargo workspace — e.g. the
# alien-operations-sdk repo itself — fails with "current package believes
# it's in a workspace when it's not", since Cargo otherwise tries to fold
# this crate into the enclosing workspace.
[workspace]

[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
publish = false
description = "An Alien operations plugin."

[lib]
name = "{lib_name}"
path = "src/lib.rs"

[[bin]]
name = "{name}"
path = "src/main.rs"

[[bin]]
name = "generate-metadata"
path = "src/bin/generate-metadata.rs"

[dependencies]
alien-operations-sdk = "={sdk_version}"
schemars = "0.8"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "io-util", "io-std"] }}
"#
    )
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "HealthParams")]
struct ScaffoldHealthParams {}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(rename = "HealthOutput")]
struct ScaffoldHealthOutput {
    status: String,
}

fn metadata_json(name: &str) -> Result<String> {
    let manifest = CanonicalPluginManifest {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        tier: RiskTier::ReadOnly,
        binaries: [
            (Arch::Amd64, format!("{name}-linux-amd64")),
            (Arch::Arm64, format!("{name}-linux-arm64")),
        ]
        .into(),
        operations: vec![
            OperationDefinition::<ScaffoldHealthParams, ScaffoldHealthOutput>::new(
                "health",
                RiskTier::ReadOnly,
                "Report plugin health.",
            )
            .with_no_permissions()
            .manifest(),
        ],
    };
    manifest.validate().context(ErrorData::ConfigurationError {
        message: "generated plugin manifest is invalid".to_string(),
    })?;
    serde_json::to_string_pretty(&manifest)
        .map(|json| format!("{json}\n"))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not encode generated plugin manifest".to_string(),
        })
}

fn main_rs(name: &str) -> String {
    let lib_name = name.replace('-', "_");
    format!(
        r#"use std::process::ExitCode;

use alien_operations_sdk::run_plugin;
use {lib_name}::operations;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {{
    let plugin = match operations() {{
        Ok(plugin) => plugin,
        Err(error) => {{
            eprintln!("plugin definition is invalid: {{error}}");
            return ExitCode::FAILURE;
        }}
    }};
    run_plugin(plugin).await
}}
"#
    )
}

fn lib_rs(name: &str) -> String {
    format!(
        r#"//! The {name} plugin.

use std::collections::BTreeMap;

use alien_operations_sdk::{{
    Arch, CanonicalPluginManifest, OperationDefinition, OperationFailure, Result, RiskTier,
    TypedOperations,
}};
use schemars::JsonSchema;
use serde::{{Deserialize, Serialize}};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HealthParams {{}}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct HealthOutput {{
    status: String,
}}

fn health_definition() -> OperationDefinition<HealthParams, HealthOutput> {{
    OperationDefinition::new("health", RiskTier::ReadOnly, "Report plugin health.")
        .with_no_permissions()
}}

async fn health(_params: HealthParams) -> std::result::Result<HealthOutput, OperationFailure> {{
    Ok(HealthOutput {{
        status: "healthy".to_string(),
    }})
}}

/// Build the runtime registry from the plugin's typed definitions.
pub fn operations() -> Result<TypedOperations> {{
    let mut operations = TypedOperations::with_unknown_operation_message(
        "{name} plugin does not expose the requested operation",
    );
    operations.register(health_definition(), health)?;
    Ok(operations)
}}

/// Generate bundle metadata from the same definitions used for dispatch.
pub fn plugin_manifest() -> Result<CanonicalPluginManifest> {{
    let operations = operations()?;
    let manifest = CanonicalPluginManifest {{
        name: "{name}".to_string(),
        version: "0.1.0".to_string(),
        tier: RiskTier::ReadOnly,
        binaries: BTreeMap::from([
            (Arch::Amd64, "{name}-linux-amd64".to_string()),
            (Arch::Arm64, "{name}-linux-arm64".to_string()),
        ]),
        operations: operations.manifests(),
    }};
    manifest.validate()?;
    Ok(manifest)
}}

#[cfg(test)]
mod tests {{
    use alien_operations_sdk::{{PluginInvocation, PluginResult}};

    use super::*;

    #[tokio::test]
    async fn health_reports_healthy() {{
        let invocation = PluginInvocation::inline_json("health", b"{{}}");
        let result = operations()
            .expect("typed definitions should register")
            .execute(&invocation)
            .await;
        let PluginResult::Success {{ response }} = result else {{
            panic!("health should succeed");
        }};
        let response = response.decode_inline().expect("health response is inline");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response).expect("health response is JSON"),
            serde_json::json!({{ "status": "healthy" }})
        );
    }}

    #[tokio::test]
    async fn unknown_operation_is_an_error() {{
        let invocation = PluginInvocation::inline_json("does-not-exist", b"{{}}");
        let result = operations()
            .expect("typed definitions should register")
            .execute(&invocation)
            .await;
        let PluginResult::Error {{ code, .. }} = result else {{
            panic!("unknown operation should fail");
        }};
        assert_eq!(code, "OPERATION_UNKNOWN");
    }}

    #[test]
    fn checked_in_metadata_matches_typed_definitions() {{
        let generated = serde_json::to_value(plugin_manifest().expect("manifest should generate"))
            .expect("generated manifest should serialize");
        let checked_in: serde_json::Value = serde_json::from_str(include_str!("../metadata.json"))
            .expect("checked-in metadata should decode");
        assert_eq!(checked_in, generated);
    }}
}}
"#
    )
}

fn generate_metadata_rs(name: &str) -> String {
    let lib_name = name.replace('-', "_");
    format!(
        r#"use std::process::ExitCode;

fn main() -> ExitCode {{
    match run() {{
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {{
            eprintln!("{{message}}");
            ExitCode::FAILURE
        }}
    }}
}}

fn run() -> Result<(), String> {{
    let manifest = {lib_name}::plugin_manifest().map_err(|error| error.to_string())?;
    let generated = serde_json::to_string_pretty(&manifest)
        .map(|json| format!("{{json}}\n"))
        .map_err(|error| format!("could not encode {name} metadata: {{error}}"))?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("metadata.json");
    if std::env::args().any(|argument| argument == "--check") {{
        let current = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read '{{}}': {{error}}", path.display()))?;
        if current != generated {{
            return Err(format!(
                "'{{}}' is stale; regenerate it with cargo run --bin generate-metadata",
                path.display()
            ));
        }}
        return Ok(());
    }}
    std::fs::write(&path, generated)
        .map_err(|error| format!("could not write '{{}}': {{error}}", path.display()))
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_names_with_uppercase_or_underscores() {
        for name in [
            "MyPlugin",
            "my_plugin",
            "-leading-hyphen",
            "1starts-with-digit",
            "",
        ] {
            assert!(
                validate_plugin_name(name).is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn accepts_kebab_case_names() {
        for name in ["postgres", "my-plugin", "plugin2"] {
            assert!(
                validate_plugin_name(name).is_ok(),
                "{name} should be accepted"
            );
        }
    }

    #[test]
    fn scaffolds_a_buildable_plugin_crate() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("my-plugin");
        write_plugin_scaffold(&target, "my-plugin").expect("scaffold should succeed");

        assert!(target.join("Cargo.toml").is_file());
        assert!(target.join("metadata.json").is_file());
        assert!(target.join("src/main.rs").is_file());
        assert!(target.join("src/lib.rs").is_file());
        assert!(target.join("src/bin/generate-metadata.rs").is_file());

        let metadata_bytes = std::fs::read(target.join("metadata.json")).expect("read metadata");
        let manifest =
            alien_operations_sdk::CanonicalPluginManifest::parse_and_validate(&metadata_bytes)
                .expect("scaffolded metadata.json should be a valid manifest");
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.operations.len(), 1);
        assert_eq!(manifest.operations[0].name, "health");
        assert!(manifest.operations[0].input_schema.is_some());
        assert!(manifest.operations[0].output_schema.is_some());
        let lib = std::fs::read_to_string(target.join("src/lib.rs")).expect("read scaffold lib");
        assert_eq!(
            lib.matches("#[serde(rename_all = \"camelCase\")]").count(),
            1
        );
        assert!(lib.contains("#[serde(rename_all = \"camelCase\", deny_unknown_fields)]"));
    }

    #[test]
    fn scaffolded_cargo_toml_is_isolated_from_an_enclosing_workspace() {
        // Regression test: scaffolding inside an existing Cargo workspace
        // (e.g. this very repo) must not produce a crate Cargo tries to fold
        // into that workspace — see the `[workspace]` comment in cargo_toml.
        let contents = cargo_toml("my-plugin");
        let workspace_line = contents
            .lines()
            .find(|line| line.trim() == "[workspace]")
            .expect("scaffolded Cargo.toml must declare an empty [workspace] table");
        let package_line_index = contents
            .lines()
            .position(|line| line.trim() == "[package]")
            .expect("scaffolded Cargo.toml must declare a [package] table");
        let workspace_line_index = contents
            .lines()
            .position(|line| line == workspace_line)
            .unwrap();
        assert!(
            workspace_line_index < package_line_index,
            "[workspace] must come before [package] so this crate is its own workspace root"
        );
    }

    #[test]
    fn refuses_to_overwrite_an_existing_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("existing");
        std::fs::create_dir(&target).expect("create existing dir");

        let err = init_task("existing", Some(target.to_str().expect("utf8 path")), false)
            .expect_err("must refuse to overwrite");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }
}

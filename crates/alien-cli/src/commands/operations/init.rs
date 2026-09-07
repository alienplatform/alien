//! `alien operations init` — scaffold a new operations plugin from the
//! public SDK. Fully local: no platform account, no network access.

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};

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
        println!("Created operations plugin '{name}' in {}.", target.display());
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

    write_file(&directory.join("Cargo.toml"), &cargo_toml(name))?;
    write_file(&directory.join("metadata.json"), &metadata_json(name))?;
    write_file(&directory.join("src/main.rs"), &main_rs(name))?;
    write_file(&directory.join("src/lib.rs"), &lib_rs(name))?;
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

/// `alien-operations-sdk = "*"`: this crate is not yet published to
/// crates.io as of this writing. Until it is, a scaffolded plugin needs a
/// path or git dependency instead — see the SDK's own repository for the
/// current recommended dependency line.
fn cargo_toml(name: &str) -> String {
    let lib_name = name.replace('-', "_");
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

[dependencies]
alien-operations-sdk = "*"
async-trait = "0.1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread", "io-util", "io-std"] }}
"#
    )
}

fn metadata_json(name: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "tier": "read-only",
  "binaries": {{
    "amd64": "{name}-linux-amd64",
    "arm64": "{name}-linux-arm64"
  }},
  "operations": [
    {{
      "name": "health",
      "tier": "read-only",
      "description": "Report plugin health."
    }}
  ]
}}
"#
    )
}

fn main_rs(name: &str) -> String {
    let lib_name = name.replace('-', "_");
    format!(
        r#"use std::process::ExitCode;

use alien_operations_sdk::run_plugin;
use {lib_name}::Plugin;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {{
    run_plugin(Plugin).await
}}
"#
    )
}

fn lib_rs(name: &str) -> String {
    format!(
        r#"//! The {name} plugin.

use alien_operations_sdk::{{Plugin as PluginTrait, PluginInvocation, PluginResult}};
use async_trait::async_trait;

/// The {name} plugin. Implements [`PluginTrait`]; [`alien_operations_sdk::run_plugin`]
/// owns stdin/stdout and protocol-version checking around [`Plugin::handle`].
pub struct Plugin;

#[async_trait]
impl PluginTrait for Plugin {{
    async fn handle(&self, invocation: &PluginInvocation) -> PluginResult {{
        match invocation.operation.as_str() {{
            "health" => PluginResult::success_json(&serde_json::json!({{ "status": "healthy" }}))
                .expect("static health payload always serializes"),
            other => PluginResult::error(
                "OPERATION_UNKNOWN",
                format!("{name} plugin has no operation '{{other}}'"),
            ),
        }}
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[tokio::test]
    async fn health_reports_healthy() {{
        let invocation = PluginInvocation::inline_json("health", b"{{}}");
        let result = Plugin.handle(&invocation).await;
        assert!(result.is_success());
    }}

    #[tokio::test]
    async fn unknown_operation_is_an_error() {{
        let invocation = PluginInvocation::inline_json("does-not-exist", b"{{}}");
        let result = Plugin.handle(&invocation).await;
        assert!(result.is_error());
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_names_with_uppercase_or_underscores() {
        for name in ["MyPlugin", "my_plugin", "-leading-hyphen", "1starts-with-digit", ""] {
            assert!(validate_plugin_name(name).is_err(), "{name} should be rejected");
        }
    }

    #[test]
    fn accepts_kebab_case_names() {
        for name in ["postgres", "my-plugin", "plugin2"] {
            assert!(validate_plugin_name(name).is_ok(), "{name} should be accepted");
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

        let metadata_bytes = std::fs::read(target.join("metadata.json")).expect("read metadata");
        let manifest = alien_operations_sdk::PluginManifest::parse_and_validate(&metadata_bytes)
            .expect("scaffolded metadata.json should be a valid manifest");
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.operations.len(), 1);
        assert_eq!(manifest.operations[0].name, "health");
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
        let workspace_line_index = contents.lines().position(|line| line == workspace_line).unwrap();
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

        let err = init_task(
            "existing",
            Some(target.to_str().expect("utf8 path")),
            false,
        )
        .expect_err("must refuse to overwrite");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }
}

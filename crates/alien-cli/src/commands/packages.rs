//! Platform package inspection and artifact download commands.

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;

#[derive(Parser, Debug, Clone)]
#[command(about = "List, inspect, and download project packages")]
pub struct PackagesArgs {
    #[command(subcommand)]
    pub action: PackagesAction,

    /// Project ID or name. Defaults to the linked project.
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum PackagesAction {
    /// List packages for a project.
    List {
        /// Filter by package type.
        #[arg(long = "type")]
        package_type: Option<String>,
        /// Filter by package status.
        #[arg(long)]
        status: Option<String>,
        /// Search package type or version.
        #[arg(long)]
        search: Option<String>,
    },
    /// Show one package and its outputs.
    Get {
        /// Package ID.
        id: String,
    },
    /// Download an artifact from a ready package.
    Download {
        /// Package ID.
        id: String,
        /// Artifact path or unique suffix, such as `binaries.linux-x64` or `linux-x64`.
        #[arg(long)]
        artifact: Option<String>,
        /// Destination file. Defaults to the artifact URL filename.
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        /// Replace an existing destination file.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Artifact {
    path: String,
    url: String,
    checksum: Option<String>,
}

pub async fn packages_task(args: PackagesArgs, ctx: ExecutionMode) -> Result<()> {
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let (_, project_link) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let project = project_link.project_id;

    match args.action {
        PackagesAction::List {
            package_type,
            status,
            search,
        } => {
            let mut url = package_api_url(&auth.base_url, "/v1/packages", &workspace)?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("project", &project);
                if let Some(package_type) = package_type {
                    query.append_pair("type", &package_type);
                }
                if let Some(status) = status {
                    query.append_pair("status", &status);
                }
                if let Some(search) = search {
                    query.append_pair("search", &search);
                }
            }
            let body = get_json(&auth, url, "listing packages").await?;
            if args.json {
                print_json(&body)?;
            } else {
                let items = body
                    .get("items")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                if items.is_empty() {
                    println!("No packages found.");
                } else {
                    for package in items {
                        println!(
                            "{}  {}  {}  {}",
                            string_field(package, "id"),
                            string_field(package, "type"),
                            string_field(package, "version"),
                            string_field(package, "status")
                        );
                    }
                }
            }
        }
        PackagesAction::Get { id } => {
            let url = package_api_url(&auth.base_url, &format!("/v1/packages/{id}"), &workspace)?;
            let package = get_json(&auth, url, "getting package").await?;
            if args.json {
                print_json(&package)?;
            } else {
                println!("ID:      {}", string_field(&package, "id"));
                println!("Type:    {}", string_field(&package, "type"));
                println!("Version: {}", string_field(&package, "version"));
                println!("Status:  {}", string_field(&package, "status"));
                let artifacts = package
                    .get("outputs")
                    .map(collect_artifacts)
                    .unwrap_or_default();
                if !artifacts.is_empty() {
                    println!("Artifacts:");
                    for artifact in artifacts {
                        println!("  {}", artifact.path);
                    }
                }
            }
        }
        PackagesAction::Download {
            id,
            artifact,
            output,
            force,
        } => {
            let url = package_api_url(&auth.base_url, &format!("/v1/packages/{id}"), &workspace)?;
            let package = get_json(&auth, url, "getting package").await?;
            let artifacts = package
                .get("outputs")
                .map(collect_artifacts)
                .unwrap_or_default();
            let selected = select_artifact(&artifacts, artifact.as_deref())?;
            let destination = output.unwrap_or_else(|| artifact_filename(selected));
            download_artifact(selected, &destination, force).await?;
            if args.json {
                print_json(&serde_json::json!({
                    "packageId": id,
                    "artifact": selected.path,
                    "path": destination,
                    "sha256": selected.checksum,
                }))?;
            } else {
                println!("Downloaded {} to {}", selected.path, destination.display());
            }
        }
    }

    Ok(())
}

async fn get_json(
    auth: &crate::auth::AuthHttp,
    url: reqwest::Url,
    operation: &str,
) -> Result<Value> {
    let response = auth
        .reqwest_client()
        .get(url.clone())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: operation.to_string(),
            url: Some(url.to_string()),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("{operation} failed ({status}): {body}"),
            url: Some(url.to_string()),
        }));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("parsing response while {operation}"),
            url: Some(url.to_string()),
        })
}

async fn download_artifact(artifact: &Artifact, destination: &Path, force: bool) -> Result<()> {
    if destination.exists() && !force {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Destination '{}' already exists; pass --force to replace it.",
                destination.display()
            ),
        }));
    }
    // Artifact URLs can point at a different host (for example, object
    // storage). Never forward the platform API bearer token to that host.
    let response = reqwest::Client::new()
        .get(&artifact.url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("downloading artifact '{}'", artifact.path),
            url: Some(artifact.url.clone()),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("artifact download failed ({status})"),
            url: Some(artifact.url.clone()),
        }));
    }
    let bytes = response
        .bytes()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("reading artifact '{}'", artifact.path),
            url: Some(artifact.url.clone()),
        })?;
    if let Some(expected) = &artifact.checksum {
        let actual = hex::encode(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Checksum mismatch for '{}': expected {}, received {}.",
                    artifact.path, expected, actual
                ),
            }));
        }
    }
    if let Some(parent) = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "create directory".to_string(),
                file_path: parent.display().to_string(),
                reason: "Failed to create artifact destination directory".to_string(),
            })?;
    }
    tokio::fs::write(destination, &bytes)
        .await
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: destination.display().to_string(),
            reason: "Failed to write downloaded artifact".to_string(),
        })
}

fn package_api_url(base_url: &str, path: &str, workspace: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(&format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "platform base URL is invalid".to_string(),
    })?;
    url.query_pairs_mut().append_pair("workspace", workspace);
    Ok(url)
}

fn collect_artifacts(outputs: &Value) -> Vec<Artifact> {
    fn visit(value: &Value, path: &mut Vec<String>, artifacts: &mut Vec<Artifact>) {
        let Some(object) = value.as_object() else {
            return;
        };
        for (key, child) in object {
            path.push(key.clone());
            if matches!(key.as_str(), "url" | "downloadUrl" | "templateUrl") {
                if let Some(url) = child.as_str().filter(|url| url.starts_with("http")) {
                    let artifact_path = path[..path.len() - 1].join(".");
                    let checksum = object
                        .get("sha256")
                        .or_else(|| object.get("shasum"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    artifacts.push(Artifact {
                        path: artifact_path,
                        url: url.to_string(),
                        checksum,
                    });
                }
            } else {
                visit(child, path, artifacts);
            }
            path.pop();
        }
    }

    let mut artifacts = Vec::new();
    visit(outputs, &mut Vec::new(), &mut artifacts);
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    artifacts.dedup_by(|left, right| left.path == right.path && left.url == right.url);
    artifacts
}

fn select_artifact<'a>(artifacts: &'a [Artifact], requested: Option<&str>) -> Result<&'a Artifact> {
    if artifacts.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "This package has no downloadable artifacts.".to_string(),
        }));
    }
    if let Some(requested) = requested {
        let matches = artifacts
            .iter()
            .filter(|artifact| {
                artifact.path == requested || artifact.path.ends_with(&format!(".{requested}"))
            })
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [artifact] => Ok(*artifact),
            [] => Err(artifact_selection_error(
                artifacts,
                format!("Artifact '{requested}' was not found."),
            )),
            _ => Err(artifact_selection_error(
                artifacts,
                format!("Artifact selector '{requested}' is ambiguous."),
            )),
        };
    }
    if artifacts.len() == 1 {
        return Ok(&artifacts[0]);
    }
    if let Some(native_target) = native_binary_target() {
        let suffix = format!(".{native_target}");
        let native = artifacts
            .iter()
            .filter(|artifact| artifact.path == native_target || artifact.path.ends_with(&suffix))
            .collect::<Vec<_>>();
        if let [artifact] = native.as_slice() {
            return Ok(*artifact);
        }
    }
    Err(artifact_selection_error(
        artifacts,
        "The package contains multiple downloadable artifacts.".to_string(),
    ))
}

fn artifact_selection_error(artifacts: &[Artifact], reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ConfigurationError {
        message: format!(
            "{reason} Pass --artifact with one of: {}",
            artifacts
                .iter()
                .map(|artifact| artifact.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}

fn native_binary_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("windows", "x86_64") => Some("windows-x64"),
        _ => None,
    }
}

fn artifact_filename(artifact: &Artifact) -> PathBuf {
    reqwest::Url::parse(&artifact.url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_string))
        .filter(|name| !name.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(artifact.path.replace('.', "-")))
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or("-")
}

fn print_json(value: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize JSON output".to_string(),
            })?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: &str) -> Artifact {
        Artifact {
            path: path.to_string(),
            url: format!("https://packages.example/{path}"),
            checksum: None,
        }
    }

    #[test]
    fn collects_downloadable_outputs_with_checksums() {
        let outputs = serde_json::json!({
            "type": "cli",
            "binaries": {
                "linux-x64": {
                    "url": "https://packages.example/tool-linux-x64",
                    "sha256": "abc"
                }
            },
            "buildInfo": { "source": "ignored" }
        });
        assert_eq!(
            collect_artifacts(&outputs),
            vec![Artifact {
                path: "binaries.linux-x64".to_string(),
                url: "https://packages.example/tool-linux-x64".to_string(),
                checksum: Some("abc".to_string()),
            }]
        );
    }

    #[test]
    fn explicit_artifact_suffix_must_be_unique() {
        let artifacts = vec![artifact("modules.aws"), artifact("modules.gcp")];
        assert_eq!(
            select_artifact(&artifacts, Some("aws")).unwrap().path,
            "modules.aws"
        );
        assert!(select_artifact(&artifacts, Some("missing")).is_err());
    }

    #[test]
    fn multiple_artifacts_require_a_selector_when_no_native_match_exists() {
        let artifacts = vec![artifact("modules.aws"), artifact("modules.gcp")];
        let error = select_artifact(&artifacts, None).unwrap_err();
        assert!(error.message.contains("--artifact"));
    }

    #[test]
    fn package_api_url_preserves_base_path_prefix() {
        let url = package_api_url(
            "https://platform.example/proxy/api/",
            "/v1/packages/pkg_123",
            "example-workspace",
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "https://platform.example/proxy/api/v1/packages/pkg_123?workspace=example-workspace"
        );
    }
}

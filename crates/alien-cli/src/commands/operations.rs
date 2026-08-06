//! CLI commands for operations plugins.
//!
//! Operations plugins package named operations (`plugin/operation`) that run
//! inside a deployment via the commands interface. `publish` uploads a custom
//! plugin bundle (a ZIP with `metadata.json` + per-arch binaries) to the
//! platform so a workspace can use its operations; `list` shows the catalog.
//!
//! Platform-gated: these talk to the Alien platform API, not a standalone
//! manager.

use std::io::Read;
use std::path::PathBuf;

use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage operations plugins",
    long_about = "Manage operations plugins.

Operations plugins package named operations you can run inside a deployment via
the commands interface (`plugin/operation`). Publish a custom bundle to make its
operations available in your workspace.

EXAMPLES:
    # Publish a custom plugin bundle
    alien operations publish ./postgres-operations-1.0.0.zip

    # List available plugins (builtin + custom)
    alien operations list
"
)]
pub struct OperationsArgs {
    #[command(subcommand)]
    pub action: OperationsAction,

    /// Project ID or name. Defaults to the linked project.
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum OperationsAction {
    /// Publish a custom operations plugin bundle (ZIP) to your workspace.
    Publish {
        /// Path to the plugin bundle ZIP (contains metadata.json + binaries).
        bundle: PathBuf,
    },
    /// List available operations plugins (builtin + custom).
    List,
}

/// The `metadata.json` fields the CLI reads to describe the bundle. The full
/// object is forwarded to the platform, which validates it authoritatively.
#[derive(Debug, Deserialize)]
struct BundleMetadata {
    name: String,
    version: String,
    #[serde(default)]
    tier: Option<String>,
}

/// Step 1: ask the platform for a presigned S3 URL to upload the bundle ZIP to.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadUrlRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadUrlResponse {
    /// Presigned S3 PUT URL to upload the ZIP to.
    upload_url: String,
    /// Content-Type header the PUT must send (must match the presign signature).
    content_type: String,
}

/// Step 2 (after the S3 upload): register the plugin. The bytes are already in
/// S3; only metadata travels here.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest {
    name: String,
    version: String,
    tier: String,
    /// The full, verbatim metadata.json object (platform re-validates it).
    metadata: Value,
}

pub async fn operations_task(args: OperationsArgs, ctx: ExecutionMode) -> Result<()> {
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    // The operations catalog is project-scoped: the platform requires a
    // `project` alongside `workspace`. Resolve the linked project (or the
    // `--project` override) the same way the other project-scoped commands do.
    let (_, project_link) = ctx.resolve_project(args.project.as_deref(), !args.json).await?;
    let project = project_link.project_id;

    match args.action {
        OperationsAction::Publish { bundle } => {
            publish_task(&auth, &workspace, &project, &bundle, args.json).await
        }
        OperationsAction::List => list_task(&auth, &workspace, &project, args.json).await,
    }
}

/// Read + validate the bundle, upload the ZIP straight to S3 via a presigned
/// URL the platform mints, then register the plugin. The bytes never flow
/// through the API.
async fn publish_task(
    auth: &crate::auth::AuthHttp,
    workspace: &str,
    project: &str,
    bundle_path: &PathBuf,
    json: bool,
) -> Result<()> {
    let bytes = std::fs::read(bundle_path)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("could not read bundle '{}'", bundle_path.display()),
        })?;

    let (metadata_value, metadata) = read_bundle_metadata(&bytes, bundle_path)?;
    let tier = metadata.tier.clone().unwrap_or_else(|| "destructive".to_string());

    // Step 1: get a presigned S3 PUT URL for this plugin's bundle.
    let upload_url_endpoint =
        api_url(&auth.base_url, "/v1/operations/plugins/upload-url", workspace, project)?;
    let presign_response = auth
        .reqwest_client()
        .request(Method::POST, upload_url_endpoint.clone())
        .json(&UploadUrlRequest {
            name: metadata.name.clone(),
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "requesting a bundle upload URL".to_string(),
            url: Some(upload_url_endpoint.to_string()),
        })?;
    if !presign_response.status().is_success() {
        let status = presign_response.status();
        let body = presign_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("could not get upload URL ({status}): {body}"),
            url: Some(upload_url_endpoint.to_string()),
        }));
    }
    let presign: UploadUrlResponse =
        presign_response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "parsing the bundle upload URL response".to_string(),
                url: Some(upload_url_endpoint.to_string()),
            })?;

    // Step 2: PUT the ZIP directly to S3. The Content-Type MUST match what the
    // presign was signed with, or S3 rejects the signature.
    let put_response = auth
        .reqwest_client()
        .request(Method::PUT, &presign.upload_url)
        .header("content-type", &presign.content_type)
        .body(bytes)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "uploading the bundle to storage".to_string(),
            url: Some(presign.upload_url.clone()),
        })?;
    if !put_response.status().is_success() {
        let status = put_response.status();
        let body = put_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("bundle upload failed ({status}): {body}"),
            url: Some(presign.upload_url.clone()),
        }));
    }

    // Step 3: register the plugin now that its ZIP is in S3.
    let publish_endpoint = api_url(&auth.base_url, "/v1/operations/plugins", workspace, project)?;
    let response = auth
        .reqwest_client()
        .request(Method::POST, publish_endpoint.clone())
        .json(&PublishRequest {
            name: metadata.name.clone(),
            version: metadata.version.clone(),
            tier,
            metadata: metadata_value,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "publishing operations plugin".to_string(),
            url: Some(publish_endpoint.to_string()),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("publish failed ({status}): {body}"),
            url: Some(publish_endpoint.to_string()),
        }));
    }

    if json {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    } else {
        println!(
            "Published plugin '{}' v{} to workspace '{}'.",
            metadata.name, metadata.version, workspace
        );
    }
    Ok(())
}

async fn list_task(
    auth: &crate::auth::AuthHttp,
    workspace: &str,
    project: &str,
    json: bool,
) -> Result<()> {
    let url = api_url(&auth.base_url, "/v1/operations/plugins", workspace, project)?;
    let response = auth
        .reqwest_client()
        .request(Method::GET, url.clone())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing operations plugins".to_string(),
            url: Some(url.to_string()),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("list failed ({status}): {body}"),
            url: Some(url.to_string()),
        }));
    }

    let body: Value = response.json().await.unwrap_or(Value::Null);
    if json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_default());
    } else {
        let plugins = body.get("plugins").and_then(Value::as_array);
        match plugins {
            Some(plugins) if !plugins.is_empty() => {
                for p in plugins {
                    let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
                    let version = p.get("version").and_then(Value::as_str).unwrap_or("?");
                    let builtin = p.get("builtin").and_then(Value::as_bool).unwrap_or(false);
                    let tier = p.get("tier").and_then(Value::as_str).unwrap_or("?");
                    let kind = if builtin { "builtin" } else { "custom" };
                    println!("{name}  v{version}  [{kind}, {tier}]");
                }
            }
            _ => println!("No operations plugins available."),
        }
    }
    Ok(())
}

/// Extract and validate `metadata.json` from the bundle ZIP. Returns the raw
/// JSON value (forwarded verbatim) plus the fields the CLI needs.
fn read_bundle_metadata(bytes: &[u8], path: &PathBuf) -> Result<(Value, BundleMetadata)> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("'{}' is not a valid ZIP bundle", path.display()),
        })?;
    let mut file = archive.by_name("metadata.json").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("bundle '{}' has no metadata.json", path.display()),
        })
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not read metadata.json from bundle".to_string(),
        })?;
    let value: Value =
        serde_json::from_str(&contents)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "metadata.json is not valid JSON".to_string(),
            })?;
    let metadata: BundleMetadata = serde_json::from_value(value.clone())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "metadata.json is missing required fields (name, version)".to_string(),
        })?;
    Ok((value, metadata))
}

fn api_url(base_url: &str, path: &str, workspace: &str, project: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base_url)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "platform base URL is invalid".to_string(),
        })?;
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("workspace", workspace)
        .append_pair("project", project);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn bundle_with_metadata(meta: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("metadata.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(serde_json::to_vec(meta).unwrap().as_slice())
                .unwrap();
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn reads_metadata_from_bundle() {
        let meta = serde_json::json!({
            "name": "postgres-operations",
            "version": "1.0.0",
            "tier": "mutating",
            "binaries": { "amd64": "postgres-operations-linux-amd64" },
            "operations": [ { "name": "vacuum" } ]
        });
        let bytes = bundle_with_metadata(&meta);
        let (value, parsed) =
            read_bundle_metadata(&bytes, &PathBuf::from("x.zip")).expect("valid bundle");
        assert_eq!(parsed.name, "postgres-operations");
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.tier.as_deref(), Some("mutating"));
        // The full object is forwarded verbatim (operations[] preserved).
        assert!(value.get("operations").is_some());
    }

    #[test]
    fn rejects_bundle_without_metadata() {
        // A ZIP with no metadata.json.
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("binary", SimpleFileOptions::default()).unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.finish().unwrap();
        }
        let err = read_bundle_metadata(&buf, &PathBuf::from("x.zip")).expect_err("must error");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }

    #[test]
    fn rejects_metadata_missing_required_fields() {
        let meta = serde_json::json!({ "binaries": {} });
        let bytes = bundle_with_metadata(&meta);
        let err = read_bundle_metadata(&bytes, &PathBuf::from("x.zip")).expect_err("must error");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }
}

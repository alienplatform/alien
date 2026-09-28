use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::success_line;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Subcommand;
use reqwest::Method;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Subcommand, Debug, Clone)]
pub enum PackageCommand {
    /// Print the project's installation package settings as JSON
    Get {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
    },
    /// Merge settings from JSON; omitted fields are preserved
    Update {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
        /// JSON patch file; use - to read standard input. Set a package to null to remove it.
        #[arg(long)]
        file: PathBuf,
    },
}

pub async fn packages_task(
    command: &PackageCommand,
    ctx: &ExecutionMode,
    json: bool,
) -> Result<()> {
    // Read and validate before authentication: invalid files must never open a browser.
    let (project, patch) = match command {
        PackageCommand::Get { project } => (project, None),
        PackageCommand::Update { project, file } => (project, Some(read_patch(file)?)),
    };
    let http = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_query_with_bootstrap(!json).await?;
    let (project, _) = ctx.resolve_project(project.as_deref(), !json).await?;
    let settings = request_settings(&http, workspace.as_deref(), &project, patch.as_ref()).await?;
    if patch.is_none() || json {
        print_json(&settings)?;
    } else {
        println!(
            "{}",
            success_line(
                "Package settings saved. New installation packages will use these settings."
            )
        );
    }
    Ok(())
}

fn read_patch(path: &Path) -> Result<Value> {
    let mut text = String::new();
    if path == Path::new("-") {
        std::io::stdin()
            .read_to_string(&mut text)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: "stdin".to_string(),
                reason: "Could not read package settings".to_string(),
            })?;
    } else {
        text = std::fs::read_to_string(path).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: path.display().to_string(),
                reason: "Could not read package settings".to_string(),
            },
        )?;
    }
    let value: Value =
        serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "Package settings must be a JSON object".to_string(),
            })?;
    if !value.is_object() {
        return Err(AlienError::new(ErrorData::ValidationError { field: "file".to_string(), message: "Provide a JSON object with cli, operatorImage, helm, terraform, or cloudformation settings".to_string() }));
    }
    Ok(value)
}

/// Keep package settings as JSON so new server fields survive a read/edit cycle.
async fn request_settings(
    http: &AuthHttp,
    workspace: Option<&str>,
    project: &str,
    patch: Option<&Value>,
) -> Result<Value> {
    let mut url =
        Url::parse(&http.base_url)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid API base URL".to_string(),
            })?;
    url.set_query(None);
    url.set_fragment(None);
    let mut segments = url.path_segments_mut().map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "API base URL must support path segments".to_string(),
        })
    })?;
    segments.clear().extend(["v1", "projects", project]);
    drop(segments);
    if let Some(workspace) = workspace {
        url.query_pairs_mut().append_pair("workspace", workspace);
    }
    let method = if patch.is_some() {
        Method::PATCH
    } else {
        Method::GET
    };
    let mut request = http.reqwest_client().request(method, url.clone());
    if let Some(patch) = patch {
        request = request.json(&serde_json::json!({ "packagesConfigPatch": patch }));
    }
    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Reading or updating package settings".to_string(),
                url: Some(url.to_string()),
            })?;
    // Do not print an unexpected proxy/HTML error body, which may contain sensitive data.
    response
        .error_for_status_ref()
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Package settings request failed; check project access and configuration"
                .to_string(),
            url: Some(url.to_string()),
        })?;
    let project: Value =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "Expected a project response containing packagesConfig".to_string(),
            })?;
    project.get("packagesConfig").cloned().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "API response omitted packagesConfig".to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{build_auth_http, client_with_header};
    use axum::{
        extract::{Query, State},
        http::HeaderMap,
        routing::get,
        Json, Router,
    };
    use clap::Parser;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[derive(Parser)]
    struct Args {
        #[command(subcommand)]
        command: PackageCommand,
    }

    #[test]
    fn validates_files_and_requires_a_complete_update_command() {
        assert!(Args::try_parse_from(["packages", "update", "sample"]).is_err());
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("settings.json");
        for invalid in ["{", "null", "[]", "true"] {
            std::fs::write(&file, invalid).unwrap();
            read_patch(&file).expect_err("only objects can be patches");
        }
        std::fs::write(&file, r#"{"helm":{"logCollector":{"mode":"podApi"}}}"#).unwrap();
        assert_eq!(
            read_patch(&file).unwrap()["helm"]["logCollector"]["mode"],
            "podApi"
        );
    }

    #[tokio::test]
    async fn authenticates_scoped_reads_and_sends_only_the_patch() {
        let captured = Arc::new(Mutex::new(None::<Value>));
        let app = Router::new().route("/v1/projects/sample", get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
            assert_eq!(query.get("workspace").map(String::as_str), Some("sample-workspace"));
            assert_eq!(headers["authorization"], "Bearer test-key");
            Json(serde_json::json!({ "packagesConfig": { "helm": { "futureSetting": true } } }))
        }).patch(|State(state): State<Arc<Mutex<Option<Value>>>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>, Json(body): Json<Value>| async move {
            assert_eq!(query.get("workspace").map(String::as_str), Some("sample-workspace"));
            assert_eq!(headers["authorization"], "Bearer test-key");
            *state.lock().unwrap() = Some(body);
            Json(serde_json::json!({ "packagesConfig": { "helm": { "enabled": false } } }))
        })).with_state(captured.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let auth = build_auth_http(
            client_with_header("Bearer test-key").unwrap(),
            base_url,
            None,
        );
        assert_eq!(
            request_settings(&auth, Some("sample-workspace"), "sample", None)
                .await
                .unwrap()["helm"]["futureSetting"],
            true
        );
        let patch = serde_json::json!({ "helm": { "enabled": false } });
        assert_eq!(
            request_settings(&auth, Some("sample-workspace"), "sample", Some(&patch))
                .await
                .unwrap(),
            patch
        );
        assert_eq!(
            *captured.lock().unwrap(),
            Some(serde_json::json!({ "packagesConfigPatch": patch }))
        );
        server.abort();
    }
}

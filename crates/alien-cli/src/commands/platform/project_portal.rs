use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::success_line;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{
    convert_sdk_error, types::UpdateProjectGcpOAuthProvider, Error as SdkError, ResponseValue,
    SdkResultExt,
};
use clap::{Args, Subcommand};
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Subcommand, Debug, Clone)]
pub enum PortalCommand {
    /// Print appearance, domain status, and Google sign-in settings
    Get {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
    },
    /// Merge appearance settings; omitted fields are preserved
    #[command(
        long_about = "Merge appearance settings; omitted fields are preserved. The API replaces the full appearance object and does not support conditional writes. Run appearance edits sequentially: concurrent edits from other clients can overwrite each other."
    )]
    Set(Box<AppearanceArgs>),
    /// Inspect domain status or select an existing verified domain
    Domain {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
        /// Existing domain ID or hostname
        #[arg(long = "use", conflicts_with = "unset")]
        domain: Option<String>,
        /// Return to the default domain
        #[arg(long)]
        unset: bool,
    },
    /// Inspect Google sign-in or configure its OAuth web client
    GoogleSignIn {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
        /// Use the Alien-managed OAuth client
        #[arg(long, conflicts_with_all = ["client_id", "client_secret_file"])]
        alien_managed: bool,
        /// Google OAuth web client ID
        #[arg(long)]
        client_id: Option<String>,
        /// Read the write-only secret from a file or - for stdin
        #[arg(long, requires = "client_id")]
        client_secret_file: Option<PathBuf>,
    },
}

#[derive(Args, Debug, Clone, Default)]
pub struct AppearanceArgs {
    /// Project ID or name (defaults to the linked project)
    project: Option<String>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    subtitle: Option<String>,
    #[arg(long, value_parser = ["clean", "technical", "enterprise", "playful", "minimal"])]
    preset: Option<String>,
    #[arg(long, value_parser = ["blue", "purple", "green", "orange", "pink", "slate"])]
    accent_color: Option<String>,
    #[arg(long, value_parser = ["comfortable", "compact"])]
    density: Option<String>,
    #[arg(long)]
    avatar_url: Option<String>,
    #[arg(long)]
    favicon_url: Option<String>,
    #[arg(long)]
    support_url: Option<String>,
    #[arg(long)]
    docs_url: Option<String>,
    /// Reset a nullable field to its default; repeat for multiple fields
    #[arg(long, value_parser = ["title", "subtitle", "avatar-url", "favicon-url", "support-url", "docs-url"])]
    clear: Vec<String>,
    /// Partial JSON object; flags override the file. Use - for stdin.
    #[arg(long)]
    file: Option<PathBuf>,
}

fn invalid(field: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: field.to_string(),
        message: message.into(),
    })
}

fn read_text(path: &Path) -> Result<String> {
    let mut text = String::new();
    let result = if path == Path::new("-") {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(path).map(|value| text = value)
    };
    result
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Could not read portal settings input".to_string(),
        })?;
    Ok(text)
}

impl AppearanceArgs {
    fn patch(&self) -> Result<Map<String, Value>> {
        let mut patch = match &self.file {
            Some(path) => {
                let value: Value = serde_json::from_str(&read_text(path)?)
                    .into_alien_error()
                    .context(ErrorData::JsonError {
                        operation: "parse".to_string(),
                        reason: "Appearance must be a JSON object".to_string(),
                    })?;
                value
                    .as_object()
                    .cloned()
                    .ok_or_else(|| invalid("file", "Provide a JSON object"))?
            }
            None => Map::new(),
        };
        for (key, value) in [
            ("title", &self.title),
            ("subtitle", &self.subtitle),
            ("preset", &self.preset),
            ("accentColor", &self.accent_color),
            ("density", &self.density),
            ("avatarUrl", &self.avatar_url),
            ("faviconUrl", &self.favicon_url),
            ("supportUrl", &self.support_url),
            ("docsUrl", &self.docs_url),
        ] {
            if let Some(value) = value {
                patch.insert(key.to_string(), Value::String(value.clone()));
            }
        }
        for field in &self.clear {
            let key = match field.as_str() {
                "avatar-url" => "avatarUrl",
                "favicon-url" => "faviconUrl",
                "support-url" => "supportUrl",
                "docs-url" => "docsUrl",
                other => other,
            };
            patch.insert(key.to_string(), Value::Null);
        }
        if patch.is_empty() {
            return Err(invalid(
                "set",
                "Provide appearance flags or a nonempty --file object",
            ));
        }
        validate_appearance(&patch)?;
        Ok(patch)
    }
}

fn validate_appearance(patch: &Map<String, Value>) -> Result<()> {
    for (key, value) in patch {
        let choices: &[&str] = match key.as_str() {
            "preset" => &["clean", "technical", "enterprise", "playful", "minimal"],
            "accentColor" => &["blue", "purple", "green", "orange", "pink", "slate"],
            "density" => &["comfortable", "compact"],
            _ => &[],
        };
        if !choices.is_empty() {
            if !value.as_str().is_some_and(|v| choices.contains(&v)) {
                return Err(invalid(
                    key,
                    format!("Choose one of: {}", choices.join(", ")),
                ));
            }
            continue;
        }
        let max = match key.as_str() {
            "title" => 80,
            "subtitle" => 160,
            "avatarUrl" | "faviconUrl" | "supportUrl" | "docsUrl" => 2048,
            // Preserve fields introduced by newer servers, including in exported JSON.
            _ => continue,
        };
        if value.is_null() {
            continue;
        }
        let text = value
            .as_str()
            .ok_or_else(|| invalid(key, "Expected a string or null"))?;
        // The API counts JavaScript UTF-16 code units.
        if text.encode_utf16().count() > max {
            return Err(invalid(key, format!("Must be at most {max} characters")));
        }
        if key.ends_with("Url") {
            Url::parse(text)
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: key.clone(),
                    message: "Provide an absolute URL".to_string(),
                })?;
        }
    }
    Ok(())
}

fn merge_appearance(current: &Value, patch: &Map<String, Value>) -> Result<Value> {
    let mut merged = if current.is_null() {
        Map::new()
    } else {
        current
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("appearance", "API appearance must be an object or null"))?
    };
    merged.extend(patch.clone());
    Ok(Value::Object(merged))
}

fn api_error(message: &str) -> ErrorData {
    ErrorData::ApiRequestFailed {
        message: message.to_string(),
        url: None,
    }
}

fn as_json(value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Could not serialize portal settings".to_string(),
        })
}

async fn project_request(
    http: &AuthHttp,
    workspace: Option<&str>,
    project: &str,
    patch: Option<Value>,
) -> Result<Value> {
    let mut url =
        Url::parse(&http.base_url)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid API base URL".to_string(),
            })?;
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|_| invalid("api", "API URL must support path segments"))?
        .pop_if_empty()
        .extend(["v1", "projects", project]);
    if let Some(workspace) = workspace {
        url.query_pairs_mut().append_pair("workspace", workspace);
    }
    let mut request = http.reqwest_client().request(
        if patch.is_some() {
            Method::PATCH
        } else {
            Method::GET
        },
        url,
    );
    if let Some(patch) = patch {
        request = request.json(&patch);
    }
    let response = request
        .send()
        .await
        .into_alien_error()
        .context(api_error("Reading or updating portal settings"))?;
    let status = response.status();
    if !status.is_success() {
        // Only structured API errors are safe to show; discard proxy HTML and other bodies.
        let error =
            match ResponseValue::from_response::<alien_platform_api::types::ApiError>(response)
                .await
            {
                Ok(response) => convert_sdk_error(SdkError::ErrorResponse(response)),
                Err(_) => {
                    let mut error = AlienError::new(api_error(&format!(
                        "Portal settings request failed with HTTP {status}"
                    )));
                    error.http_status_code = Some(status.as_u16());
                    return Err(error);
                }
            };
        return Err(error).context(api_error("Reading or updating portal settings"));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "Expected a project response".to_string(),
        })
}

async fn save_appearance(
    http: &AuthHttp,
    workspace: Option<&str>,
    project: &str,
    patch: &Map<String, Value>,
) -> Result<Value> {
    let response = project_request(http, workspace, project, None).await?;
    let current = response.get("deploymentPortalAppearance").ok_or_else(|| {
        AlienError::new(api_error("API response omitted deploymentPortalAppearance"))
    })?;
    // This API has no conditional-write or partial-appearance contract. Preserve
    // fields from this read; concurrent editors still require external coordination.
    let merged = merge_appearance(current, patch)?;
    let response = project_request(
        http,
        workspace,
        project,
        Some(json!({"deploymentPortalAppearance": merged})),
    )
    .await?;
    let saved = response.get("deploymentPortalAppearance").ok_or_else(|| {
        AlienError::new(api_error("API response omitted deploymentPortalAppearance"))
    })?;
    for (field, expected) in patch {
        if saved.get(field) != Some(expected) {
            return Err(AlienError::new(api_error(&format!(
                "The API did not retain appearance field '{field}'. Some settings may have been saved; run `alien projects portal get` and check that the server supports this field."
            ))));
        }
    }
    Ok(saved.clone())
}

async fn domain_status(http: &AuthHttp, workspace: Option<&str>, project: &str) -> Result<Value> {
    let mut request = http
        .sdk_client()
        .get_project_deployment_portal_domain()
        .id_or_name(project);
    if let Some(workspace) = workspace {
        request = request.workspace(workspace);
    }
    as_json(
        request
            .send()
            .await
            .into_sdk_error()
            .context(api_error("Reading portal domain status"))?
            .into_inner(),
    )
}

async fn google_settings(
    http: &AuthHttp,
    workspace: Option<&str>,
    project: &str,
    body: Option<UpdateProjectGcpOAuthProvider>,
) -> Result<Value> {
    if let Some(body) = body {
        let mut request = http
            .sdk_client()
            .update_project_gcp_o_auth_provider()
            .id_or_name(project)
            .body(body);
        if let Some(workspace) = workspace {
            request = request.workspace(workspace);
        }
        as_json(
            request
                .send()
                .await
                .into_sdk_error()
                .context(api_error("Updating Google sign-in"))?
                .into_inner(),
        )
    } else {
        let mut request = http
            .sdk_client()
            .get_project_gcp_o_auth_provider()
            .id_or_name(project);
        if let Some(workspace) = workspace {
            request = request.workspace(workspace);
        }
        as_json(
            request
                .send()
                .await
                .into_sdk_error()
                .context(api_error("Reading Google sign-in"))?
                .into_inner(),
        )
    }
}

// Only access-denied and missing sections are optional; all other failures remain fatal.
fn readable_section(result: Result<Value>) -> Result<Value> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let detail = error.source.as_deref();
            let status = detail
                .and_then(|source| source.http_status_code)
                .or(error.http_status_code);
            if matches!(status, Some(403 | 404)) {
                let (code, message) = detail
                    .map(|source| (&source.code, &source.message))
                    .unwrap_or((&error.code, &error.message));
                Ok(json!({"error": {"code": code, "message": message, "httpStatusCode": status}}))
            } else {
                Err(error)
            }
        }
    }
}

async fn get_settings(http: &AuthHttp, workspace: Option<&str>, project: &str) -> Result<Value> {
    let appearance = project_request(http, workspace, project, None)
        .await
        .and_then(|response| {
            response
                .get("deploymentPortalAppearance")
                .cloned()
                .ok_or_else(|| {
                    invalid(
                        "appearance",
                        "API response omitted deploymentPortalAppearance",
                    )
                })
        });
    let appearance = readable_section(appearance)?;
    let domain = readable_section(domain_status(http, workspace, project).await)?;
    let google = readable_section(google_settings(http, workspace, project, None).await)?;
    Ok(json!({"appearance": appearance, "domain": domain, "googleSignIn": google}))
}

pub async fn portal_task(
    command: &PortalCommand,
    ctx: &ExecutionMode,
    json_output: bool,
) -> Result<()> {
    // All local validation and input reads precede authentication.
    let mut appearance_patch = None;
    let mut google_body = None;
    let project = match command {
        PortalCommand::Set(args) => {
            appearance_patch = Some(args.patch()?);
            &args.project
        }
        PortalCommand::GoogleSignIn {
            project,
            alien_managed,
            client_id,
            client_secret_file,
        } => {
            let body = if *alien_managed {
                Some(json!({"mode": "alien-managed"}))
            } else if let Some(id) = client_id {
                let mut body = json!({"mode": "custom", "clientId": id});
                if let Some(path) = client_secret_file {
                    let secret = read_text(path)?.trim_end_matches(['\r', '\n']).to_string();
                    if secret.is_empty() || secret.encode_utf16().count() > 512 {
                        return Err(invalid(
                            "client-secret-file",
                            "Secret must contain 1 to 512 characters",
                        ));
                    }
                    body["clientSecret"] = Value::String(secret);
                }
                Some(body)
            } else {
                None
            };
            google_body = body
                .map(|body| {
                    serde_json::from_value(body).into_alien_error().context(
                        ErrorData::ValidationError {
                            field: "google-sign-in".to_string(),
                            message: "Provide a valid Google OAuth web client ID".to_string(),
                        },
                    )
                })
                .transpose()?;
            project
        }
        PortalCommand::Get { project } | PortalCommand::Domain { project, .. } => project,
    };
    let http = ctx.auth_http().await?;
    let workspace = ctx
        .resolve_workspace_query_with_bootstrap(!json_output)
        .await?;
    let (project, _) = ctx
        .resolve_project(project.as_deref(), !json_output)
        .await?;
    let workspace = workspace.as_deref();
    let mut saved = false;
    let result = match command {
        PortalCommand::Get { .. } => get_settings(&http, workspace, &project).await?,
        PortalCommand::Set(_) => {
            let patch =
                appearance_patch.ok_or_else(|| invalid("set", "Missing appearance patch"))?;
            let result = save_appearance(&http, workspace, &project, &patch).await?;
            saved = true;
            result
        }
        PortalCommand::Domain { domain, unset, .. } => {
            if domain.is_some() || *unset {
                let domain_id = if let Some(domain) = domain {
                    let mut request = http.sdk_client().list_domains();
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    let response = as_json(
                        request
                            .send()
                            .await
                            .into_sdk_error()
                            .context(api_error("Finding an existing domain"))?
                            .into_inner(),
                    )?;
                    let domains = response["domains"]
                        .as_array()
                        .ok_or_else(|| invalid("domain", "API response omitted domains"))?;
                    let found = domains
                        .iter()
                        .find(|item| {
                            item["id"].as_str() == Some(domain)
                                || item["domain"].as_str().is_some_and(|host| {
                                    host.eq_ignore_ascii_case(domain.trim_end_matches('.'))
                                })
                        })
                        .ok_or_else(|| {
                            invalid(
                                "use",
                                "Domain not found; add and verify it before selecting it",
                            )
                        })?;
                    found["id"].clone()
                } else {
                    Value::Null
                };
                project_request(
                    &http,
                    workspace,
                    &project,
                    Some(json!({"domainId": domain_id})),
                )
                .await?;
                saved = true;
            }
            domain_status(&http, workspace, &project).await?
        }
        PortalCommand::GoogleSignIn { .. } => {
            saved = google_body.is_some();
            google_settings(&http, workspace, &project, google_body).await?
        }
    };
    if json_output {
        return print_json(&result);
    }
    if saved {
        println!(
            "{}",
            success_line(&format!(
                "Portal settings saved. Every setup link for {project} uses them."
            ))
        );
        println!("Check a setup link: alien onboard <name> --json | jq -r .deploymentLink");
    }
    match command {
        PortalCommand::Domain { .. } => print_domains(&result),
        PortalCommand::GoogleSignIn { .. } => print_google(&result),
        PortalCommand::Get { .. } => {
            println!("Appearance:");
            print_json(&result["appearance"])?;
            print_domains(&result["domain"])?;
            print_google(&result["googleSignIn"])
        }
        PortalCommand::Set(_) => Ok(()),
    }
}

fn print_domains(value: &Value) -> Result<()> {
    if let Some(error) = value.get("error") {
        println!(
            "Domains unavailable: {}: {}",
            error["code"], error["message"]
        );
        return Ok(());
    }
    for (key, label) in [
        ("deploymentPortalEndpoint", "Portal"),
        ("packageEndpoint", "Packages"),
    ] {
        let endpoint = &value[key];
        if endpoint.is_null() {
            println!("{label}: default domain");
        } else {
            let status = match endpoint["status"].as_str() {
                Some("waiting_for_domain" | "provisioning" | "waiting_for_health") => "Setting up",
                Some("waiting_for_dns") => "Waiting for DNS",
                Some("active") => "Active",
                Some("failed") => "Needs attention",
                Some("deleting") => "Removing",
                _ => return Err(invalid("domain", "API returned an unknown endpoint status")),
            };
            let hostname = endpoint["hostname"]
                .as_str()
                .ok_or_else(|| invalid("domain", "API omitted endpoint hostname"))?;
            println!("{label}: {hostname} — {status}");
        }
    }
    Ok(())
}

fn print_google(value: &Value) -> Result<()> {
    if let Some(error) = value.get("error") {
        println!(
            "Google sign-in unavailable: {}: {}",
            error["code"], error["message"]
        );
        return Ok(());
    }
    let mode = value["mode"]
        .as_str()
        .ok_or_else(|| invalid("google-sign-in", "API omitted mode"))?;
    println!("Google sign-in: {mode}");
    if let Some(id) = value["clientId"].as_str() {
        println!("Client ID: {id}");
    }
    let uris = value["redirectUris"]
        .as_array()
        .ok_or_else(|| invalid("google-sign-in", "API omitted redirect URIs"))?;
    println!("Authorized redirect URIs:");
    for uri in uris {
        let uri = uri
            .as_str()
            .ok_or_else(|| invalid("google-sign-in", "API returned an invalid redirect URI"))?;
        println!("  {uri}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::projects::{project_task, ProjectArgs};
    use super::*;
    use crate::auth::{build_auth_http, client_with_header};
    use axum::{extract::State, routing::get, Json, Router};
    use clap::Parser;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    #[test]
    fn flags_override_file_and_clear_resets_nullable_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("appearance.json");
        std::fs::write(
            &file,
            r#"{"title":"Old","subtitle":"Remove","future":{"enabled":true}}"#,
        )
        .unwrap();
        let args = AppearanceArgs {
            title: Some("Acme".to_string()),
            clear: vec!["subtitle".to_string()],
            file: Some(file),
            ..Default::default()
        };
        assert_eq!(
            args.patch().unwrap(),
            json!({"title":"Acme", "subtitle":null,"future":{"enabled":true}})
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[tokio::test]
    async fn rejects_invalid_input_before_authentication() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("appearance.json");
        let mode = ExecutionMode::Platform {
            base_url: "not-an-api-url".to_string(),
            api_key: None,
            no_browser: true,
            workspace: Some("sample-workspace".to_string()),
            project: Some("sample".to_string()),
        };
        for value in [
            json!({"preset":"unknown"}),
            json!({"density":null}),
            json!({"accentColor":"red"}),
            json!({"title":"x".repeat(81)}),
            json!({"subtitle":"x".repeat(161)}),
            json!({"avatarUrl":"relative.png"}),
            json!({"faviconUrl":"https://example.com/".repeat(110)}),
            json!({"title":true}),
            json!([]),
        ] {
            std::fs::write(&file, value.to_string()).unwrap();
            let args = ProjectArgs::try_parse_from([
                "projects",
                "portal",
                "set",
                "--file",
                file.to_str().unwrap(),
                "--json",
            ])
            .unwrap();
            let error = project_task(args, mode.clone())
                .await
                .expect_err("local validation must precede auth");
            assert_eq!(error.code, "VALIDATION_ERROR", "{error}");
        }
        for (flag, value) in [
            ("--preset", "unknown"),
            ("--accent-color", "red"),
            ("--density", "tiny"),
            ("--clear", "preset"),
        ] {
            assert!(
                ProjectArgs::try_parse_from(["projects", "portal", "set", flag, value]).is_err()
            );
        }
        let args = ProjectArgs::try_parse_from([
            "projects",
            "portal",
            "google-sign-in",
            "--client-id",
            "invalid",
            "--json",
        ])
        .unwrap();
        let error = project_task(args, mode)
            .await
            .expect_err("client ID must validate locally");
        assert_eq!(error.code, "VALIDATION_ERROR", "{error}");
    }

    #[tokio::test]
    async fn unexpected_http_errors_do_not_expose_response_bodies() {
        let app = Router::new().route(
            "/v1/projects/sample",
            get(|| async {
                (
                    axum::http::StatusCode::BAD_GATEWAY,
                    "<html>private-proxy-detail</html>",
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = build_auth_http(
            client_with_header("Bearer test-key").unwrap(),
            format!("http://{}", listener.local_addr().unwrap()),
            None,
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = project_request(&http, None, "sample", None)
            .await
            .expect_err("proxy failure must fail the command");
        assert_eq!(error.code, "API_REQUEST_FAILED");
        let error = serde_json::to_string(&error).unwrap();
        assert!(!error.contains("private-proxy-detail"));
        assert!(!error.contains("<html>"));
        server.abort();
    }

    #[tokio::test]
    async fn api_errors_preserve_details_and_only_access_failures_allow_partial_reads() {
        for (status, code) in [
            (400, "DOMAIN_NOT_VERIFIED"),
            (400, "VALIDATION_ERROR"),
            (422, "REQUEST_VALIDATION_ERROR"),
            (403, "FORBIDDEN"),
            (404, "PROJECT_NOT_FOUND"),
            (500, "INTERNAL_ERROR"),
        ] {
            let app = Router::new().route(
                "/v1/projects/sample",
                get(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        Json(json!({
                            "code": code, "message": "Actionable API message",
                            "retryable": false, "internal": false,
                            "hint": "Check configuration", "requestId": "req_example"
                        })),
                    )
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let http = build_auth_http(
                client_with_header("Bearer test-key").unwrap(),
                format!("http://{}", listener.local_addr().unwrap()),
                None,
            );
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let error = project_request(&http, None, "sample", None)
                .await
                .unwrap_err();
            let source = error.source.as_ref().unwrap();
            assert_eq!(source.code, code);
            assert_eq!(source.message, "Actionable API message");
            assert_eq!(source.http_status_code, Some(status));
            assert_eq!(source.hint.as_deref(), Some("Check configuration"));
            assert_eq!(source.context.as_ref().unwrap()["requestId"], "req_example");
            let section = readable_section(Err(error));
            if matches!(status, 403 | 404) {
                assert_eq!(section.unwrap()["error"]["code"], code);
            } else {
                assert!(section.is_err());
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn get_keeps_readable_sections_when_google_settings_are_forbidden() {
        let app = Router::new()
            .route(
                "/v1/projects/sample",
                get(|| async { Json(json!({"deploymentPortalAppearance": {"title": "Acme"}})) }),
            )
            .route(
                "/v1/projects/sample/deployment-portal-domain",
                get(|| async {
                    Json(json!({"deploymentPortalEndpoint": null, "packageEndpoint": null}))
                }),
            )
            .route(
                "/v1/projects/sample/gcp-oauth-provider",
                get(|| async {
                    (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(json!({
                            "code": "FORBIDDEN", "message": "Administrator required",
                            "retryable": false, "internal": false
                        })),
                    )
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = build_auth_http(
            client_with_header("Bearer test-key").unwrap(),
            format!("http://{}", listener.local_addr().unwrap()),
            None,
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let settings = get_settings(&http, None, "sample").await.unwrap();
        assert_eq!(settings["appearance"]["title"], "Acme");
        assert_eq!(
            settings["domain"],
            json!({"deploymentPortalEndpoint": null, "packageEndpoint": null})
        );
        assert_eq!(settings["googleSignIn"]["error"]["httpStatusCode"], 403);
        print_domains(&settings["domain"]).unwrap();
        print_google(&settings["googleSignIn"]).unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn refuses_success_when_an_older_server_drops_favicon() {
        let app = Router::new().route(
            "/v1/projects/sample",
            get(|| async { Json(json!({"deploymentPortalAppearance":{"title":"Old"}})) }).patch(
                |Json(body): Json<Value>| async move {
                    let mut appearance = body["deploymentPortalAppearance"].clone();
                    appearance.as_object_mut().unwrap().remove("faviconUrl");
                    Json(json!({"deploymentPortalAppearance":appearance}))
                },
            ),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = build_auth_http(
            client_with_header("Bearer test-key").unwrap(),
            format!("http://{}", listener.local_addr().unwrap()),
            None,
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let args = AppearanceArgs {
            title: Some("Acme".to_string()),
            favicon_url: Some("https://example.com/favicon.png".to_string()),
            ..Default::default()
        };
        let error = save_appearance(&http, None, "sample", &args.patch().unwrap())
            .await
            .expect_err("a discarded favicon must not be reported as saved");
        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert!(error.to_string().contains("faviconUrl"));
        assert!(error
            .to_string()
            .contains("Some settings may have been saved"));
        server.abort();
    }

    #[tokio::test]
    async fn get_merge_set_preserves_unknown_and_unset_fields() {
        let initial = json!({"title":"Old", "subtitle":"Keep", "accentColor":"purple", "future":{"nested": [1,2]}});
        let state = Arc::new(Mutex::new(initial.clone()));
        let app = Router::new()
            .route(
                "/api/v1/projects/sample",
                get(|State(state): State<Arc<Mutex<Value>>>| async move {
                    Json(json!({"deploymentPortalAppearance": state.lock().unwrap().clone()}))
                })
                .patch(
                    |State(state): State<Arc<Mutex<Value>>>, Json(body): Json<Value>| async move {
                        assert_eq!(body.as_object().unwrap().len(), 1);
                        *state.lock().unwrap() = body["deploymentPortalAppearance"].clone();
                        Json(body)
                    },
                ),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = build_auth_http(
            client_with_header("Bearer test-key").unwrap(),
            format!("http://{}/api/", listener.local_addr().unwrap()),
            None,
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let args = AppearanceArgs {
            title: Some("Acme".to_string()),
            clear: vec!["subtitle".to_string()],
            ..Default::default()
        };
        let saved = save_appearance(
            &http,
            Some("sample-workspace"),
            "sample",
            &args.patch().unwrap(),
        )
        .await
        .unwrap();
        let expected = json!({"title":"Acme", "subtitle":null, "accentColor":"purple", "future":{"nested":[1,2]}});
        assert_eq!(saved, expected);
        assert_eq!(*state.lock().unwrap(), expected);
        server.abort();
    }
}

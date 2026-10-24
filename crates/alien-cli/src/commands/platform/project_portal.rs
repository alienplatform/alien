use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::success_line;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{types::UpdateProjectGcpOAuthProvider, SdkResultExt};
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
    // Proxy responses may contain sensitive HTML; never include their body in errors.
    response
        .error_for_status_ref()
        .into_alien_error()
        .context(api_error(
            "Portal settings request failed; check project access and configuration",
        ))?;
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "Expected a project response".to_string(),
        })
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
        PortalCommand::Get { .. } => {
            let response = project_request(&http, workspace, &project, None).await?;
            let appearance = response.get("deploymentPortalAppearance").ok_or_else(|| {
                invalid(
                    "appearance",
                    "API response omitted deploymentPortalAppearance",
                )
            })?;
            json!({"appearance": appearance, "domain": domain_status(&http, workspace, &project).await?, "googleSignIn": google_settings(&http, workspace, &project, None).await?})
        }
        PortalCommand::Set(_) => {
            let response = project_request(&http, workspace, &project, None).await?;
            let current = response.get("deploymentPortalAppearance").ok_or_else(|| {
                invalid(
                    "appearance",
                    "API response omitted deploymentPortalAppearance",
                )
            })?;
            let patch =
                appearance_patch.ok_or_else(|| invalid("set", "Missing appearance patch"))?;
            let merged = merge_appearance(current, &patch)?;
            let response = project_request(
                &http,
                workspace,
                &project,
                Some(json!({"deploymentPortalAppearance": merged})),
            )
            .await?;
            saved = true;
            response
                .get("deploymentPortalAppearance")
                .cloned()
                .ok_or_else(|| {
                    invalid(
                        "appearance",
                        "API response omitted deploymentPortalAppearance",
                    )
                })?
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
                    if found["status"] != "verified" {
                        return Err(invalid(
                            "use",
                            "Domain must be verified before selecting it",
                        ));
                    }
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
        let response = project_request(&http, Some("sample-workspace"), "sample", None)
            .await
            .unwrap();
        let merged = merge_appearance(
            &response["deploymentPortalAppearance"],
            &args.patch().unwrap(),
        )
        .unwrap();
        let saved = project_request(
            &http,
            Some("sample-workspace"),
            "sample",
            Some(json!({"deploymentPortalAppearance":merged})),
        )
        .await
        .unwrap();
        let expected = json!({"title":"Acme", "subtitle":null, "accentColor":"purple", "future":{"nested":[1,2]}});
        assert_eq!(saved["deploymentPortalAppearance"], expected);
        assert_eq!(*state.lock().unwrap(), expected);
        server.abort();
    }
}

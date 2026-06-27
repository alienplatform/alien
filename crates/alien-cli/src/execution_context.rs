//! Execution context for CLI operation.
//!
//! The CLI talks to a manager. Period.
//!
//! Mode resolution:
//! - `alien dev ...` → Dev mode (localhost, no auth, auto-starts manager)
//! - `ALIEN_MANAGER_URL` env var → standalone alien-manager instance
//! - `#[cfg(feature = "platform")]`: if authenticated, auto-resolve to manager.alien.dev
//! - else → error (no manager URL)
//!
//! Platform-specific features (OAuth login, workspaces, projects, link/unlink) are
//! behind `#[cfg(feature = "platform")]`.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_manager_api::Client as ServerSdkClient;
#[cfg(feature = "platform")]
use alien_platform_api::types::Subject;
use alien_platform_api::Client as SdkClient;
#[cfg(feature = "platform")]
use alien_platform_api::SdkResultExt as _;
use tokio::time::Duration;
use tracing::{info, warn};

#[cfg(feature = "platform")]
use crate::auth::{get_auth_http, load_workspace, save_workspace, AuthOpts};
#[cfg(feature = "platform")]
use crate::commands::platform::workspace::prompt_workspace;

/// Resolved manager connection — the single way commands should interact with a manager.
pub struct ManagerContext {
    /// Base URL of the manager (e.g. "http://localhost:8090").
    pub manager_url: String,
    /// Display name of the resolved manager, when provided by the platform.
    pub manager_name: Option<String>,
    /// Whether the resolved manager is Alien-hosted.
    pub manager_is_system: Option<bool>,
    /// Hosting cloud for private managers.
    pub manager_cloud: Option<String>,
    /// Authenticated manager SDK client.
    pub client: ServerSdkClient,
    /// Underlying reqwest client (carries auth headers, useful for non-SDK endpoints).
    pub http_client: reqwest::Client,
    /// Raw auth token (API key or OAuth JWT) for proxy push auth.
    /// None in dev mode (no auth required).
    pub auth_token: Option<String>,
    /// Repository name for image pushing (only available via platform discovery).
    pub repository_name: Option<String>,
    /// Repository URI for image pushing (only available via platform discovery).
    pub repository_uri: Option<String>,
    /// Workspace the caller is acting in. The manager needs it to resolve
    /// the user's identity — user OAuth tokens don't carry that themselves.
    /// `None` in single-tenant modes (dev, standalone).
    pub workspace: Option<String>,
}

#[cfg(feature = "platform")]
pub struct PlatformWorkspaceContext {
    pub name: String,
    pub query: Option<String>,
}

/// Execution mode determines which API the command targets and carries all global flags.
///
/// Core modes (always available):
/// - `Standalone` — direct manager connection via `ALIEN_MANAGER_URL`
/// - `Dev` — local dev server (auto-started)
///
/// Platform mode (behind `platform` feature):
/// - `Platform` — commands run against the platform API (api.alien.dev)
#[derive(Clone, Debug)]
pub enum ExecutionMode {
    /// Platform mode: commands run against the real platform API
    #[cfg(feature = "platform")]
    Platform {
        base_url: String,
        api_key: Option<String>,
        no_browser: bool,
        workspace: Option<String>,
        project: Option<String>,
    },
    /// Standalone mode: commands run against a standalone alien-manager instance
    Standalone { server_url: String, api_key: String },
    /// Dev mode: commands run against local dev server
    Dev { port: u16 },
}

impl ExecutionMode {
    /// Get the manager URL for this execution mode.
    pub fn manager_url(&self) -> String {
        match self {
            #[cfg(feature = "platform")]
            Self::Platform { base_url, .. } => normalize_base_url(base_url),
            Self::Standalone { server_url, .. } => normalize_base_url(server_url),
            Self::Dev { port } => format!("http://localhost:{}", port),
        }
    }

    /// Alias for manager_url (backward compatibility).
    pub fn base_url(&self) -> String {
        self.manager_url()
    }

    /// Check if this is dev mode
    pub fn is_dev(&self) -> bool {
        matches!(self, Self::Dev { .. })
    }

    /// Check if this is standalone mode
    pub fn is_standalone(&self) -> bool {
        matches!(self, Self::Standalone { .. })
    }

    /// Get the auth token (API key) for this execution mode, if any.
    pub fn auth_token(&self) -> Option<&str> {
        match self {
            #[cfg(feature = "platform")]
            Self::Platform { api_key, .. } => api_key.as_deref(),
            Self::Standalone { api_key, .. } => Some(api_key.as_str()),
            Self::Dev { .. } => None,
        }
    }

    /// Ensure the target is ready (starts dev server if needed)
    pub async fn ensure_ready(&self) -> Result<()> {
        match self {
            Self::Dev { port } => {
                if !check_server_health(*port).await {
                    return Err(AlienError::new(ErrorData::ConfigurationError {
                        message: format!(
                            "Local dev server is not running on http://localhost:{port}. Start it with `alien dev` (full session) or `alien dev server`."
                        ),
                    }));
                }
                Ok(())
            }
            Self::Standalone { .. } => Ok(()),
            #[cfg(feature = "platform")]
            Self::Platform { .. } => Ok(()),
        }
    }

    /// Get an authenticated manager SDK client.
    ///
    /// This is the primary way commands should interact with the manager.
    pub fn server_sdk_client(&self) -> Result<ServerSdkClient> {
        match self {
            Self::Standalone {
                server_url,
                api_key,
            } => {
                let mut headers = reqwest::header::HeaderMap::new();
                let header_value =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                            message: "Invalid API key format".to_string(),
                        })?;
                headers.insert(reqwest::header::AUTHORIZATION, header_value);

                let http_client = reqwest::Client::builder()
                    .default_headers(headers)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to build HTTP client".to_string(),
                    })?;

                let manager_url = normalize_base_url(server_url);
                Ok(ServerSdkClient::new_with_client(&manager_url, http_client))
            }
            Self::Dev { port } => Ok(ServerSdkClient::new(&format!("http://localhost:{}", port))),
            #[cfg(feature = "platform")]
            Self::Platform { .. } => Err(AlienError::new(ErrorData::ConfigurationError {
                message: "server_sdk_client() is not available in platform mode. Use sdk_client() instead.".to_string(),
            })),
        }
    }

    /// Get a platform SDK client (works in all modes).
    ///
    /// - Standalone: creates client with auth header pointing at manager URL
    /// - Dev: creates unauthenticated client pointing at localhost
    /// - Platform: uses OAuth auth flow
    pub async fn sdk_client(&self) -> Result<SdkClient> {
        match self {
            Self::Standalone {
                server_url,
                api_key,
            } => {
                let mut headers = reqwest::header::HeaderMap::new();
                let header_value =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                            message: "Invalid API key format".to_string(),
                        })?;
                headers.insert(reqwest::header::AUTHORIZATION, header_value);

                let http_client = reqwest::Client::builder()
                    .default_headers(headers)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to build HTTP client".to_string(),
                    })?;

                let manager_url = normalize_base_url(server_url);
                Ok(SdkClient::new_with_client(&manager_url, http_client))
            }
            Self::Dev { port } => Ok(SdkClient::new(&format!("http://localhost:{}", port))),
            #[cfg(feature = "platform")]
            Self::Platform { .. } => {
                let auth = self.auth_http().await?;
                Ok(auth.sdk_client().clone())
            }
        }
    }

    /// Resolve workspace from flags/profile and optionally prompt in a real TTY.
    pub async fn resolve_workspace_with_bootstrap(&self, allow_prompt: bool) -> Result<String> {
        match self {
            Self::Dev { .. } => Ok("local-dev".to_string()),
            Self::Standalone { .. } => Ok("default".to_string()),
            #[cfg(feature = "platform")]
            Self::Platform { workspace, .. } => {
                if let Some(workspace) = workspace.clone().or_else(load_workspace) {
                    return Ok(workspace);
                }

                if !allow_prompt {
                    return Err(AlienError::new(ErrorData::ConfigurationError {
                        message:
                            "No workspace is configured. Pass `--workspace <name>`, run `alien login --workspace <name>`, or run `alien workspaces set <name>`."
                                .to_string(),
                    }));
                }

                let http = self.auth_http().await?;
                let workspace = prompt_workspace(&http, false).await?;
                save_workspace(&workspace)?;
                println!("Default workspace set to: {workspace}");
                Ok(workspace)
            }
        }
    }

    pub async fn resolve_workspace(&self) -> Result<String> {
        self.resolve_workspace_with_bootstrap(true).await
    }

    /// Resolve the optional workspace query/header for platform API requests.
    ///
    /// User credentials need an explicit workspace because roles are per-workspace.
    /// API keys are already scoped, so a saved OAuth default workspace must not be
    /// inherited; only an explicit `--workspace` should be forwarded for mismatch
    /// checking.
    #[cfg(feature = "platform")]
    pub async fn resolve_workspace_query_with_bootstrap(
        &self,
        allow_prompt: bool,
    ) -> Result<Option<String>> {
        match self {
            Self::Platform {
                api_key: Some(_),
                workspace,
                ..
            } => Ok(workspace.clone()),
            Self::Platform { .. } => self
                .resolve_workspace_with_bootstrap(allow_prompt)
                .await
                .map(Some),
            Self::Dev { .. } => Ok(Some("local-dev".to_string())),
            Self::Standalone { .. } => Ok(Some("default".to_string())),
        }
    }

    #[cfg(feature = "platform")]
    pub async fn resolve_platform_workspace_context(
        &self,
        allow_prompt: bool,
    ) -> Result<PlatformWorkspaceContext> {
        match self {
            Self::Platform {
                api_key: Some(_),
                workspace: Some(workspace),
                ..
            } => Ok(PlatformWorkspaceContext {
                name: workspace.clone(),
                query: Some(workspace.clone()),
            }),
            Self::Platform {
                api_key: Some(_), ..
            } => {
                let http = self.auth_http().await?;
                let subject = http
                    .sdk_client()
                    .whoami()
                    .send()
                    .await
                    .into_sdk_error()
                    .context(ErrorData::ApiRequestFailed {
                        message: "Failed to resolve API key workspace".to_string(),
                        url: None,
                    })?
                    .into_inner();

                let workspace_name = match subject {
                    Subject::ServiceAccountSubject(subject) => subject.workspace_name,
                    Subject::UserSubject(_) => None,
                }
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: "API key subject is missing workspace name".to_string(),
                    })
                })?;

                Ok(PlatformWorkspaceContext {
                    name: workspace_name,
                    query: None,
                })
            }
            Self::Platform { .. } => {
                let workspace = self.resolve_workspace_with_bootstrap(allow_prompt).await?;
                Ok(PlatformWorkspaceContext {
                    name: workspace.clone(),
                    query: Some(workspace),
                })
            }
            Self::Dev { .. } => Ok(PlatformWorkspaceContext {
                name: "local-dev".to_string(),
                query: Some("local-dev".to_string()),
            }),
            Self::Standalone { .. } => Ok(PlatformWorkspaceContext {
                name: "default".to_string(),
                query: Some("default".to_string()),
            }),
        }
    }

    /// Workspace from the `--workspace` flag or the saved profile, if any.
    /// `None` in single-tenant modes (dev, standalone) — they don't need one.
    pub fn configured_workspace(&self) -> Option<String> {
        match self {
            #[cfg(feature = "platform")]
            Self::Platform { workspace, .. } => workspace.clone().or_else(load_workspace),
            _ => None,
        }
    }

    /// Get global project override (if any).
    pub fn project_override(&self) -> Option<&str> {
        match self {
            Self::Dev { .. } => Some("local-dev"),
            Self::Standalone { .. } => Some("default"),
            #[cfg(feature = "platform")]
            Self::Platform { project, .. } => project.as_deref(),
        }
    }

    /// Whether commands should avoid opening a browser.
    pub fn no_browser(&self) -> bool {
        match self {
            #[cfg(feature = "platform")]
            Self::Platform { no_browser, .. } => *no_browser,
            Self::Standalone { .. } | Self::Dev { .. } => true,
        }
    }

    /// Get workspace and project for this mode (non-interactive, fails if not set).
    pub fn get_workspace_project(&self) -> Result<(String, Option<String>)> {
        match self {
            Self::Dev { .. } => Ok(("local-dev".to_string(), Some("local-dev".to_string()))),
            Self::Standalone { .. } => Ok(("default".to_string(), Some("default".to_string()))),
            #[cfg(feature = "platform")]
            Self::Platform {
                workspace, project, ..
            } => {
                let ws = workspace.clone().or_else(load_workspace).ok_or_else(|| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: "No workspace set. Run 'alien login' or use --workspace"
                            .to_string(),
                    })
                })?;
                Ok((ws, project.clone()))
            }
        }
    }

    /// Get an authenticated HTTP client (works in all modes).
    ///
    /// - Standalone: creates client with Bearer token
    /// - Dev: creates unauthenticated client
    /// - Platform: uses OAuth flow (requires `platform` feature)
    pub async fn auth_http(&self) -> Result<crate::auth::AuthHttp> {
        match self {
            Self::Standalone {
                server_url,
                api_key,
            } => {
                let auth_value = format!("Bearer {}", api_key);
                let client = crate::auth::client_with_header(&auth_value)?;
                Ok(crate::auth::build_auth_http(
                    client,
                    server_url.clone(),
                    Some(api_key.clone()),
                ))
            }
            Self::Dev { port } => {
                let base_url = format!("http://localhost:{}", port);
                Ok(crate::auth::AuthHttp::new_unauthenticated(base_url))
            }
            #[cfg(feature = "platform")]
            Self::Platform { .. } => get_auth_http(&self.auth_opts()).await,
        }
    }

    /// Like [`resolve_manager`] but skips the artifact-registry repo
    /// provisioning step. Use this for read-only / traffic-only flows
    /// (`debug`, `commands`, `deployments list/get`) where the CLI never
    /// pushes container images and the repo isn't needed.
    ///
    /// Saves ~10–15s per invocation on dev clusters where the artifact repo
    /// provisioning call is slow (network round trip + cloud API call).
    pub async fn resolve_manager_metadata_only(
        &self,
        project: &str,
        platform: &str,
    ) -> Result<ManagerContext> {
        self.resolve_manager_inner(project, platform, false).await
    }

    /// Resolve manager URL and return an authenticated manager SDK client.
    ///
    /// - Standalone: uses the known server_url
    /// - Dev: uses localhost:{port}
    /// - Platform: calls /v1/resolve to discover the project's manager URL,
    ///   then calls the manager directly to create/get the artifact registry repo.
    pub async fn resolve_manager(&self, project: &str, platform: &str) -> Result<ManagerContext> {
        self.resolve_manager_inner(project, platform, true).await
    }

    async fn resolve_manager_inner(
        &self,
        project: &str,
        platform: &str,
        with_artifact_repo: bool,
    ) -> Result<ManagerContext> {
        match self {
            Self::Standalone {
                server_url,
                api_key,
            } => {
                let mut headers = reqwest::header::HeaderMap::new();
                let header_value =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                            message: "Invalid API key format".to_string(),
                        })?;
                headers.insert(reqwest::header::AUTHORIZATION, header_value);

                let http_client = reqwest::Client::builder()
                    .default_headers(headers)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to build HTTP client".to_string(),
                    })?;

                let manager_url = normalize_base_url(server_url);
                let client = ServerSdkClient::new_with_client(&manager_url, http_client.clone());

                Ok(ManagerContext {
                    manager_url: manager_url.clone(),
                    manager_name: None,
                    manager_is_system: None,
                    manager_cloud: None,
                    client,
                    http_client,
                    auth_token: Some(api_key.clone()),
                    repository_name: None, // Resolved at push time via manager's /v1/build-config
                    repository_uri: Some(manager_url),
                    workspace: None,
                })
            }
            Self::Dev { port } => {
                let manager_url = format!("http://localhost:{}", port);
                let http_client = reqwest::Client::new();
                let client = ServerSdkClient::new_with_client(&manager_url, http_client.clone());

                Ok(ManagerContext {
                    manager_url: manager_url.clone(),
                    manager_name: Some("local manager".to_string()),
                    manager_is_system: None,
                    manager_cloud: None,
                    client,
                    http_client,
                    auth_token: None,
                    repository_name: Some("artifacts/default".to_string()),
                    repository_uri: Some(manager_url),
                    workspace: None,
                })
            }
            #[cfg(feature = "platform")]
            Self::Platform { .. } => {
                let http = self.auth_http().await?;
                let workspace = self.resolve_platform_workspace_context(true).await?;

                // Call GET /v1/resolve?platform=X&project=Y[&workspace=Z].
                // API keys are already workspace-scoped, so workspace is omitted
                // unless the caller explicitly supplied one.
                let mut query = vec![
                    format!("platform={}", urlencoding::encode(platform)),
                    format!("project={}", urlencoding::encode(project)),
                ];
                if let Some(workspace) = &workspace.query {
                    query.push(format!("workspace={}", urlencoding::encode(workspace)));
                }
                let resolve_url = format!("{}/v1/resolve?{}", http.base_url, query.join("&"));

                // Retry logic: manager might be starting up (503)
                let max_duration = std::time::Duration::from_secs(60);
                let start_time = std::time::Instant::now();
                let mut attempt: u32 = 0;

                let resolved: ResolveResponse = loop {
                    attempt += 1;

                    let resp = http
                        .client
                        .get(&resolve_url)
                        .send()
                        .await
                        .into_alien_error()
                        .context(ErrorData::ApiRequestFailed {
                            message: "Failed to call /v1/resolve endpoint".to_string(),
                            url: Some(resolve_url.clone()),
                        })?;

                    let status = resp.status();

                    if status.is_success() {
                        break resp
                            .json::<ResolveResponse>()
                            .await
                            .into_alien_error()
                            .context(ErrorData::ApiRequestFailed {
                                message: "Failed to parse /v1/resolve response".to_string(),
                                url: Some(resolve_url.clone()),
                            })?;
                    }

                    let is_retryable = status.as_u16() == 503;

                    if is_retryable && start_time.elapsed() < max_duration {
                        let backoff_secs = std::cmp::min(2u64.pow(attempt.saturating_sub(1)), 15);
                        if attempt == 1 {
                            info!("Manager not ready yet, waiting for startup...");
                        } else {
                            info!("Still waiting for manager (attempt {})...", attempt);
                        }
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        continue;
                    }

                    let body = resp.text().await.unwrap_or_default();

                    if is_retryable {
                        return Err(AlienError::new(ErrorData::ApiRequestFailed {
                            message: format!(
                                "Manager for {} platform did not become ready within {} seconds",
                                platform,
                                max_duration.as_secs()
                            ),
                            url: Some(resolve_url),
                        }));
                    } else {
                        return Err(AlienError::new(ErrorData::ApiRequestFailed {
                            message: format!(
                                "Failed to resolve manager for {} platform (HTTP {}): {}",
                                platform, status, body
                            ),
                            url: Some(resolve_url),
                        }));
                    }
                };

                if attempt > 1 {
                    info!("Manager ready after {} attempt(s)", attempt);
                }

                let manager_url = normalize_base_url(&resolved.manager_url);

                info!("Manager: {}", manager_url);

                // Manager-bound client; workspace lives in default headers.
                let auth_token = http.bearer_token.clone();
                let manager_http_client = match &auth_token {
                    Some(token) => match &workspace.query {
                        Some(workspace) => crate::auth::client_with_auth_and_workspace(
                            &format!("Bearer {}", token),
                            workspace,
                        )?,
                        None => crate::auth::client_with_header(&format!("Bearer {}", token))?,
                    },
                    None => http.client.clone(),
                };

                // Now call the manager directly to create/get the artifact registry repo.
                // The manager owns backend selection: cloud targets use target providers,
                // while pull platforms like Kubernetes/local fall back to the manager's
                // primary registry provider.
                //
                // Skip this for read-only / traffic-only flows where the CLI
                // never pushes images (`debug`, `commands`, `deployments`).
                // Cloud-side repo provisioning can take 10s+ on dev clusters
                // and isn't worth the wait when we don't need the repo.
                let repo_name = if with_artifact_repo {
                    let name = create_or_get_artifact_repo(
                        &manager_http_client,
                        &manager_url,
                        &resolved.project_id,
                        platform,
                    )
                    .await?;
                    info!("Repository: {}", name);
                    Some(name)
                } else {
                    None
                };

                let manager_client =
                    ServerSdkClient::new_with_client(&manager_url, manager_http_client.clone());

                Ok(ManagerContext {
                    manager_url,
                    manager_name: resolved.manager_name,
                    manager_is_system: resolved.manager_is_system,
                    manager_cloud: resolved.manager_cloud,
                    client: manager_client,
                    http_client: manager_http_client,
                    auth_token,
                    repository_name: repo_name,
                    repository_uri: None,
                    workspace: workspace.query,
                })
            }
        }
    }

    /// Resolve project: --project flag > project link > optional interactive bootstrap.
    ///
    /// In Standalone/Dev modes, returns "default"/"local-dev".
    /// In Platform mode, resolves from flag/profile/interactive prompt and returns the project ID.
    pub async fn resolve_project(
        &self,
        project_override: Option<&str>,
        allow_prompt: bool,
    ) -> Result<(String, crate::project_link::ProjectLink)> {
        match self {
            Self::Dev { .. } => {
                let link = crate::project_link::ProjectLink::new(
                    "local-dev".to_string(),
                    "local-dev".to_string(),
                    "local-dev".to_string(),
                );
                Ok(("local-dev".to_string(), link))
            }
            Self::Standalone { .. } => {
                let link = crate::project_link::ProjectLink::new(
                    "default".to_string(),
                    "default".to_string(),
                    "default".to_string(),
                );
                Ok(("default".to_string(), link))
            }
            #[cfg(feature = "platform")]
            Self::Platform { .. } => {
                let http = self.auth_http().await?;
                let workspace = self
                    .resolve_platform_workspace_context(allow_prompt)
                    .await?;
                let effective_project = project_override.or(self.project_override());

                let link = if let Some(project_name) = effective_project {
                    crate::project_link::get_project_by_name(
                        &http,
                        &workspace.name,
                        workspace.query.as_deref(),
                        project_name,
                    )
                    .await?
                } else {
                    let Some(workspace_query) = workspace.query.as_deref() else {
                        return Err(AlienError::new(ErrorData::ConfigurationError {
                            message:
                                "API key mode requires `--project <name>` when no `--workspace` is supplied."
                                    .to_string(),
                        }));
                    };
                    let current_dir = crate::get_current_dir()?;
                    crate::project_link::ensure_project_linked(
                        &current_dir,
                        &http,
                        workspace_query,
                        allow_prompt,
                    )
                    .await?
                };

                let project_id = link.project_id.clone();
                Ok((project_id, link))
            }
        }
    }

    // --- Platform-only methods ---

    /// Get raw auth options for commands that run their own auth flow (e.g. login)
    #[cfg(feature = "platform")]
    pub fn auth_opts(&self) -> AuthOpts {
        match self {
            Self::Platform {
                base_url,
                api_key,
                no_browser,
                ..
            } => AuthOpts {
                api_key: api_key.clone(),
                base_url: Some(base_url.clone()),
                no_browser: *no_browser,
            },
            Self::Standalone {
                server_url,
                api_key,
            } => AuthOpts {
                api_key: Some(api_key.clone()),
                base_url: Some(server_url.clone()),
                no_browser: true,
            },
            Self::Dev { port } => AuthOpts {
                api_key: None,
                base_url: Some(format!("http://localhost:{}", port)),
                no_browser: true,
            },
        }
    }
}

/// Check if dev server is healthy
async fn check_server_health(port: u16) -> bool {
    let client = reqwest::Client::new();
    let url = format!("http://localhost:{}/health", port);
    client.get(&url).send().await.is_ok()
}

/// Response from GET /v1/resolve
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveResponse {
    manager_url: String,
    #[serde(default)]
    manager_name: Option<String>,
    #[serde(default)]
    manager_is_system: Option<bool>,
    #[serde(default)]
    manager_cloud: Option<String>,
    project_id: String,
}

fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    if trimmed.is_empty() {
        url.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Create or get an artifact registry repository on the manager.
///
/// GET first (repo may exist), POST to create on 404. The `client` is
/// expected to carry the caller's workspace in its default headers (see
/// [`crate::auth::client_with_auth_and_workspace`]); no per-call header
/// plumbing happens here.
#[cfg(feature = "platform")]
async fn create_or_get_artifact_repo(
    client: &reqwest::Client,
    manager_url: &str,
    project_id: &str,
    platform: &str,
) -> crate::error::Result<String> {
    use alien_error::{Context, IntoAlienError};

    let manager_url = normalize_base_url(manager_url);

    // Step 1: Try GET for existing repo
    let get_url = format!(
        "{}/v1/artifact-registry/repositories/{}?platform={}",
        manager_url, project_id, platform
    );

    let get_resp = send_artifact_registry_request_with_retry(
        || client.get(&get_url),
        &manager_url,
        &get_url,
        "Failed to reach artifact registry on manager",
    )
    .await?;

    if get_resp.status().is_success() {
        let body: serde_json::Value =
            get_resp
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to parse artifact repository response".to_string(),
                    url: Some(get_url.clone()),
                })?;

        if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
            return Ok(name.to_string());
        }
    }

    // Step 2: Repo doesn't exist — create it
    let create_url = format!(
        "{}/v1/artifact-registry/repositories?platform={}",
        manager_url, platform
    );

    let create_resp = send_artifact_registry_request_with_retry(
        || {
            client
                .post(&create_url)
                .json(&serde_json::json!({ "name": project_id }))
        },
        &manager_url,
        &create_url,
        "Failed to create artifact repository on manager",
    )
    .await?;

    let create_status = create_resp.status();

    if create_status.is_success() {
        let body: serde_json::Value =
            create_resp
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to parse artifact repository response".to_string(),
                    url: Some(create_url.clone()),
                })?;

        if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
            return Ok(name.to_string());
        }
    }

    // Create returned non-success (409 = already exists, or other error).
    // Try GET again — the repo may have been created concurrently.
    let get_resp2 = send_artifact_registry_request_with_retry(
        || client.get(&get_url),
        &manager_url,
        &get_url,
        "Failed to get artifact repository from manager",
    )
    .await?;

    if get_resp2.status().is_success() {
        let body: serde_json::Value =
            get_resp2
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to parse artifact repository response".to_string(),
                    url: Some(get_url.clone()),
                })?;

        if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
            return Ok(name.to_string());
        }
    }

    // Both create and get failed — report the create error
    Err(AlienError::new(ErrorData::ApiRequestFailed {
        message: format!(
            "Failed to create artifact repository '{}' for platform '{}' on manager (HTTP {})",
            project_id, platform, create_status
        ),
        url: Some(create_url),
    }))
}

#[cfg(feature = "platform")]
async fn send_artifact_registry_request_with_retry<F>(
    build_request: F,
    manager_url: &str,
    url: &str,
    message: &str,
) -> crate::error::Result<reqwest::Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    for attempt in 1..=3 {
        match build_request().send().await {
            Ok(response) => return Ok(response),
            Err(error) if attempt < 3 && is_retryable_artifact_registry_error(&error) => {
                let delay = Duration::from_secs(attempt * 2);
                warn!(
                    attempt,
                    delay_secs = delay.as_secs(),
                    url,
                    error = %error,
                    "Retrying artifact registry manager request"
                );
                tokio::time::sleep(delay).await;
            }
            Err(error) => {
                return Err(error
                    .into_alien_error()
                    .context(ErrorData::ApiRequestFailed {
                        message: format!("{} at {}", message, manager_url),
                        url: Some(url.to_string()),
                    }));
            }
        }
    }

    unreachable!("artifact registry retry loop always returns")
}

#[cfg(feature = "platform")]
fn is_retryable_artifact_registry_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_url, ExecutionMode};

    #[test]
    fn normalize_base_url_removes_trailing_slashes() {
        assert_eq!(
            normalize_base_url("https://alien-manager.example.com/"),
            "https://alien-manager.example.com"
        );
        assert_eq!(
            normalize_base_url("https://alien-manager.example.com///"),
            "https://alien-manager.example.com"
        );
    }

    #[test]
    fn normalize_base_url_keeps_urls_without_trailing_slash() {
        assert_eq!(
            normalize_base_url("http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[cfg(feature = "platform")]
    #[tokio::test]
    async fn api_key_without_explicit_workspace_omits_workspace_query() {
        let mode = ExecutionMode::Platform {
            base_url: "https://api.alien.localhost".to_string(),
            api_key: Some("ax_test".to_string()),
            no_browser: true,
            workspace: None,
            project: None,
        };

        assert_eq!(
            mode.resolve_workspace_query_with_bootstrap(false)
                .await
                .unwrap(),
            None
        );
    }

    #[cfg(feature = "platform")]
    #[tokio::test]
    async fn api_key_with_explicit_workspace_forwards_workspace_query() {
        let mode = ExecutionMode::Platform {
            base_url: "https://api.alien.localhost".to_string(),
            api_key: Some("ax_test".to_string()),
            no_browser: true,
            workspace: Some("demo".to_string()),
            project: None,
        };

        assert_eq!(
            mode.resolve_workspace_query_with_bootstrap(false)
                .await
                .unwrap(),
            Some("demo".to_string())
        );
    }
}

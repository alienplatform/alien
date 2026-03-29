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

use crate::commands::start_embedded_dev_manager;
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::Client as ServerSdkClient;
use alien_platform_api::Client as SdkClient;
use tokio::time::Duration;
use tracing::info;

#[cfg(feature = "platform")]
use crate::auth::{get_auth_http, load_workspace, save_workspace, AuthOpts};
#[cfg(feature = "platform")]
use crate::commands::platform::workspace::prompt_workspace_with_tui;

/// Resolved manager connection — the single way commands should interact with a manager.
pub struct ManagerContext {
    /// Base URL of the manager (e.g. "http://localhost:8090").
    pub manager_url: String,
    /// Authenticated manager SDK client.
    pub client: ServerSdkClient,
    /// Underlying reqwest client (carries auth headers, useful for non-SDK endpoints).
    pub http_client: reqwest::Client,
    /// Repository name for image pushing (only available via platform discovery).
    pub repository_name: Option<String>,
    /// Repository URI for image pushing (only available via platform discovery).
    pub repository_uri: Option<String>,
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
            Self::Platform { base_url, .. } => base_url.clone(),
            Self::Standalone { server_url, .. } => server_url.clone(),
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

    /// Ensure the target is ready (starts dev server if needed)
    pub async fn ensure_ready(&self) -> Result<()> {
        match self {
            Self::Dev { port } => {
                if !check_server_health(*port).await {
                    info!("Starting dev server on port {}...", port);
                    start_dev_server(*port).await?;
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

                Ok(ServerSdkClient::new_with_client(server_url, http_client))
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

                Ok(SdkClient::new_with_client(server_url, http_client))
            }
            Self::Dev { port } => Ok(SdkClient::new(&format!("http://localhost:{}", port))),
            #[cfg(feature = "platform")]
            Self::Platform { .. } => {
                let auth = self.auth_http().await?;
                Ok(auth.sdk_client().clone())
            }
        }
    }

    /// Resolve workspace: flag override -> profile.json -> interactive prompt.
    ///
    /// In Standalone/Dev modes, returns a sensible default.
    /// In Platform mode, resolves from flag/profile/interactive prompt.
    pub async fn resolve_workspace(&self) -> Result<String> {
        match self {
            Self::Dev { .. } => Ok("local-dev".to_string()),
            Self::Standalone { .. } => Ok("default".to_string()),
            #[cfg(feature = "platform")]
            Self::Platform { workspace, .. } => match workspace.clone().or_else(load_workspace) {
                Some(ws) => Ok(ws),
                None => {
                    let http = self.auth_http().await?;
                    let ws = prompt_workspace_with_tui(&http).await?;
                    save_workspace(&ws)?;
                    println!("Default workspace set to: {}", ws);
                    Ok(ws)
                }
            },
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
                    &auth_value,
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

    /// Resolve manager URL and return an authenticated manager SDK client.
    ///
    /// - Standalone: uses the known server_url
    /// - Dev: uses localhost:{port}
    /// - Platform: calls build-config API to discover the project's manager URL
    pub async fn resolve_manager(&self, project: &str, platform: &str) -> Result<ManagerContext> {
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

                let client = ServerSdkClient::new_with_client(server_url, http_client.clone());

                Ok(ManagerContext {
                    manager_url: server_url.clone(),
                    client,
                    http_client,
                    repository_name: None,
                    repository_uri: None,
                })
            }
            Self::Dev { port } => {
                let manager_url = format!("http://localhost:{}", port);
                let http_client = reqwest::Client::new();
                let client = ServerSdkClient::new_with_client(&manager_url, http_client.clone());

                Ok(ManagerContext {
                    manager_url,
                    client,
                    http_client,
                    repository_name: None,
                    repository_uri: None,
                })
            }
            #[cfg(feature = "platform")]
            Self::Platform { .. } => {
                use alien_platform_api::types::{
                    GetProjectBuildConfigPlatform, GetProjectBuildConfigWorkspace,
                    ProjectIdOrNamePathParam,
                };
                use alien_platform_api::SdkResultExt;

                let http = self.auth_http().await?;
                let platform_client = http.sdk_client();
                let workspace = self.resolve_workspace().await?;

                let workspace_param = GetProjectBuildConfigWorkspace::try_from(workspace.as_str())
                    .map_err(|e| {
                        AlienError::new(ErrorData::ValidationError {
                            field: "workspace".to_string(),
                            message: format!("Invalid workspace: {}", e),
                        })
                    })?;

                let project_param = ProjectIdOrNamePathParam::try_from(project).map_err(|e| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "project".to_string(),
                        message: format!("Invalid project: {}", e),
                    })
                })?;

                let platform_param =
                    GetProjectBuildConfigPlatform::try_from(platform).map_err(|e| {
                        AlienError::new(ErrorData::ValidationError {
                            field: "platform".to_string(),
                            message: format!("Invalid platform: {}", e),
                        })
                    })?;

                // Retry logic: manager might be starting up
                let max_duration = std::time::Duration::from_secs(60);
                let start_time = std::time::Instant::now();
                let mut attempt: u32 = 0;

                let build_config = loop {
                    attempt += 1;

                    let result = platform_client
                        .get_project_build_config()
                        .id_or_name(&project_param)
                        .platform(&platform_param)
                        .workspace(&workspace_param)
                        .send()
                        .await
                        .into_sdk_error();

                    match result {
                        Ok(response) => break response.into_inner(),
                        Err(sdk_err) => {
                            let is_retryable = sdk_err.http_status_code == Some(503);

                            if is_retryable && start_time.elapsed() < max_duration {
                                let backoff_secs =
                                    std::cmp::min(2u64.pow(attempt.saturating_sub(1)), 15);
                                if attempt == 1 {
                                    info!("Manager not ready yet, waiting for startup...");
                                } else {
                                    info!("Still waiting for manager (attempt {})...", attempt);
                                }
                                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                                continue;
                            }

                            if is_retryable {
                                let mut err = AlienError::new(ErrorData::ApiRequestFailed {
                                    message: format!(
                                        "Manager for {} platform did not become ready within {} seconds",
                                        platform,
                                        max_duration.as_secs()
                                    ),
                                    url: None,
                                });
                                err.source = Some(Box::new(sdk_err));
                                return Err(err);
                            } else {
                                let mut err = AlienError::new(ErrorData::ApiRequestFailed {
                                    message: format!(
                                        "Failed to get build configuration for {} platform",
                                        platform
                                    ),
                                    url: None,
                                });
                                err.source = Some(Box::new(sdk_err));
                                return Err(err);
                            }
                        }
                    }
                };

                if attempt > 1 {
                    info!("Agent manager ready after {} attempt(s)", attempt);
                }

                info!("Manager: {}", build_config.manager_url);
                info!("Repository: {}", build_config.repository_name);

                // Create manager SDK client reusing the authenticated reqwest client
                let authenticated_client = http.client.clone();
                let manager_client = ServerSdkClient::new_with_client(
                    &build_config.manager_url,
                    authenticated_client.clone(),
                );

                Ok(ManagerContext {
                    manager_url: build_config.manager_url,
                    client: manager_client,
                    http_client: authenticated_client,
                    repository_name: Some(build_config.repository_name),
                    repository_uri: build_config.repository_uri,
                })
            }
        }
    }

    /// Resolve project: --project flag > project link > interactive prompt.
    ///
    /// In Standalone/Dev modes, returns "default"/"local-dev".
    /// In Platform mode, resolves from flag/profile/interactive prompt and returns the project ID.
    pub async fn resolve_project(
        &self,
        project_override: Option<&str>,
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
                let workspace = self.resolve_workspace().await?;
                let effective_project = project_override.or(self.project_override());

                let link = if let Some(project_name) = effective_project {
                    crate::project_link::get_project_by_name(&http, &workspace, project_name)
                        .await?
                } else {
                    let current_dir = crate::get_current_dir()?;
                    crate::project_link::ensure_project_linked(&current_dir, &http, &workspace)
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

/// Start the dev server in the background
async fn start_dev_server(port: u16) -> Result<()> {
    start_embedded_dev_manager(port).await
}

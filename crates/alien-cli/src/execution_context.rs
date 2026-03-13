//! Execution context for tri-mode CLI operation
//!
//! Determines whether commands run against the platform API, a self-hosted
//! alien-manager, or the local dev server.
//!
//! Mode resolution:
//! - `alien dev ...` → Dev mode (localhost, no auth)
//! - `ALIEN_SERVER` set → Self-hosted mode (direct alien-manager, API key required)
//! - else → Platform mode (api.alien.dev or ALIEN_BASE_URL, OAuth or API key)
//!
//! Carries all global CLI flags and provides centralized resolution methods.

use crate::auth::{get_auth_http, load_workspace, save_workspace, AuthHttp, AuthOpts};
use crate::commands::workspace::prompt_workspace_with_tui;
use crate::error::{ErrorData, Result};
use alien_platform_api::Client as SdkClient;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_server_sdk::Client as ServerSdkClient;
use tokio::time::Duration;
use tracing::info;

/// Execution mode determines which API the command targets and carries all global flags.
#[derive(Clone, Debug)]
pub enum ExecutionMode {
    /// Platform mode: commands run against the real platform API
    Platform {
        base_url: String,
        api_key: Option<String>,
        no_browser: bool,
        workspace: Option<String>,
        project: Option<String>,
    },
    /// Self-hosted mode: commands run against a self-hosted alien-manager
    SelfHosted {
        server_url: String, // from ALIEN_SERVER env var
        api_key: String,    // from ALIEN_API_KEY env var (required)
    },
    /// Dev mode: commands run against local dev server
    Dev { port: u16 },
}

impl ExecutionMode {
    /// Get the base URL for this execution mode
    pub fn base_url(&self) -> String {
        match self {
            Self::Platform { base_url, .. } => base_url.clone(),
            Self::SelfHosted { server_url, .. } => server_url.clone(),
            Self::Dev { port } => format!("http://localhost:{}", port),
        }
    }

    /// Check if this is dev mode
    pub fn is_dev(&self) -> bool {
        matches!(self, Self::Dev { .. })
    }

    /// Check if this is self-hosted mode
    pub fn is_self_hosted(&self) -> bool {
        matches!(self, Self::SelfHosted { .. })
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
            Self::SelfHosted { .. } => Ok(()),
            Self::Platform { .. } => Ok(()),
        }
    }

    /// Get raw auth options for commands that run their own auth flow (e.g. login)
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
            Self::SelfHosted {
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

    /// Get an authenticated SDK client
    pub async fn sdk_client(&self) -> Result<SdkClient> {
        match self {
            Self::Platform { .. } => {
                let auth = self.auth_http().await?;
                Ok(auth.sdk_client().clone())
            }
            Self::SelfHosted { .. } => {
                let auth = self.auth_http().await?;
                Ok(auth.sdk_client().clone())
            }
            Self::Dev { port } => Ok(SdkClient::new(&format!("http://localhost:{}", port))),
        }
    }

    /// Get an authenticated server SDK client (for self-hosted and dev modes)
    pub fn server_sdk_client(&self) -> Result<ServerSdkClient> {
        match self {
            Self::SelfHosted {
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
            Self::Platform { .. } => Err(AlienError::new(ErrorData::ConfigurationError {
                message: "server_sdk_client() is not available in platform mode".to_string(),
            })),
        }
    }

    /// Get authenticated HTTP client
    ///
    /// Platform mode: uses OAuth/API key authentication
    /// Dev mode: returns unauthenticated client (dev server requires no auth)
    pub async fn auth_http(&self) -> Result<AuthHttp> {
        match self {
            Self::Platform { .. } => get_auth_http(&self.auth_opts()).await,
            Self::SelfHosted { .. } => get_auth_http(&self.auth_opts()).await,
            Self::Dev { port } => {
                let base_url = format!("http://localhost:{}", port);
                Ok(AuthHttp::new_unauthenticated(base_url))
            }
        }
    }

    /// Resolve workspace: flag override -> profile.json -> interactive prompt
    ///
    /// This centralizes the workspace resolution pattern that was previously
    /// duplicated across agents, onboard, projects, link, and release commands.
    pub async fn resolve_workspace(&self) -> Result<String> {
        match self {
            Self::Dev { .. } => Ok("local-dev".to_string()),
            Self::SelfHosted { .. } => Ok("default".to_string()),
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

    /// Get global project override (if any)
    pub fn project_override(&self) -> Option<&str> {
        match self {
            Self::Dev { .. } => Some("local-dev"),
            Self::SelfHosted { .. } => Some("default"),
            Self::Platform { project, .. } => project.as_deref(),
        }
    }

    /// Get workspace and project for this mode (non-interactive, fails if not set)
    ///
    /// Dev mode: Returns constants ("local-dev", Some("local-dev"))
    /// Platform mode: Checks override then config file
    pub fn get_workspace_project(&self) -> Result<(String, Option<String>)> {
        match self {
            Self::Dev { .. } => Ok(("local-dev".to_string(), Some("local-dev".to_string()))),
            Self::SelfHosted { .. } => Ok(("default".to_string(), Some("default".to_string()))),
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
}

/// Check if dev server is healthy
async fn check_server_health(port: u16) -> bool {
    let client = reqwest::Client::new();
    let url = format!("http://localhost:{}/health", port);
    client.get(&url).send().await.is_ok()
}

/// Start the dev server in the background
async fn start_dev_server(port: u16) -> Result<()> {
    let current_dir =
        std::env::current_dir()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "get current directory".to_string(),
                file_path: ".".to_string(),
                reason: "Failed to get current directory".to_string(),
            })?;

    let state_dir = current_dir.join(".alien");
    std::fs::create_dir_all(&state_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: state_dir.display().to_string(),
            reason: "Failed to create .alien directory".to_string(),
        })?;

    let db_path = state_dir.join("dev-server.db");

    let config = alien_manager::ManagerConfig {
        port,
        db_path,
        state_dir: state_dir.clone(),
        dev_mode: true,
        ..Default::default()
    };

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    // Spawn server in background
    tokio::spawn(async move {
        match alien_manager::AlienManager::builder(config).build().await {
            Ok(server) => {
                if let Err(e) = server.start(addr).await {
                    tracing::error!("Dev server error: {:?}", e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to start dev server: {:?}", e);
            }
        }
    });

    // Wait for server to be ready
    for _ in 0..50 {
        if check_server_health(port).await {
            info!("Dev server ready");
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(AlienError::new(ErrorData::ServerStartFailed {
        reason: "Timeout waiting for dev server to start".to_string(),
    }))
}

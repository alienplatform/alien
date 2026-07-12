// Authentication module: OAuth/API-key auth and workspace/profile store
//
// Core types (AuthHttp, AuthOpts, load_workspace, save_workspace) are always available.
// OAuth flow, session storage, and interactive login require the `platform` feature.

use alien_error::{Context, IntoAlienError};
use alien_platform_api::Client as SdkClient;
use dirs::config_dir;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::error::{ErrorData, Result};

// --- Core types (always available) ---

#[derive(Debug, Clone)]
pub struct AuthOpts {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub no_browser: bool,
}

#[derive(Debug, Clone)]
pub struct AuthHttp {
    pub client: Client,
    pub base_url: String,
    /// Raw bearer token (API key or OAuth access token) for reuse in proxy push.
    pub bearer_token: Option<String>,
    sdk_client: SdkClient,
}

impl AuthHttp {
    /// Create an unauthenticated client (for dev server which requires no auth)
    pub(crate) fn new_unauthenticated(base_url: String) -> Self {
        let client = Client::new();
        let sdk_client = SdkClient::new_with_client(&base_url, client.clone());
        Self {
            client,
            base_url,
            bearer_token: None,
            sdk_client,
        }
    }

    /// Get a configured SDK client for making API calls
    pub fn sdk_client(&self) -> &SdkClient {
        &self.sdk_client
    }

    /// Get the underlying reqwest client for manual API calls (when needed)
    pub fn reqwest_client(&self) -> &Client {
        &self.client
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
struct ProfileStore {
    default_workspace: Option<String>,
}

fn cfg_path() -> PathBuf {
    config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("alien")
        .join("profile.json")
}

pub fn load_workspace() -> Option<String> {
    fs::read_to_string(cfg_path())
        .ok()
        .and_then(|s| serde_json::from_str::<ProfileStore>(&s).ok())
        .and_then(|p| p.default_workspace)
        .and_then(normalize_workspace_name)
}

pub fn save_workspace(ws: &str) -> Result<()> {
    let workspace = normalize_workspace_name(ws.to_string()).ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Workspace name cannot be empty".to_string(),
        })
    })?;
    let cfg = cfg_path();
    let dir = cfg.parent().unwrap();
    fs::create_dir_all(dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: dir.display().to_string(),
            reason: "Failed to create config directory".to_string(),
        })?;
    let s = serde_json::to_string_pretty(&ProfileStore {
        default_workspace: Some(workspace),
    })
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "serialize".to_string(),
        reason: "Failed to serialize profile store".to_string(),
    })?;
    let cfg_display = cfg.display().to_string();
    fs::write(&cfg, s)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: cfg_display,
            reason: "Failed to write profile config".to_string(),
        })?;
    Ok(())
}

pub fn normalize_workspace_name(workspace: String) -> Option<String> {
    let trimmed = workspace.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn client_with_header(auth_value: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(auth_value)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));
    Ok(Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Failed to create HTTP client".to_string(),
        })?)
}

/// Build a Client that carries both the Authorization bearer and the
/// caller's workspace name on every request. User OAuth tokens don't
/// carry a workspace claim, so the manager needs this hint to know which
/// membership to resolve when forwarding to `/v1/whoami`.
pub fn client_with_auth_and_workspace(auth_value: &str, workspace: &str) -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(auth_value)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(
        "x-alien-workspace",
        HeaderValue::from_str(workspace)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason:
                    "Invalid workspace name (contains characters not allowed in an HTTP header)"
                        .to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));
    Ok(Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::NetworkError {
            message: "Failed to create HTTP client".to_string(),
        })?)
}

/// Build an AuthHttp instance with both reqwest client and SDK client
pub fn build_auth_http(client: Client, base_url: String, bearer_token: Option<String>) -> AuthHttp {
    let sdk_client = SdkClient::new_with_client(&base_url, client.clone());
    AuthHttp {
        client,
        base_url,
        bearer_token,
        sdk_client,
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_workspace_name;

    #[test]
    fn normalize_workspace_name_treats_blank_as_unset() {
        assert_eq!(normalize_workspace_name("   ".to_string()), None);
    }

    #[test]
    fn normalize_workspace_name_trims_non_blank_names() {
        assert_eq!(
            normalize_workspace_name("  demo  ".to_string()),
            Some("demo".to_string())
        );
    }
}

// --- Platform-only: OAuth flow, session storage, interactive login ---

#[cfg(feature = "platform")]
mod oauth_flow {
    use super::*;
    use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
    use axum::extract::Query;
    use axum::response::{Html, IntoResponse};
    use axum::routing::get;
    use axum::Router;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use chrono::{DateTime, Duration, Utc};
    use oauth2::basic::{BasicClient, BasicErrorResponseType};
    use oauth2::TokenResponse as OAuth2TokenResponse;
    use oauth2::{
        AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
        RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenUrl,
    };
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    const DEFAULT_BASE: &str = "https://api.alien.dev";
    const CLI_CLIENT_ID: &str = "alien-cli";

    const OAUTH_CALLBACK_PORTS: &[u16] = &[20350, 20351, 20352, 20353, 20354];

    /// Saved OAuth session, one JSON file next to `profile.json`.
    ///
    /// A file instead of the OS keychain: released CLI binaries are ad-hoc
    /// signed, and macOS ties keychain items to the creating binary's
    /// signature — after any upgrade the new binary is denied access to the
    /// saved session and every command would need a fresh browser login.
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StoredTokens {
        /// OAuth access token, sent as the Bearer credential.
        access_token: String,
        /// Refresh token used to mint a new access token after expiry.
        refresh_token: Option<String>,
    }

    struct TokenStore {
        path: PathBuf,
    }

    impl TokenStore {
        fn new() -> Result<Self> {
            // Refuse to fall back to the working directory: credentials
            // written into a project tree can end up committed.
            let config_dir = config_dir().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "No user config directory found to store the login session"
                        .to_string(),
                })
            })?;
            Ok(Self {
                path: config_dir.join("alien").join("credentials.json"),
            })
        }

        /// `Ok(None)` when no session has been saved yet.
        fn load(&self) -> Result<Option<StoredTokens>> {
            if !self.path.exists() {
                return Ok(None);
            }
            let content = fs::read_to_string(&self.path).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: self.path.display().to_string(),
                    reason: "Failed to read the saved login session".to_string(),
                },
            )?;
            let tokens = serde_json::from_str(&content).into_alien_error().context(
                ErrorData::JsonError {
                    operation: "parse".to_string(),
                    reason: format!(
                    "Saved login session at {} is not valid JSON; run `alien login` to recreate it",
                    self.path.display()
                ),
                },
            )?;
            Ok(Some(tokens))
        }

        fn save(&self, tokens: &StoredTokens) -> Result<()> {
            let dir = self.path.parent().expect("credentials path has a parent");
            fs::create_dir_all(dir)
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "create directory".to_string(),
                    file_path: dir.display().to_string(),
                    reason: "Failed to create config directory".to_string(),
                })?;
            let json = serde_json::to_string_pretty(tokens)
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "serialize".to_string(),
                    reason: "Failed to serialize login session".to_string(),
                })?;
            alien_core::file_utils::write_secret_file(&self.path, json.as_bytes())
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "write".to_string(),
                    file_path: self.path.display().to_string(),
                    reason: "Failed to write the login session".to_string(),
                })
        }

        /// Persist tokens from a login or refresh response, keeping the
        /// existing refresh token when the response doesn't carry a new one.
        fn save_response(&self, t: &TokenResponse) -> Result<()> {
            // A corrupt existing file must not block `alien login` — it gets
            // rewritten here; only the previous refresh token would be lost.
            let existing_refresh = self.load().ok().flatten().and_then(|s| s.refresh_token);
            self.save(&StoredTokens {
                access_token: t.access_token.clone(),
                refresh_token: t.refresh_token.clone().or(existing_refresh),
            })
        }

        fn clear(&self) -> Result<()> {
            match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e
                    .into_alien_error()
                    .context(ErrorData::FileOperationFailed {
                        operation: "delete".to_string(),
                        file_path: self.path.display().to_string(),
                        reason: "Failed to delete the saved login session".to_string(),
                    })),
            }
        }
    }

    /// Build an authenticated HTTP handle: API key if given, else the saved
    /// session (refreshed when expired), else interactive login.
    pub async fn get_auth_http(opts: &AuthOpts) -> Result<AuthHttp> {
        let base_url = opts
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        if let Some(api_key) = opts.api_key.clone() {
            let auth_value = format!("Bearer {}", api_key);
            let client = client_with_header(&auth_value)?;
            return Ok(build_auth_http(client, base_url, Some(api_key)));
        }

        if let Some((client, access_token)) = saved_session_client(&base_url).await? {
            return Ok(build_auth_http(client, base_url, Some(access_token)));
        }

        // No usable session. Interactive login needs a human at a real
        // terminal — headless, the browser callback would never complete, so
        // fail with the next step instead of waiting forever.
        if !crate::output::can_prompt() {
            return Err(AlienError::new(ErrorData::LoginRequired {
                reason: "no usable login session on this machine".to_string(),
            }));
        }

        let success_url = derive_dashboard_success_url(&base_url);
        let tokens = login_pkce(&base_url, opts.no_browser, success_url.as_deref()).await?;
        store_tokens(&tokens)?;
        let auth_value = format!("Bearer {}", tokens.access_token);
        let client = client_with_header(&auth_value)?;
        Ok(build_auth_http(client, base_url, Some(tokens.access_token)))
    }

    /// Force a fresh login flow (for explicit login command)
    pub async fn force_login(opts: &AuthOpts) -> Result<AuthHttp> {
        let base_url = opts
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        logout()?;

        if let Some(api_key) = opts.api_key.clone() {
            let auth_value = format!("Bearer {}", api_key);
            let client = client_with_header(&auth_value)?;
            return Ok(build_auth_http(client, base_url, Some(api_key)));
        }

        let tokens = login_with_ui(&base_url, opts.no_browser).await?;
        store_tokens(&tokens)?;
        let auth_value = format!("Bearer {}", tokens.access_token);
        let client = client_with_header(&auth_value)?;
        Ok(build_auth_http(client, base_url, Some(tokens.access_token)))
    }

    /// Explicit logout util
    pub fn logout() -> Result<()> {
        TokenStore::new()?.clear()?;
        match fs::remove_file(cfg_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "delete".to_string(),
                    file_path: cfg_path().display().to_string(),
                    reason: "Failed to delete the saved workspace profile".to_string(),
                })),
        }
    }

    /* ── internals ─────────────────────────────────────────────────────────── */

    /// Derive the dashboard success URL from the API base URL.
    fn derive_dashboard_success_url(api_base: &str) -> Option<String> {
        let url = url::Url::parse(api_base).ok()?;
        let host = url.host_str()?;
        let dashboard_host = host.strip_prefix("api.").unwrap_or(host);

        let mut dashboard_url = url.clone();
        dashboard_url.set_host(Some(dashboard_host)).ok()?;
        dashboard_url.set_path("/oauth/consent/success");

        Some(dashboard_url.to_string().trim_end_matches('/').to_string())
    }

    async fn bind_oauth_callback_port() -> Result<(u16, tokio::net::TcpListener)> {
        for &port in OAUTH_CALLBACK_PORTS {
            let addr: SocketAddr = format!("127.0.0.1:{}", port)
                .parse()
                .expect("valid socket address");

            if let Ok(listener) = tokio::net::TcpListener::bind(addr).await {
                return Ok((port, listener));
            }
        }

        Err(AlienError::new(ErrorData::NetworkError {
            message: format!(
                "All OAuth callback ports are in use. Tried ports: {}",
                OAUTH_CALLBACK_PORTS
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }))
    }

    /// Resolve a usable session from the saved tokens.
    ///
    /// `Ok(None)` means there is no usable session — nothing saved, the access
    /// token expired with no refresh token, or the server rejected the refresh
    /// token — and only a fresh login can produce one. Every other failure is
    /// an error: a new browser login wouldn't fix it, so the caller must not
    /// fall back to one.
    async fn saved_session_client(base_url: &str) -> Result<Option<(Client, String)>> {
        let store = TokenStore::new()?;
        let Some(saved) = store.load()? else {
            return Ok(None);
        };

        let access_token = if token_expired(&saved.access_token, 30) {
            let Some(refresh) = saved.refresh_token else {
                return Ok(None);
            };
            match refresh_token(base_url, &refresh).await? {
                RefreshOutcome::Refreshed(new_tokens) => {
                    let access = new_tokens.access_token.clone();
                    store.save(&StoredTokens {
                        access_token: new_tokens.access_token,
                        // The provider rotates refresh tokens; if a response
                        // ever omits one, the current token is still valid.
                        refresh_token: new_tokens.refresh_token.or(Some(refresh)),
                    })?;
                    access
                }
                RefreshOutcome::Rejected => return Ok(None),
            }
        } else {
            saved.access_token
        };

        let client = client_with_header(&format!("Bearer {}", access_token))?;
        Ok(Some((client, access_token)))
    }

    /// The provider issues JWTs with `exp`, so an unreadable token means a corrupted
    /// store that a refresh repairs. Nothing reacts to a 401 here, so erring toward a
    /// refresh is safer than sending a token we couldn't read.
    fn token_expired(jwt: &str, leeway_secs: i64) -> bool {
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return true;
        }
        if let Ok(payload) = URL_SAFE_NO_PAD.decode(parts[1]) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&payload) {
                if let Some(exp) = v.get("exp").and_then(|e| e.as_i64()) {
                    if let Some(exp_dt) = DateTime::from_timestamp(exp, 0) {
                        return Utc::now() + Duration::seconds(leeway_secs) >= exp_dt;
                    }
                }
            }
        }
        true
    }

    fn store_tokens(t: &TokenResponse) -> Result<()> {
        TokenStore::new()?.save_response(t)
    }

    /// Result of a refresh-token exchange the server answered.
    enum RefreshOutcome {
        /// New tokens were issued.
        Refreshed(TokenResponse),
        /// The server rejected the refresh token (revoked or expired) — the
        /// saved session is dead and only a fresh login can replace it.
        Rejected,
    }

    async fn refresh_token(base: &str, refresh: &str) -> Result<RefreshOutcome> {
        let auth_url = AuthUrl::new(format!("{}/auth/oauth2/authorize", base))
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid authorization URL".to_string(),
            })?;

        let token_url = TokenUrl::new(format!("{}/auth/oauth2/token", base))
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid token URL".to_string(),
            })?;

        let oauth_client = BasicClient::new(ClientId::new(CLI_CLIENT_ID.to_string()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url);

        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .into_alien_error()
            .context(ErrorData::NetworkError {
                message: "Failed to build HTTP client".to_string(),
            })?;

        let token_result = oauth_client
            .exchange_refresh_token(&RefreshToken::new(refresh.to_string()))
            .add_extra_param("resource", base)
            .request_async(&http_client)
            .await;

        match token_result {
            Ok(token) => Ok(RefreshOutcome::Refreshed(TokenResponse {
                access_token: OAuth2TokenResponse::access_token(&token).secret().clone(),
                refresh_token: OAuth2TokenResponse::refresh_token(&token)
                    .map(|t| t.secret().clone()),
                expires_in: OAuth2TokenResponse::expires_in(&token).map(|d| d.as_secs() as i64),
            })),
            // Only a definitive rejection (revoked or expired refresh token)
            // means the session is dead; transient OAuth error responses
            // (server_error, temporarily_unavailable) must not discard it.
            Err(RequestTokenError::ServerResponse(e))
                if matches!(e.error(), BasicErrorResponseType::InvalidGrant) =>
            {
                Ok(RefreshOutcome::Rejected)
            }
            Err(e) => Err(e
                .into_alien_error()
                .context(ErrorData::AuthenticationFailed {
                    reason: "Refresh token request failed".to_string(),
                })),
        }
    }

    /// CSRF check: reject the OAuth callback unless `state` is present and matches.
    /// Absence is a rejection, not a skip — this guards independently of PKCE.
    fn state_is_valid(params: &HashMap<String, String>, expected: &str) -> bool {
        params.get("state").is_some_and(|s| s == expected)
    }

    /// Escape the redirect URL for the single-quoted JS string in the inline
    /// `<script>` — a raw `'`, `\`, or `</script>` (via `<`) would break out, and
    /// a raw line terminator (`\n`, `\r`, U+2028, U+2029) is illegal in a JS string.
    fn escape_js_single_quoted(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('<', "\\x3C")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }

    async fn login_pkce(
        base: &str,
        no_browser: bool,
        success_redirect: Option<&str>,
    ) -> Result<TokenResponse> {
        let (port, listener) = bind_oauth_callback_port().await?;
        let redirect = format!("http://127.0.0.1:{}/callback", port);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let oauth_client = build_oauth_client(base, &redirect)?;

        let (auth_url, csrf_token) = oauth_client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("offline_access".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", base)
            .url();

        let expected_state = csrf_token.secret().clone();
        let (tx, rx) = oneshot::channel::<String>();
        let state = Arc::new(Mutex::new(Some(tx)));
        let success_redirect_owned = success_redirect.map(|s| s.to_string());
        let app = Router::new().route(
            "/callback",
            get({
                let state = state.clone();
                let success_redirect = success_redirect_owned.clone();
                let expected_state = expected_state.clone();
                move |Query(params): Query<HashMap<String, String>>| {
                    let state = state.clone();
                    let success_redirect = success_redirect.clone();
                    let expected_state = expected_state.clone();
                    async move {
                        if !state_is_valid(&params, &expected_state) {
                            return "Invalid state parameter".into_response();
                        }
                        if let Some(code) = params.get("code").cloned() {
                            if let Some(sender) = state.lock().unwrap().take() {
                                let _ = sender.send(code);
                            }
                            match success_redirect {
                                Some(url) => Html(format!(
                                    "<script>window.location.href = '{}';</script>",
                                    escape_js_single_quoted(&url)
                                ))
                                .into_response(),
                                None => {
                                    "Authentication successful! You may return to the terminal."
                                        .into_response()
                                }
                            }
                        } else {
                            "Invalid redirect".into_response()
                        }
                    }
                }
            }),
        );
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let auth_url_str = auth_url.to_string();
        if !no_browser {
            let _ = open::that(&auth_url_str);
        } else {
            eprintln!("Open this URL to authenticate:\n{}", auth_url_str);
        }

        let code = rx
            .await
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "No auth code received".to_string(),
            })?;
        let token = exchange_code_with_pkce(base, &code, &redirect, pkce_verifier).await?;
        server_handle.abort();
        Ok(token)
    }

    async fn login_with_ui(base: &str, no_browser: bool) -> Result<TokenResponse> {
        let (port, listener) = bind_oauth_callback_port().await?;
        let redirect = format!("http://127.0.0.1:{}/callback", port);

        let success_url = derive_dashboard_success_url(base);

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let oauth_client = build_oauth_client(base, &redirect)?;

        let (auth_url, csrf_token) = oauth_client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("offline_access".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", base)
            .url();

        let expected_state = csrf_token.secret().clone();
        let auth_url_str = auth_url.to_string();

        println!("Please visit the following URL in your web browser:");
        println!("> {}\n", auth_url_str);

        let (tx, rx) = oneshot::channel::<String>();
        let state = Arc::new(Mutex::new(Some(tx)));
        let app = Router::new().route(
            "/callback",
            get({
                let state = state.clone();
                let expected_state = expected_state.clone();
                move |Query(params): Query<HashMap<String, String>>| {
                    let state = state.clone();
                    let expected_state = expected_state.clone();
                    async move {
                        if !state_is_valid(&params, &expected_state) {
                            return "Invalid state parameter".into_response();
                        }
                        if let Some(code) = params.get("code").cloned() {
                            if let Some(sender) = state.lock().unwrap().take() {
                                let _ = sender.send(code);
                            }
                            if let Some(ref url) = success_url {
                                Html(format!(
                                    "<script>window.location.href = '{}';</script>",
                                    escape_js_single_quoted(url)
                                ))
                                .into_response()
                            } else {
                                "Authentication successful! You may close this window."
                                    .into_response()
                            }
                        } else {
                            "Invalid redirect".into_response()
                        }
                    }
                }
            }),
        );
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        if !no_browser {
            let _ = open::that(&auth_url_str);
        }

        let spinner_frames = &['⠇', '⠏', '⠋', '⠙', '⠸', '⠴', '⠦', '⠇'];
        let mut spinner_frame = 0;

        let code = tokio::select! {
            result = rx => result.into_alien_error().context(ErrorData::AuthenticationFailed {
                reason: "No auth code received".to_string(),
            })?,
            _ = async {
                loop {
                    print!("\r{} Waiting for authentication to be completed", spinner_frames[spinner_frame]);
                    use std::io::{self, Write};
                    io::stdout().flush().ok();
                    spinner_frame = (spinner_frame + 1) % spinner_frames.len();
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            } => unreachable!()
        };

        print!("\r                                                     \r");

        let token = exchange_code_with_pkce(base, &code, &redirect, pkce_verifier).await?;
        server_handle.abort();
        Ok(token)
    }

    fn build_oauth_client(
        base: &str,
        redirect: &str,
    ) -> Result<
        oauth2::Client<
            oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
            oauth2::StandardTokenResponse<
                oauth2::EmptyExtraTokenFields,
                oauth2::basic::BasicTokenType,
            >,
            oauth2::StandardTokenIntrospectionResponse<
                oauth2::EmptyExtraTokenFields,
                oauth2::basic::BasicTokenType,
            >,
            oauth2::StandardRevocableToken,
            oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
            oauth2::EndpointSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointNotSet,
            oauth2::EndpointSet,
        >,
    > {
        let auth_url = AuthUrl::new(format!("{}/auth/oauth2/authorize", base))
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid authorization URL".to_string(),
            })?;

        let token_url = TokenUrl::new(format!("{}/auth/oauth2/token", base))
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid token URL".to_string(),
            })?;

        let redirect_url = RedirectUrl::new(redirect.to_string())
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Invalid redirect URL".to_string(),
            })?;

        let client = BasicClient::new(ClientId::new(CLI_CLIENT_ID.to_string()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect_url);

        Ok(client)
    }

    async fn exchange_code_with_pkce(
        base: &str,
        code: &str,
        redirect: &str,
        pkce_verifier: PkceCodeVerifier,
    ) -> Result<TokenResponse> {
        let oauth_client = build_oauth_client(base, redirect)?;

        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .into_alien_error()
            .context(ErrorData::NetworkError {
                message: "Failed to build HTTP client".to_string(),
            })?;

        let token_result = oauth_client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pkce_verifier)
            .add_extra_param("resource", base)
            .request_async(&http_client)
            .await
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Code exchange failed".to_string(),
            })?;

        Ok(TokenResponse {
            access_token: OAuth2TokenResponse::access_token(&token_result)
                .secret()
                .clone(),
            refresh_token: OAuth2TokenResponse::refresh_token(&token_result)
                .map(|t| t.secret().clone()),
            expires_in: OAuth2TokenResponse::expires_in(&token_result).map(|d| d.as_secs() as i64),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn temp_store() -> (tempfile::TempDir, TokenStore) {
            let dir = tempfile::tempdir().expect("create temp dir");
            let store = TokenStore {
                path: dir.path().join("credentials.json"),
            };
            (dir, store)
        }

        fn jwt_with_exp(exp: i64) -> String {
            let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
            let payload = URL_SAFE_NO_PAD.encode(serde_json::json!({ "exp": exp }).to_string());
            format!("{}.{}.signature", header, payload)
        }

        #[test]
        fn store_round_trip_preserves_tokens_and_restricts_permissions() {
            let (_dir, store) = temp_store();

            store
                .save(&StoredTokens {
                    access_token: "access-1".to_string(),
                    refresh_token: Some("refresh-1".to_string()),
                })
                .expect("save should succeed");

            let loaded = store
                .load()
                .expect("load should succeed")
                .expect("session should exist");
            assert_eq!(loaded.access_token, "access-1");
            assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-1"));

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&store.path)
                    .expect("stat credentials file")
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600, "credentials file must be 0600");
            }
        }

        #[test]
        fn load_returns_none_when_nothing_saved() {
            let (_dir, store) = temp_store();
            assert!(store.load().expect("load should succeed").is_none());
        }

        #[test]
        fn load_fails_on_corrupt_file() {
            let (_dir, store) = temp_store();
            fs::write(&store.path, "not-json{{{").expect("write corrupt file");

            let error = store.load().expect_err("corrupt file should be an error");
            assert!(
                format!("{:?}", error).contains("alien login"),
                "error should tell the user how to recover: {:?}",
                error
            );
        }

        #[test]
        fn save_response_keeps_existing_refresh_token_when_response_omits_it() {
            let (_dir, store) = temp_store();
            store
                .save(&StoredTokens {
                    access_token: "access-1".to_string(),
                    refresh_token: Some("refresh-1".to_string()),
                })
                .expect("seed session");

            store
                .save_response(&TokenResponse {
                    access_token: "access-2".to_string(),
                    refresh_token: None,
                    expires_in: Some(3600),
                })
                .expect("save_response should succeed");

            let loaded = store
                .load()
                .expect("load should succeed")
                .expect("session should exist");
            assert_eq!(loaded.access_token, "access-2");
            assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-1"));
        }

        #[test]
        fn save_response_takes_rotated_refresh_token() {
            let (_dir, store) = temp_store();
            store
                .save(&StoredTokens {
                    access_token: "access-1".to_string(),
                    refresh_token: Some("refresh-1".to_string()),
                })
                .expect("seed session");

            store
                .save_response(&TokenResponse {
                    access_token: "access-2".to_string(),
                    refresh_token: Some("refresh-2".to_string()),
                    expires_in: Some(3600),
                })
                .expect("save_response should succeed");

            let loaded = store
                .load()
                .expect("load should succeed")
                .expect("session should exist");
            assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-2"));
        }

        #[test]
        fn save_response_recovers_from_corrupt_file() {
            let (_dir, store) = temp_store();
            fs::write(&store.path, "not-json{{{").expect("write corrupt file");

            store
                .save_response(&TokenResponse {
                    access_token: "access-1".to_string(),
                    refresh_token: Some("refresh-1".to_string()),
                    expires_in: Some(3600),
                })
                .expect("login must overwrite a corrupt session file");

            let loaded = store
                .load()
                .expect("load should succeed")
                .expect("session should exist");
            assert_eq!(loaded.access_token, "access-1");
        }

        #[test]
        fn token_expired_for_past_and_near_expiry() {
            let now = Utc::now().timestamp();
            assert!(token_expired(&jwt_with_exp(now - 100), 30));
            // Within the leeway window counts as expired.
            assert!(token_expired(&jwt_with_exp(now + 10), 30));
        }

        #[test]
        fn token_not_expired_with_future_expiry() {
            let now = Utc::now().timestamp();
            assert!(!token_expired(&jwt_with_exp(now + 3600), 30));
        }

        #[test]
        fn token_expired_for_malformed_tokens() {
            assert!(token_expired("opaque-token", 30));
            assert!(token_expired("a.b", 30));
            let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
            let payload_without_exp = URL_SAFE_NO_PAD.encode(br#"{"sub":"user"}"#);
            assert!(token_expired(
                &format!("{}.{}.sig", header, payload_without_exp),
                30
            ));
        }

        #[test]
        fn callback_state_must_be_present_and_match() {
            let expected = "expected-csrf-state";

            // A callback carrying no `state` is the CSRF attack case — it must
            // not pass, even though `code` alone looks like a valid response.
            assert!(!state_is_valid(&HashMap::new(), expected));
            let mut code_only = HashMap::new();
            code_only.insert("code".to_string(), "the-auth-code".to_string());
            assert!(!state_is_valid(&code_only, expected));

            let mut mismatched = HashMap::new();
            mismatched.insert("state".to_string(), "attacker-value".to_string());
            assert!(!state_is_valid(&mismatched, expected));

            let mut matching = HashMap::new();
            matching.insert("state".to_string(), expected.to_string());
            assert!(state_is_valid(&matching, expected));
        }

        #[test]
        fn redirect_url_is_escaped_for_the_inline_script() {
            // A plain URL is passed through untouched.
            assert_eq!(
                escape_js_single_quoted("https://app.example.com/done"),
                "https://app.example.com/done"
            );
            // A crafted value can't close the JS string literal or the <script> element.
            let escaped = escape_js_single_quoted("x'</script>");
            assert_eq!(escaped, "x\\'\\x3C/script>");
            assert!(!escaped.contains('<'));

            // Line terminators are escaped — a raw one is illegal in a JS string.
            assert_eq!(escape_js_single_quoted("a\u{2028}b"), "a\\u2028b");
        }
    }
}

// Re-export platform-only functions
#[cfg(feature = "platform")]
pub use oauth_flow::{force_login, get_auth_http, logout};

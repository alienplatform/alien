// Authentication module: OAuth/API-key auth and workspace/profile store
//
// Core types (AuthHttp, AuthOpts, load_workspace, save_workspace) are always available.
// OAuth flow, keyring storage, and interactive login require the `platform` feature.

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
}

pub fn save_workspace(ws: &str) -> Result<()> {
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
        default_workspace: Some(ws.to_string()),
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

// --- Platform-only: OAuth flow, keyring, interactive login ---

#[cfg(feature = "platform")]
mod oauth_flow {
    use super::*;
    use alien_error::{AlienError, Context, IntoAlienError};
    use axum::extract::Query;
    use axum::response::{Html, IntoResponse};
    use axum::routing::get;
    use axum::Router;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use chrono::{DateTime, Duration, Utc};
    use oauth2::basic::BasicClient;
    use oauth2::TokenResponse as OAuth2TokenResponse;
    use oauth2::{
        AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
        RedirectUrl, RefreshToken, Scope, TokenUrl,
    };
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex, OnceLock};
    use tokio::sync::oneshot;

    #[cfg(debug_assertions)]
    use debug_keyring::Entry;
    #[cfg(not(debug_assertions))]
    use keyring::Entry;

    const SERVICE: &str = "alien-cli";
    const ACCESS_USER: &str = "access_token";
    const REFRESH_USER: &str = "refresh_token";
    const DEFAULT_BASE: &str = "https://api.alien.dev";
    const CLI_CLIENT_ID: &str = "alien-cli";

    const OAUTH_CALLBACK_PORTS: &[u16] = &[20350, 20351, 20352, 20353, 20354];

    /// In-memory cache for tokens to reduce keyring access
    #[derive(Debug, Clone)]
    struct TokenCache {
        access_token: Option<String>,
        refresh_token: Option<String>,
        last_updated: DateTime<Utc>,
    }

    impl TokenCache {
        fn new() -> Self {
            Self {
                access_token: None,
                refresh_token: None,
                last_updated: Utc::now(),
            }
        }

        fn is_stale(&self) -> bool {
            Utc::now().signed_duration_since(self.last_updated) > Duration::minutes(5)
        }

        fn update_tokens(&mut self, access: Option<String>, refresh: Option<String>) {
            self.access_token = access;
            self.refresh_token = refresh;
            self.last_updated = Utc::now();
        }

        fn clear(&mut self) {
            self.access_token = None;
            self.refresh_token = None;
            self.last_updated = Utc::now();
        }
    }

    static TOKEN_CACHE: OnceLock<Mutex<TokenCache>> = OnceLock::new();

    fn get_cache() -> &'static Mutex<TokenCache> {
        TOKEN_CACHE.get_or_init(|| Mutex::new(TokenCache::new()))
    }

    fn with_cache<T>(f: impl FnOnce(&mut TokenCache) -> T) -> T {
        let cache = get_cache();
        let mut guard = cache.lock().unwrap();
        f(&mut guard)
    }

    /// Build an authenticated HTTP handle (uses API key if present; else OAuth tokens)
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

        match try_bearer_client(&base_url).await {
            Ok(client) => {
                let token = extract_bearer_token(&client)?;
                Ok(build_auth_http(client, base_url, Some(token)))
            }
            Err(_) => {
                let success_url = derive_dashboard_success_url(&base_url);
                let tokens = login_pkce(&base_url, opts.no_browser, success_url.as_deref()).await?;
                store_tokens(&tokens)?;
                let auth_value = format!("Bearer {}", tokens.access_token);
                let client = client_with_header(&auth_value)?;
                Ok(build_auth_http(client, base_url, Some(tokens.access_token)))
            }
        }
    }

    /// Force a fresh login flow (for explicit login command)
    pub async fn force_login(opts: &AuthOpts) -> Result<AuthHttp> {
        let base_url = opts
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        logout();

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
    pub fn logout() {
        let _ = Entry::new(SERVICE, ACCESS_USER).and_then(|e| e.delete_password());
        let _ = Entry::new(SERVICE, REFRESH_USER).and_then(|e| e.delete_password());
        let _ = std::fs::remove_file(cfg_path());
        with_cache(|cache| cache.clear());
    }

    /* ── internals ─────────────────────────────────────────────────────────── */

    fn derive_dashboard_success_url(api_base: &str) -> Option<String> {
        let url = match url::Url::parse(api_base) {
            Ok(u) => u,
            Err(_) => return None,
        };

        let host = url.host_str()?;
        let scheme = url.scheme();

        let dashboard_base = if host == "localhost" || host == "127.0.0.1" {
            format!("{}://localhost:3000", scheme)
        } else {
            let dashboard_host = host.strip_prefix("api.").unwrap_or(host);
            format!("{}://{}", scheme, dashboard_host)
        };

        Some(format!("{}/oauth/consent/success", dashboard_base))
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

    fn extract_bearer_token(_client: &Client) -> Result<String> {
        let cached_tokens = with_cache(|cache| cache.access_token.clone());
        cached_tokens.ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                reason: "No bearer token available".to_string(),
            })
        })
    }

    pub async fn try_bearer_client(base_url: &str) -> Result<Client> {
        let cached_tokens = with_cache(|cache| {
            if cache.is_stale() {
                cache.clear();
                None
            } else {
                cache.access_token.clone()
            }
        });

        let access_token = if let Some(token) = cached_tokens {
            if token_expired(&token, 30) {
                refresh_cached_token(base_url).await?
            } else {
                token
            }
        } else {
            load_tokens_from_keyring(base_url).await?
        };

        client_with_header(&format!("Bearer {}", access_token))
    }

    async fn load_tokens_from_keyring(base_url: &str) -> Result<String> {
        let access = Entry::new(SERVICE, ACCESS_USER)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to create keyring entry".to_string(),
            })?
            .get_password()
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to get access token from keyring".to_string(),
            })?;
        if access.trim().is_empty() {
            return Err(AlienError::new(ErrorData::AuthenticationFailed {
                reason: "No access token".to_string(),
            }));
        }

        let refresh = Entry::new(SERVICE, REFRESH_USER)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to create refresh token keyring entry".to_string(),
            })?
            .get_password()
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to get refresh token from keyring".to_string(),
            })
            .ok();

        with_cache(|cache| {
            cache.update_tokens(Some(access.clone()), refresh);
        });

        if token_expired(&access, 30) {
            refresh_cached_token(base_url).await
        } else {
            Ok(access)
        }
    }

    async fn refresh_cached_token(base_url: &str) -> Result<String> {
        let cached_refresh_token = with_cache(|cache| cache.refresh_token.clone());

        let refresh = match cached_refresh_token {
            Some(token) => token,
            None => Entry::new(SERVICE, REFRESH_USER)
                .into_alien_error()
                .context(ErrorData::AuthenticationFailed {
                    reason: "Failed to create refresh token keyring entry".to_string(),
                })?
                .get_password()
                .into_alien_error()
                .context(ErrorData::AuthenticationFailed {
                    reason: "Failed to get refresh token from keyring".to_string(),
                })?,
        };

        let new_tokens = refresh_token(base_url, &refresh).await?;
        store_tokens(&new_tokens)?;

        with_cache(|cache| {
            cache.update_tokens(
                Some(new_tokens.access_token.clone()),
                new_tokens.refresh_token.clone(),
            );
        });

        Ok(new_tokens.access_token)
    }

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

    pub fn store_tokens(t: &TokenResponse) -> Result<()> {
        Entry::new(SERVICE, ACCESS_USER)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to create access token keyring entry".to_string(),
            })?
            .set_password(&t.access_token)
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Failed to store access token".to_string(),
            })?;
        if let Some(r) = &t.refresh_token {
            Entry::new(SERVICE, REFRESH_USER)
                .into_alien_error()
                .context(ErrorData::AuthenticationFailed {
                    reason: "Failed to create refresh token keyring entry".to_string(),
                })?
                .set_password(r)
                .into_alien_error()
                .context(ErrorData::AuthenticationFailed {
                    reason: "Failed to store refresh token".to_string(),
                })?;
        }

        with_cache(|cache| {
            cache.update_tokens(Some(t.access_token.clone()), t.refresh_token.clone());
        });

        Ok(())
    }

    async fn refresh_token(base: &str, refresh: &str) -> Result<TokenResponse> {
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
            .request_async(&http_client)
            .await
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                reason: "Refresh token request failed".to_string(),
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
                        if let Some(returned_state) = params.get("state") {
                            if returned_state != &expected_state {
                                return "Invalid state parameter".into_response();
                            }
                        }
                        if let Some(code) = params.get("code").cloned() {
                            if let Some(sender) = state.lock().unwrap().take() {
                                let _ = sender.send(code);
                            }
                            match success_redirect {
                                Some(url) => Html(format!(
                                    "<script>window.location.href = '{}';</script>",
                                    url
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
                        if let Some(returned_state) = params.get("state") {
                            if returned_state != &expected_state {
                                return "Invalid state parameter".into_response();
                            }
                        }
                        if let Some(code) = params.get("code").cloned() {
                            if let Some(sender) = state.lock().unwrap().take() {
                                let _ = sender.send(code);
                            }
                            if let Some(ref url) = success_url {
                                Html(format!(
                                    "<script>window.location.href = '{}';</script>",
                                    url
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

    /// Simple file-based keyring for debug builds to avoid macOS keychain prompts
    #[cfg(debug_assertions)]
    mod debug_keyring {
        use std::collections::HashMap;
        use std::fs;
        use std::path::PathBuf;

        #[derive(Debug)]
        pub struct DebugKeyringError(String);

        impl std::fmt::Display for DebugKeyringError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::error::Error for DebugKeyringError {}

        pub struct Entry {
            service: String,
            user: String,
        }

        impl Entry {
            pub fn new(service: &str, user: &str) -> Result<Self, DebugKeyringError> {
                Ok(Self {
                    service: service.to_string(),
                    user: user.to_string(),
                })
            }

            pub fn set_password(&self, password: &str) -> Result<(), DebugKeyringError> {
                let mut store = self.load_store()?;
                let key = format!("{}:{}", self.service, self.user);
                store.insert(key, password.to_string());
                self.save_store(&store)
            }

            pub fn get_password(&self) -> Result<String, DebugKeyringError> {
                let store = self.load_store()?;
                let key = format!("{}:{}", self.service, self.user);
                store
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| DebugKeyringError("No entry found".to_string()))
            }

            pub fn delete_password(&self) -> Result<(), DebugKeyringError> {
                let mut store = self.load_store()?;
                let key = format!("{}:{}", self.service, self.user);
                store.remove(&key);
                self.save_store(&store)
            }

            fn keyring_path(&self) -> PathBuf {
                dirs::config_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("alien")
                    .join("debug-keyring.json")
            }

            fn load_store(&self) -> Result<HashMap<String, String>, DebugKeyringError> {
                let path = self.keyring_path();
                if path.exists() {
                    let content = fs::read_to_string(path).map_err(|e| {
                        DebugKeyringError(format!("Failed to read keyring file: {}", e))
                    })?;
                    Ok(serde_json::from_str(&content).unwrap_or_default())
                } else {
                    Ok(HashMap::new())
                }
            }

            fn save_store(&self, store: &HashMap<String, String>) -> Result<(), DebugKeyringError> {
                let path = self.keyring_path();
                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir).map_err(|e| {
                        DebugKeyringError(format!("Failed to create config dir: {}", e))
                    })?;
                }
                let content = serde_json::to_string_pretty(store).map_err(|e| {
                    DebugKeyringError(format!("Failed to serialize keyring: {}", e))
                })?;
                alien_core::file_utils::write_secret_file(&path, content.as_bytes())
                    .map_err(|e| {
                        DebugKeyringError(format!("Failed to write keyring file: {}", e))
                    })?;
                Ok(())
            }
        }
    }
}

// Re-export platform-only functions
#[cfg(feature = "platform")]
pub use oauth_flow::{force_login, get_auth_http, logout, store_tokens, try_bearer_client};

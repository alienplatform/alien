//! OCI Registry Proxy — transparent HTTPS reverse proxy with auth.
//!
//! The manager exposes `/v2/` as an OCI Distribution endpoint. Every request
//! is authenticated, then forwarded to the upstream cloud registry (ECR/GAR/ACR)
//! with injected credentials. The path is forwarded **unchanged**.
//!
//! ## Auth
//!
//! **Push** (POST/PUT/PATCH): Admin or developer token only.
//! **Pull** (GET/HEAD): Deployment token — validates the requested repo exists
//! in the deployment's release.
//!
//! ## Performance
//!
//! - Upstream credentials are cached (keyed by repo+permissions, TTL from expiry)
//! - Pull validation (deployment→release→repo names) is cached per deployment
//! - A single shared `reqwest::Client` is used for all upstream requests

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use tracing::{debug, warn};
use url::Url;

use alien_bindings::traits::{
    ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions,
};
use alien_bindings::BindingsProviderApi;
use alien_core::Platform;

use super::AppState;
use crate::auth::{Scope, Subject};

mod auth;
mod routing;
mod upload_session;

#[cfg(test)]
use auth::extract_repo_names;
use auth::{require_push_auth, validate_pull_access};
#[cfg(test)]
use routing::project_id_after_prefix;
pub use routing::{RegistryRoute, RegistryRoutingTable};
#[cfg(test)]
use upload_session::sign_upload_session;
use upload_session::{
    rewrite_location_with_upload_session_auth, strip_upload_session_auth_params,
    verify_upload_session_auth,
};

type HmacSha256 = Hmac<Sha256>;

const UPLOAD_SESSION_VERSION: &str = "1";
const UPLOAD_SESSION_TTL_SECONDS: i64 = 3600;
const UPLOAD_SESSION_VERSION_PARAM: &str = "_alien_v";
const UPLOAD_SESSION_REPO_PARAM: &str = "_alien_repo";
const UPLOAD_SESSION_EXPIRES_PARAM: &str = "_alien_exp";
const UPLOAD_SESSION_SIGNATURE_PARAM: &str = "_alien_sig";
const UPLOAD_SESSION_SIGNING_CONTEXT: &[u8] = b"registry-upload-session-signing";

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        // Some reverse proxies normalize the OCI client's `/v2/` probe to
        // `/v2`. Both forms must return the registry auth challenge or the
        // client will treat the registry as anonymous and omit credentials
        // from subsequent push requests.
        .route("/v2", get(version_check))
        .route("/v2/", get(version_check))
        .route(
            "/v2/{*path}",
            get(proxy_pull)
                .head(proxy_pull)
                .post(proxy_push)
                .put(proxy_push)
                .patch(proxy_push),
        )
        // GAR uses /artifacts-uploads/ for blob upload sessions.
        // These are session-scoped URLs that the proxy rewrites to go through
        // itself (so credentials are injected). Forward them to upstream unchanged.
        .route(
            "/artifacts-uploads/{*path}",
            axum::routing::put(proxy_upload_session)
                .patch(proxy_upload_session)
                .post(proxy_upload_session),
        )
}

// ---------------------------------------------------------------------------
// Caches
// ---------------------------------------------------------------------------

/// Cached upstream credentials. Avoids calling `generate_credentials()` on
/// every HTTP request (~50 requests per image push/pull).
pub struct CredentialCache {
    entries: std::sync::RwLock<HashMap<String, CachedCredential>>,
    generation_locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

struct CachedCredential {
    creds: ArtifactRegistryCredentials,
    created_at: Instant,
    ttl: Duration,
}

impl CredentialCache {
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(HashMap::new()),
            generation_locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, key: &str) -> Option<ArtifactRegistryCredentials> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(key)?;
        if entry.created_at.elapsed() < entry.ttl {
            Some(entry.creds.clone())
        } else {
            None
        }
    }

    fn insert(&self, key: String, creds: ArtifactRegistryCredentials, ttl: Duration) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                key,
                CachedCredential {
                    creds,
                    created_at: Instant::now(),
                    ttl,
                },
            );
        }
    }

    fn generation_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .generation_locks
            .lock()
            .expect("credential cache generation lock poisoned");
        locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }
}

impl Default for CredentialCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cached pull validation: deployment_id → (release_id, repo_names, created_at).
/// Avoids 3 DB queries per pull request (~20 requests per image pull).
pub struct PullValidationCache {
    entries: std::sync::RwLock<HashMap<String, CachedPullValidation>>,
}

struct CachedPullValidation {
    release_id: String,
    repo_names: Vec<String>,
    created_at: Instant,
}

impl PullValidationCache {
    /// Cache TTL — entries expire after 5 minutes.
    const TTL: Duration = Duration::from_secs(300);

    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(HashMap::new()),
        }
    }

    fn get(&self, deployment_id: &str) -> Option<(String, Vec<String>)> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(deployment_id)?;
        if entry.created_at.elapsed() < Self::TTL {
            Some((entry.release_id.clone(), entry.repo_names.clone()))
        } else {
            None
        }
    }

    fn insert(&self, deployment_id: String, release_id: String, repo_names: Vec<String>) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                deployment_id,
                CachedPullValidation {
                    release_id,
                    repo_names,
                    created_at: Instant::now(),
                },
            );
        }
    }

    /// Invalidate a specific deployment's cache (e.g., on release change).
    pub fn invalidate(&self, deployment_id: &str) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(deployment_id);
        }
    }
}

impl Default for PullValidationCache {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// OCI error response format
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OciError {
    code: &'static str,
    message: String,
    detail: Option<String>,
}

#[derive(Serialize)]
struct OciErrorResponse {
    errors: Vec<OciError>,
}

fn oci_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    let body = OciErrorResponse {
        errors: vec![OciError {
            code,
            message: message.into(),
            detail: None,
        }],
    };

    let body_str = serde_json::to_string(&body).unwrap_or_default();

    let mut response = (status, body_str).into_response();
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());

    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            "www-authenticate",
            "Basic realm=\"alien-manager\"".parse().unwrap(),
        );
    }

    response
}

// ---------------------------------------------------------------------------
// Version check
// ---------------------------------------------------------------------------

/// `GET /v2/` — OCI Distribution spec requires this endpoint to exist.
async fn version_check(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = super::auth::require_auth(&state, &headers).await {
        return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string());
    }
    (StatusCode::OK, "{}").into_response()
}

// ---------------------------------------------------------------------------
// Push handler (POST/PUT/PATCH — admin auth)
// ---------------------------------------------------------------------------

/// Push: authenticate as admin, forward to upstream unchanged.
///
/// Two auth shapes are accepted:
///
/// * **Bearer**: the normal `alien release` push flow — the CLI sends an
///   `Authorization: Bearer <workspace-token>` on every request and we
///   validate it through `require_auth`.
///
/// * **Signed upload-session URL**: cloud OCI registries (ECR, GCR, etc.)
///   return pre-signed S3/GCS URLs in the `Location` header of a successful
///   POST to `/v2/{repo}/blobs/uploads/`. Push clients (`dockdash` included)
///   treat that URL as self-authenticating — they PUT/PATCH to it WITHOUT
///   the Bearer token they were sending on the initial POST. To stay
///   compatible we sign the rewritten Location ourselves
///   (`rewrite_location_with_upload_session_auth` below), and accept
///   subsequent PUT/PATCH requests to the same path on the basis of the
///   signature alone. Without this branch every `alien release` push
///   would die at the layer-upload PUT with a confusing
///   `UNAUTHORIZED: Authentication required`.
async fn proxy_push(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: axum::http::Method,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    body: Body,
) -> Response {
    let oci_path_str = canonicalize_oci_push_path(path.trim_start_matches('/'));
    // The signing flow in `rewrite_location_with_upload_session_auth` signs
    // the URL's full path (`/v2/...`). axum's `Path` extractor on the
    // `/v2/{*path}` route strips the leading `/v2/`, so rebuild it before
    // calling the verifier so the path-component of the HMAC matches the
    // one used when signing.
    let full_path = format!("/v2/{}", oci_path_str);
    let signed_session_repo = if is_oci_upload_session_path(&full_path)
        && query.contains_key(UPLOAD_SESSION_VERSION_PARAM)
    {
        match verify_upload_session_auth(&state.config.response_signing_key, &full_path, &query) {
            Ok(repo) => Some(repo),
            Err(e) => return e,
        }
    } else {
        None
    };

    let repo_name = if let Some(ref repo) = signed_session_repo {
        // Signed-URL bypass: the path's repo is implied by the signature,
        // not by Bearer auth. Trust the signature's repo.
        repo.clone()
    } else {
        let subject = match super::auth::require_auth(&state, &headers).await {
            Ok(s) => s,
            Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
        };
        let repo_name = extract_repo_name(&path);
        if let Err(e) = require_push_auth(&state, &subject, &repo_name) {
            return e;
        }
        repo_name
    };

    let upstream_query = if signed_session_repo.is_some() {
        strip_upload_session_auth_params(&query)
    } else {
        query
    };
    let qs = query_string(&upstream_query);
    let oci_path = format!("{}{}", oci_path_str, qs);
    forward_to_upstream(
        &state,
        &method,
        &oci_path,
        &headers,
        Some(body),
        Some(&repo_name),
    )
    .await
}

/// Restore the significant trailing slash on the OCI upload-init endpoint.
/// Reverse proxies commonly normalize it away, while upstream registries such
/// as ECR require `POST .../blobs/uploads/` and reject the slashless form.
fn canonicalize_oci_push_path(path: &str) -> std::borrow::Cow<'_, str> {
    if path.ends_with("/blobs/uploads") {
        std::borrow::Cow::Owned(format!("{path}/"))
    } else {
        std::borrow::Cow::Borrowed(path)
    }
}

/// True if the path matches an OCI blob-upload-session URL —
/// `/v2/{repo}/blobs/uploads/{session-id}` with a session-id segment after
/// `/uploads/`. The initial POST to `/v2/{repo}/blobs/uploads/` (no
/// session-id segment) returns false here so it still requires Bearer.
fn is_oci_upload_session_path(path: &str) -> bool {
    let path = path.trim_start_matches('/');
    if !path.starts_with("v2/") {
        return false;
    }
    let Some(idx) = path.find("/blobs/uploads/") else {
        return false;
    };
    let after = &path[idx + "/blobs/uploads/".len()..];
    !after.is_empty() && !after.starts_with('?')
}

// ---------------------------------------------------------------------------
// Upload session handler (GAR /artifacts-uploads/ paths)
// ---------------------------------------------------------------------------

/// Forward GAR upload session requests to the upstream registry.
///
/// GAR returns `/artifacts-uploads/...` as Location headers for blob uploads.
/// The proxy rewrites these to go through itself so credentials are injected.
/// This handler forwards them to the upstream unchanged (with the original path).
async fn proxy_upload_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: axum::http::Method,
    original_uri: axum::http::Uri,
    Query(query): Query<HashMap<String, String>>,
    body: Body,
) -> Response {
    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
    };

    let repo_name = match verify_upload_session_auth(
        &state.config.response_signing_key,
        original_uri.path(),
        &query,
    ) {
        Ok(repo_name) => repo_name,
        Err(e) => return e,
    };

    if let Err(e) = require_push_auth(&state, &subject, &repo_name) {
        return e;
    }

    // Forward the full path (including /artifacts-uploads/) to upstream.
    let upstream_query = strip_upload_session_auth_params(&query);
    let qs = query_string(&upstream_query);
    let raw_path = original_uri.path();
    let full_path = format!("{}{}", raw_path, qs);
    forward_to_upstream_raw(
        &state,
        &method,
        &full_path,
        &headers,
        Some(body),
        Some(&repo_name),
    )
    .await
}

// ---------------------------------------------------------------------------
// Pull handler (GET/HEAD — deployment auth + image validation)
// ---------------------------------------------------------------------------

/// Pull: authenticate as deployment, validate image access, forward to upstream.
async fn proxy_pull(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: axum::http::Method,
    Path(path): Path<String>,
) -> Response {
    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
    };

    let oci_path_str = path.trim_start_matches('/');
    let repo_name = extract_repo_name(oci_path_str);
    if let Err(e) = validate_pull_access(&state, &subject, &repo_name).await {
        return e;
    }

    forward_to_upstream(&state, &method, oci_path_str, &headers, None, None).await
}

// ---------------------------------------------------------------------------
// Core forwarding logic
// ---------------------------------------------------------------------------

/// Transparent reverse proxy: forward the OCI request to the upstream registry
/// with injected credentials. Path is forwarded unchanged.
async fn forward_to_upstream(
    state: &AppState,
    method: &axum::http::Method,
    oci_path: &str,
    original_headers: &HeaderMap,
    body: Option<Body>,
    upload_session_repo: Option<&str>,
) -> Response {
    let repo_name = extract_repo_name(oci_path);

    let artifact_registry = match load_artifact_registry_for_repo(state, &repo_name).await {
        Ok(ar) => ar,
        Err(e) => return e,
    };

    let upstream_endpoint = artifact_registry.registry_endpoint();

    let permissions = if *method == axum::http::Method::GET || *method == axum::http::Method::HEAD {
        ArtifactRegistryPermissions::Pull
    } else {
        ArtifactRegistryPermissions::PushPull
    };

    // Check credential cache before calling generate_credentials().
    // Include the registry endpoint in the cache key to prevent cross-registry
    // credential contamination when multiple registries are configured.
    let cache_key = format!("{}:{}:{:?}", upstream_endpoint, repo_name, permissions);
    let creds = if let Some(cached) = state.credential_cache.get(&cache_key) {
        cached
    } else {
        let generation_lock = state.credential_cache.generation_lock(&cache_key);
        let _guard = generation_lock.lock().await;

        if let Some(cached) = state.credential_cache.get(&cache_key) {
            cached
        } else {
            let fresh = match artifact_registry
                .generate_credentials(&repo_name, permissions, Some(3600))
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "Failed to generate upstream credentials");
                    return oci_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL_ERROR",
                        "Failed to generate upstream credentials",
                    );
                }
            };

            // Cache with TTL derived from expiry, or default 5 minutes.
            let ttl = fresh
                .expires_at
                .as_deref()
                .and_then(|exp| {
                    chrono::DateTime::parse_from_rfc3339(exp).ok().map(|dt| {
                        let remaining = dt.timestamp() - chrono::Utc::now().timestamp();
                        // Use 80% of remaining time as TTL (refresh before expiry)
                        Duration::from_secs((remaining.max(0) as u64) * 4 / 5)
                    })
                })
                .unwrap_or(Duration::from_secs(300));

            state.credential_cache.insert(cache_key, fresh.clone(), ttl);
            fresh
        }
    };

    let upstream_url = format!(
        "{}/v2/{}",
        upstream_endpoint.trim_end_matches('/'),
        oci_path
    );
    forward_request(
        state,
        method,
        &upstream_url,
        &upstream_endpoint,
        &creds,
        original_headers,
        body,
        upload_session_repo,
    )
    .await
}

/// Forward a raw-path request to the upstream registry (for non-/v2/ paths like /artifacts-uploads/).
async fn forward_to_upstream_raw(
    state: &AppState,
    method: &axum::http::Method,
    raw_path: &str,
    original_headers: &HeaderMap,
    body: Option<Body>,
    upload_session_repo: Option<&str>,
) -> Response {
    // GAR upload session paths have the format:
    // /artifacts-uploads/namespaces/{project}/repositories/{repo}/uploads/{id}
    // Extract "{project}/{repo}" as the repo name for routing.
    let repo_name = extract_gar_upload_repo(raw_path);
    let artifact_registry = match load_artifact_registry_for_repo(state, &repo_name).await {
        Ok(ar) => ar,
        Err(e) => return e,
    };

    let upstream_endpoint = artifact_registry.registry_endpoint();
    if upstream_endpoint.is_empty() {
        return oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Artifact registry does not expose a registry endpoint",
        );
    }

    // Use PushPull permissions — upload session paths are always push operations.
    let permissions = ArtifactRegistryPermissions::PushPull;
    let cache_key = format!("upload-session:{}:{:?}", upstream_endpoint, permissions);
    let creds = if let Some(cached) = state.credential_cache.get(&cache_key) {
        cached
    } else {
        let generation_lock = state.credential_cache.generation_lock(&cache_key);
        let _guard = generation_lock.lock().await;

        if let Some(cached) = state.credential_cache.get(&cache_key) {
            cached
        } else {
            let fresh = match artifact_registry
                .generate_credentials("", permissions, Some(3600))
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "Failed to generate upstream credentials for upload session");
                    return oci_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL_ERROR",
                        "Failed to generate upstream credentials",
                    );
                }
            };

            let ttl = fresh
                .expires_at
                .as_deref()
                .and_then(|exp| {
                    chrono::DateTime::parse_from_rfc3339(exp).ok().map(|dt| {
                        let remaining = dt.timestamp() - chrono::Utc::now().timestamp();
                        Duration::from_secs((remaining.max(0) as u64) * 4 / 5)
                    })
                })
                .unwrap_or(Duration::from_secs(300));

            state.credential_cache.insert(cache_key, fresh.clone(), ttl);
            fresh
        }
    };

    let upstream_url = format!("{}{}", upstream_endpoint.trim_end_matches('/'), raw_path);
    forward_request(
        state,
        method,
        &upstream_url,
        &upstream_endpoint,
        &creds,
        original_headers,
        body,
        upload_session_repo,
    )
    .await
}

/// Shared HTTP forwarding logic. Sends the request to `upstream_url` with injected
/// credentials, streams body and response, rewrites Location headers.
async fn forward_request(
    state: &AppState,
    method: &axum::http::Method,
    upstream_url: &str,
    upstream_endpoint: &str,
    creds: &ArtifactRegistryCredentials,
    original_headers: &HeaderMap,
    body: Option<Body>,
    upload_session_repo: Option<&str>,
) -> Response {
    debug!(%method, %upstream_url, "Forwarding to upstream");

    // Use shared HTTP client from AppState.
    let mut req = state.http_client.request(method.clone(), upstream_url);

    // Forward relevant request headers.
    for key in &["content-type", "content-length", "accept"] {
        if let Some(val) = original_headers.get(*key) {
            req = req.header(*key, val);
        }
    }

    // Inject upstream auth.
    use alien_bindings::traits::RegistryAuthMethod;
    match creds.auth_method {
        RegistryAuthMethod::Bearer => {
            req = req.bearer_auth(&creds.password);
        }
        RegistryAuthMethod::Basic => {
            if !creds.username.is_empty() || !creds.password.is_empty() {
                req = req.basic_auth(&creds.username, Some(&creds.password));
            }
        }
    }

    // Stream body to upstream (push operations).
    // Uses streaming to avoid buffering large blobs (100s of MB) in memory.
    if let Some(body) = body {
        req = req.body(reqwest::Body::wrap_stream(body.into_data_stream()));
    }

    // Send.
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, %upstream_url, "Upstream request failed");
            return oci_error(
                StatusCode::BAD_GATEWAY,
                "INTERNAL_ERROR",
                "Upstream request failed",
            );
        }
    };

    // Build proxy response — stream body, rewrite headers.
    let status = resp.status();
    let resp_headers = resp.headers().clone();

    debug!(%method, %upstream_url, upstream_status = %status.as_u16(), "Upstream response");

    // Stream the response body instead of buffering.
    let resp_body = Body::from_stream(resp.bytes_stream());
    let mut response = (
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        resp_body,
    )
        .into_response();

    let upstream_host = upstream_endpoint.trim_end_matches('/');
    let proxy_base = proxy_base_url(original_headers, &state.config.base_url());
    let proxy_host = proxy_base.trim_end_matches('/');

    for (key, value) in &resp_headers {
        if key == "location" {
            if let Ok(location) = value.to_str() {
                debug!(raw_location = %location, "Rewriting Location header");
                if location.starts_with('/') {
                    // Relative URL (e.g., GAR's /artifacts-uploads/...).
                    // Rewrite to go through the proxy so credentials are injected.
                    let proxied = match rewrite_location_with_upload_session_auth(
                        &format!("{}{}", proxy_host, location),
                        upload_session_repo,
                        &state.config.response_signing_key,
                    ) {
                        Ok(url) => url,
                        Err(e) => return e,
                    };
                    if let Ok(v) = proxied.parse() {
                        response.headers_mut().insert(key, v);
                        continue;
                    }
                } else if location.contains(upstream_host) {
                    // Absolute upstream URL — rewrite host to proxy.
                    let rewritten = location.replace(upstream_host, proxy_host);
                    let rewritten = match rewrite_location_with_upload_session_auth(
                        &rewritten,
                        upload_session_repo,
                        &state.config.response_signing_key,
                    ) {
                        Ok(url) => url,
                        Err(e) => return e,
                    };
                    if let Ok(v) = rewritten.parse() {
                        response.headers_mut().insert(key, v);
                        continue;
                    }
                }
                // Other absolute URLs — pass through unchanged.
                response.headers_mut().insert(key, value.clone());
                continue;
            }
        }
        // Skip hop-by-hop headers.
        if key != "transfer-encoding" && key != "connection" {
            response.headers_mut().insert(key, value.clone());
        }
    }

    response
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the repository name from an OCI path.
///
/// The repo name is everything before the first OCI operation keyword
/// (/manifests/, /blobs/, /uploads/). Repo names can be multi-segment
/// (e.g., "gcp-project/gar-repo/alien-prj-123" for GAR).
fn extract_repo_name(oci_path: &str) -> String {
    for keyword in &["/manifests/", "/blobs/", "/uploads/"] {
        if let Some(idx) = oci_path.find(keyword) {
            return oci_path[..idx].to_string();
        }
    }
    oci_path.split('/').next().unwrap_or(oci_path).to_string()
}

/// Extract repo name from a GAR /artifacts-uploads/ path.
///
/// GAR upload session paths: `/artifacts-uploads/namespaces/{project}/repositories/{repo}/uploads/{id}`
/// Returns `"{project}/{repo}"` which matches the GAR routing table prefix.
fn extract_gar_upload_repo(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    // Look for /namespaces/{project}/repositories/{repo}/
    for (i, part) in parts.iter().enumerate() {
        if *part == "namespaces" && i + 3 < parts.len() && parts[i + 2] == "repositories" {
            return format!("{}/{}", parts[i + 1], parts[i + 3]);
        }
    }
    // Fallback: empty string (will match catch-all if available)
    String::new()
}

fn query_string(params: &HashMap<String, String>) -> String {
    if params.is_empty() {
        String::new()
    } else {
        let qs: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect();
        format!("?{}", qs.join("&"))
    }
}

fn proxy_base_url(original_headers: &HeaderMap, fallback_base_url: &str) -> String {
    let host = first_header_value(original_headers, "x-forwarded-host")
        .or_else(|| forwarded_header_param(original_headers, "host"))
        .or_else(|| first_header_value(original_headers, "host"));

    let Some(host) = host else {
        return fallback_base_url.trim_end_matches('/').to_string();
    };

    let proto = first_header_value(original_headers, "x-forwarded-proto")
        .or_else(|| forwarded_header_param(original_headers, "proto"))
        .unwrap_or_else(|| {
            if is_loopback_host(&host) {
                fallback_base_url
                    .split_once("://")
                    .map(|(scheme, _)| scheme)
                    .unwrap_or("http")
                    .to_string()
            } else {
                "https".to_string()
            }
        });

    format!("{}://{}", proto, host)
        .trim_end_matches('/')
        .to_string()
}

fn first_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn forwarded_header_param(headers: &HeaderMap, param: &str) -> Option<String> {
    let header = headers.get("forwarded")?.to_str().ok()?;
    header.split(',').next()?.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(param) {
            let value = value.trim().trim_matches('"');
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    })
}

fn is_loopback_host(host: &str) -> bool {
    let host_without_port = host
        .strip_prefix('[')
        .and_then(|h| h.split_once(']').map(|(h, _)| h))
        .unwrap_or_else(|| host.split(':').next().unwrap_or(host));

    matches!(host_without_port, "localhost" | "127.0.0.1" | "::1")
}

/// Load the artifact registry for a specific repository path.
///
/// Uses the routing table to find the correct upstream registry based on the
/// repository path prefix. Falls back to the legacy provider scan when the
/// routing table is empty (backwards compatibility during migration).
async fn load_artifact_registry_for_repo(
    state: &AppState,
    repo_name: &str,
) -> Result<Arc<dyn ArtifactRegistry>, Response> {
    if !state.registry_routing_table.is_empty() {
        let route = state
            .registry_routing_table
            .resolve(repo_name)
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    format!(
                        "No artifact registry configured for repository path '{}'",
                        repo_name
                    ),
                )
            })?;

        let ar = route
            .provider
            .load_artifact_registry(&route.binding_name)
            .await
            .map_err(|e| {
                warn!(error = %e, prefix = %route.prefix, "Failed to load artifact registry");
                oci_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Failed to load artifact registry",
                )
            })?;

        if ar.registry_endpoint().is_empty() {
            return Err(oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Artifact registry does not expose a registry endpoint",
            ));
        }

        return Ok(ar);
    }

    // Legacy fallback: try providers in order when no routing table is configured.
    if let Some(ref primary) = state.bindings_provider {
        if let Ok(ar) = primary.load_artifact_registry("artifact-registry").await {
            if !ar.registry_endpoint().is_empty() {
                return Ok(ar);
            }
        }
        if let Ok(ar) = primary.load_artifact_registry("artifacts").await {
            if !ar.registry_endpoint().is_empty() {
                return Ok(ar);
            }
        }
    }

    for platform in &state.config.targets {
        if let Some(target) = state.target_bindings_providers.get(platform) {
            if let Ok(ar) = target.load_artifact_registry("artifacts").await {
                if !ar.registry_endpoint().is_empty() {
                    return Ok(ar);
                }
            }
        }
    }

    Err(oci_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL_ERROR",
        "No artifact registry binding configured on this manager",
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

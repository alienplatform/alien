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
use serde::Serialize;
use tracing::{debug, info, warn};

use alien_bindings::traits::{
    ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions,
};
use alien_bindings::BindingsProviderApi;
use alien_core::Platform;

use super::AppState;
use crate::auth::{Scope, Subject};

// ---------------------------------------------------------------------------
// Registry routing table
// ---------------------------------------------------------------------------

/// A route mapping a repository path prefix to an artifact registry provider.
#[derive(Clone)]
pub struct RegistryRoute {
    pub prefix: String,
    pub platform: Platform,
    pub provider: Arc<dyn BindingsProviderApi>,
    pub binding_name: String,
}

/// Routes OCI requests to the correct upstream registry based on repo path prefix.
/// Built once at startup from the manager's artifact registry configuration.
pub struct RegistryRoutingTable {
    /// Routes sorted by prefix length descending (longest prefix match wins).
    routes: Vec<RegistryRoute>,
}

impl RegistryRoutingTable {
    pub fn new(mut routes: Vec<RegistryRoute>) -> Self {
        // Sort by prefix length descending for longest-prefix match.
        routes.sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));
        Self { routes }
    }

    /// Find the registry route that matches the given repo name.
    pub fn resolve(&self, repo_name: &str) -> Option<&RegistryRoute> {
        self.routes.iter().find(|r| {
            if r.prefix.is_empty() {
                // Empty prefix = catch-all fallback (local registry).
                true
            } else {
                repo_name.starts_with(&r.prefix)
            }
        })
    }

    /// Extract the project_id from an OCI repo path using this table's
    /// boot-time-static `prefix → platform` map. The provider that owns the
    /// matching route composes its full repo name as `{prefix}{sep}{name}` —
    /// `-` for ECR, `/` for GAR/ACR/Local — so we strip the prefix, strip the
    /// single separator byte, and take everything up to the next `/` as the
    /// project_id.
    ///
    /// Returns `None` when no route matches, when the suffix doesn't start
    /// with `-` or `/` (defense — a path that didn't go through a provider's
    /// `make_full_repo_name`), or when the extracted id is empty. Callers
    /// fall back to `"default"`; the [`crate::auth::Authz`] impl then
    /// decides whether to allow the push.
    pub fn project_id_for_repo<'a>(&self, repo_name: &'a str) -> Option<&'a str> {
        let route = self.resolve(repo_name)?;
        project_id_after_prefix(repo_name, route.prefix.as_str())
    }

    /// Get the repo prefix for a given platform.
    pub fn prefix_for_platform(&self, platform: Platform) -> Option<&str> {
        self.routes
            .iter()
            .find(|r| r.platform == platform)
            .map(|r| r.prefix.as_str())
    }

    /// Return the list of explicitly configured (non-fallback) platforms.
    ///
    /// These are cloud platforms with dedicated artifact registries (ECR, GAR, ACR).
    /// The local catch-all fallback is excluded.
    pub fn configured_platforms(&self) -> Vec<Platform> {
        let mut platforms: Vec<Platform> = self
            .routes
            .iter()
            .filter(|r| r.platform != Platform::Local)
            .map(|r| r.platform)
            .collect();
        platforms.dedup();
        platforms
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Validate no overlapping prefixes (call at startup).
    pub fn validate(&self) -> Result<(), String> {
        for (i, a) in self.routes.iter().enumerate() {
            for b in self.routes.iter().skip(i + 1) {
                if !a.prefix.is_empty() && !b.prefix.is_empty() {
                    if a.prefix.starts_with(&b.prefix) || b.prefix.starts_with(&a.prefix) {
                        return Err(format!(
                            "Overlapping artifact registry prefixes: '{}' ({}) and '{}' ({})",
                            a.prefix, a.platform, b.prefix, b.platform
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Strip `prefix` from `repo_name`, then strip a single separator byte
/// (`-` for ECR, `/` for GAR/ACR/Local — empty prefix needs no separator),
/// and return everything up to the next `/`. See
/// [`RegistryRoutingTable::project_id_for_repo`] for the full algorithm
/// (including the route resolution this helper assumes has already happened).
///
/// Exposed at module level so unit tests can exercise the algorithm without
/// constructing a full `RegistryRoutingTable` (which would require a real
/// `BindingsProviderApi` and a tokio runtime).
fn project_id_after_prefix<'a>(repo_name: &'a str, prefix: &str) -> Option<&'a str> {
    let suffix = if prefix.is_empty() {
        repo_name
    } else {
        let rest = repo_name.strip_prefix(prefix)?;
        let first = rest.chars().next()?;
        if first != '-' && first != '/' {
            return None;
        }
        &rest[1..]
    };
    let pid = suffix.split('/').next()?;
    if pid.is_empty() {
        None
    } else {
        Some(pid)
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
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
async fn proxy_push(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: axum::http::Method,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    body: Body,
) -> Response {
    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
    };

    let repo_name = extract_repo_name(&path);
    if let Err(e) = require_push_auth(&state, &subject, &repo_name) {
        return e;
    }

    let qs = query_string(&query);
    let oci_path = format!("{}{}", path.trim_start_matches('/'), qs);
    forward_to_upstream(&state, &method, &oci_path, &headers, Some(body)).await
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

    // Upload-session URLs are signed by the upstream registry on a previous
    // push request; treat the path itself as the repo identifier for authz.
    let repo_name = original_uri.path().to_string();
    if let Err(e) = require_push_auth(&state, &subject, &repo_name) {
        return e;
    }

    // Forward the full path (including /artifacts-uploads/) to upstream.
    let qs = query_string(&query);
    let raw_path = original_uri.path();
    let full_path = format!("{}{}", raw_path, qs);
    forward_to_upstream_raw(&state, &method, &full_path, &headers, Some(body)).await
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

    forward_to_upstream(&state, &method, oci_path_str, &headers, None).await
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
    let proxy_base = state.config.base_url();
    let proxy_host = proxy_base.trim_end_matches('/');

    for (key, value) in &resp_headers {
        if key == "location" {
            if let Ok(location) = value.to_str() {
                debug!(raw_location = %location, "Rewriting Location header");
                if location.starts_with('/') {
                    // Relative URL (e.g., GAR's /artifacts-uploads/...).
                    // Rewrite to go through the proxy so credentials are injected.
                    let proxied = format!("{}{}", proxy_host, location);
                    if let Ok(v) = proxied.parse() {
                        response.headers_mut().insert(key, v);
                        continue;
                    }
                } else if location.contains(upstream_host) {
                    // Absolute upstream URL — rewrite host to proxy.
                    let rewritten = location.replace(upstream_host, proxy_host);
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
// Auth helpers
// ---------------------------------------------------------------------------

/// Validate that the caller has push permissions. The project_id comes
/// from the **routing table** — each provider composes repo names as
/// `{prefix}{sep}{name}` (`-` for ECR, `/` for GAR/ACR/Local), so the
/// routing-table prefix lookup gives us the project's name unambiguously.
/// Pushes that don't match any prefix fall back to `"default"`; the
/// configured [`crate::auth::Authz`] impl decides whether to allow.
fn require_push_auth(
    state: &AppState,
    subject: &Subject,
    repo_name: &str,
) -> Result<(), Response> {
    let project_id = state
        .registry_routing_table
        .project_id_for_repo(repo_name)
        .unwrap_or("default");
    if state.authz.can_push_image(subject, project_id, repo_name) {
        Ok(())
    } else {
        Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            "Caller cannot push images to this project.",
        ))
    }
}

/// Validate that a deployment token can access the requested repo.
///
/// Uses the pull validation cache to avoid repeated DB lookups. Workspace-
/// scoped subjects bypass repo validation (they can pull anything in the
/// workspace).
async fn validate_pull_access(
    state: &AppState,
    subject: &Subject,
    repo_name: &str,
) -> Result<(), Response> {
    let deployment_id = match &subject.scope {
        Scope::Workspace | Scope::Project { .. } => return Ok(()),
        Scope::DeploymentGroup { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Registry proxy pulls require a deployment token",
            ))
        }
        Scope::Deployment { deployment_id, .. } => deployment_id.as_str(),
    };

    // Check cache first.
    let repo_names = if let Some((_release_id, cached_repos)) =
        state.pull_validation_cache.get(deployment_id)
    {
        cached_repos
    } else {
        // Cache miss — query DB.
        let deployment = state
            .deployment_store
            .get_deployment(deployment_id)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to get deployment for registry proxy");
                oci_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Failed to resolve deployment",
                )
            })?
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    format!("Deployment {} not found", deployment_id),
                )
            })?;

        let release_id = deployment
            .current_release_id
            .as_deref()
            .or(deployment.desired_release_id.as_deref())
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    "Deployment has no release",
                )
            })?
            .to_string();

        let system = crate::auth::Subject::system();
        let release = state
            .release_store
            .get_release(&system, &release_id)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to get release for registry proxy");
                oci_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Failed to resolve release",
                )
            })?
            .ok_or_else(|| {
                oci_error(
                    StatusCode::NOT_FOUND,
                    "NAME_UNKNOWN",
                    format!("Release {} not found", release_id),
                )
            })?;

        let repos = release
            .stacks
            .values()
            .flat_map(|stack| extract_repo_names(stack))
            .collect::<Vec<_>>();

        // Cache the result.
        state
            .pull_validation_cache
            .insert(deployment_id.to_string(), release_id, repos.clone());

        repos
    };

    if !repo_names.iter().any(|r| r == repo_name) {
        return Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            format!(
                "Repository '{}' not found in deployment's release",
                repo_name
            ),
        ));
    }

    Ok(())
}

/// Extract the set of repo names from a release's stack.
fn extract_repo_names(stack: &alien_core::Stack) -> Vec<String> {
    use alien_core::image_rewrite::strip_registry_host;
    use alien_core::{Container, ContainerCode, Function, FunctionCode};

    let mut repos = Vec::new();

    for (_resource_id, entry) in stack.resources() {
        let image = if let Some(func) = entry.config.downcast_ref::<Function>() {
            match &func.code {
                FunctionCode::Image { image } => Some(image.as_str()),
                FunctionCode::Source { .. } => None,
            }
        } else if let Some(container) = entry.config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => Some(image.as_str()),
                ContainerCode::Source { .. } => None,
            }
        } else {
            None
        };

        if let Some(image_uri) = image {
            if let Some(stripped) = strip_registry_host(image_uri) {
                let repo = stripped.split(':').next().unwrap_or(&stripped);
                let repo = repo.split('@').next().unwrap_or(repo);
                if !repo.is_empty() && !repos.contains(&repo.to_string()) {
                    repos.push(repo.to_string());
                }
            }
        }
    }

    repos
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
mod tests {
    use super::*;
    use alien_core::image_rewrite::strip_registry_host;

    #[test]
    fn test_strip_registry_host_gar() {
        assert_eq!(
            strip_registry_host("us-central1-docker.pkg.dev/project/repo:tag"),
            Some("project/repo:tag".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_ecr() {
        assert_eq!(
            strip_registry_host("123456.dkr.ecr.us-east-1.amazonaws.com/repo:tag"),
            Some("repo:tag".to_string())
        );
    }

    #[test]
    fn test_strip_registry_host_localhost() {
        assert_eq!(
            strip_registry_host("localhost:5000/repo:tag"),
            Some("repo:tag".to_string())
        );
    }

    #[test]
    fn test_extract_repo_name_flat() {
        assert_eq!(extract_repo_name("alien-e2e/manifests/v1"), "alien-e2e");
    }

    #[test]
    fn test_extract_repo_name_gar_multi_segment() {
        assert_eq!(
            extract_repo_name("my-project/alien-repo/alien-prj-123/manifests/v1"),
            "my-project/alien-repo/alien-prj-123"
        );
    }

    #[test]
    fn test_extract_repo_name_blobs() {
        assert_eq!(
            extract_repo_name("alien-e2e/blobs/sha256:abc123"),
            "alien-e2e"
        );
    }

    #[test]
    fn test_extract_repo_name_uploads() {
        assert_eq!(
            extract_repo_name("alien-e2e/blobs/uploads/uuid-123"),
            "alien-e2e"
        );
    }

    // -----------------------------------------------------------------------
    // project_id_after_prefix — the algorithm behind
    // RegistryRoutingTable::project_id_for_repo. Tests target the free
    // helper because constructing a full `RegistryRoutingTable` requires a
    // real `BindingsProviderApi` and a tokio runtime.
    // -----------------------------------------------------------------------

    #[test]
    fn project_id_aws_ecr_dash_separator() {
        assert_eq!(
            project_id_after_prefix("alien-artifacts-prj_xxx", "alien-artifacts"),
            Some("prj_xxx")
        );
        assert_eq!(
            project_id_after_prefix("alien-artifacts-prj_xxx/sub", "alien-artifacts"),
            Some("prj_xxx")
        );
    }

    #[test]
    fn project_id_gar_slash_separator() {
        assert_eq!(
            project_id_after_prefix(
                "alien-dev-1/alien-artifacts/prj_xxx",
                "alien-dev-1/alien-artifacts",
            ),
            Some("prj_xxx")
        );
    }

    #[test]
    fn project_id_local_slash_separator() {
        assert_eq!(
            project_id_after_prefix("artifacts/default/prj_xxx", "artifacts/default"),
            Some("prj_xxx")
        );
        assert_eq!(
            project_id_after_prefix(
                "artifacts/default/prj_xxx/release-v1",
                "artifacts/default",
            ),
            Some("prj_xxx")
        );
    }

    #[test]
    fn project_id_acr_empty_prefix() {
        assert_eq!(project_id_after_prefix("prj_xxx", ""), Some("prj_xxx"));
        assert_eq!(project_id_after_prefix("prj_xxx/sub", ""), Some("prj_xxx"));
    }

    #[test]
    fn project_id_rejects_malformed_separator() {
        // No `-` or `/` after the prefix — defense against repos that didn't
        // go through `make_full_repo_name`.
        assert_eq!(
            project_id_after_prefix("alien-artifactsXprj_xxx", "alien-artifacts"),
            None
        );
    }

    #[test]
    fn project_id_rejects_empty_id() {
        assert_eq!(
            project_id_after_prefix("alien-artifacts-", "alien-artifacts"),
            None
        );
    }

    #[test]
    fn project_id_rejects_bare_prefix() {
        // No suffix at all after the prefix.
        assert_eq!(
            project_id_after_prefix("alien-artifacts", "alien-artifacts"),
            None
        );
    }

    #[test]
    fn project_id_rejects_unrelated_prefix() {
        // The prefix isn't actually a prefix of repo_name — `strip_prefix` returns None.
        assert_eq!(
            project_id_after_prefix("unknown/path/prj_xxx", "alien-artifacts"),
            None
        );
    }
}

//! OCI Registry Proxy — transparent HTTPS reverse proxy with auth.
//!
//! The manager exposes `/v2/` as an OCI Distribution endpoint. Every request
//! is authenticated, then forwarded to the upstream cloud registry (ECR/GAR/ACR)
//! with injected credentials. The path is forwarded unchanged once its repository
//! name and trailing segments pass validation.
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
use crate::auth::{Role, Scope, Subject};

type HmacSha256 = Hmac<Sha256>;

const UPLOAD_SESSION_VERSION: &str = "1";
const UPLOAD_SESSION_TTL_SECONDS: i64 = 3600;
const UPLOAD_SESSION_VERSION_PARAM: &str = "_alien_v";
const UPLOAD_SESSION_REPO_PARAM: &str = "_alien_repo";
const UPLOAD_SESSION_EXPIRES_PARAM: &str = "_alien_exp";
const UPLOAD_SESSION_SIGNATURE_PARAM: &str = "_alien_sig";
const UPLOAD_SESSION_SIGNING_CONTEXT: &[u8] = b"registry-upload-session-signing";

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
    pub fn new(mut routes: Vec<RegistryRoute>) -> Result<Self, String> {
        Self::validate_unique_prefixes(&routes)?;
        // Sort by prefix length descending for longest-prefix match.
        routes.sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));
        Ok(Self { routes })
    }

    /// Find the registry route that matches the given repo name.
    pub fn resolve(&self, repo_name: &str) -> Option<&RegistryRoute> {
        self.routes.iter().find(|r| {
            if r.prefix.is_empty() {
                // Empty prefix = catch-all; construction rejects more than one.
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
        self.route_for_platform(platform).map(|r| r.prefix.as_str())
    }

    /// Route that stores images for `platform`.
    ///
    /// Cloud platforms use their own registry (ECR, GAR, ACR) when one is
    /// configured. Pull-delivered platforms (Kubernetes, Machines) have no
    /// registry of their own: their images live in the primary registry (the
    /// route registered as `Platform::Local`) and nodes pull them through this
    /// manager with the deployment's token.
    pub fn route_for_platform(&self, platform: Platform) -> Option<&RegistryRoute> {
        if let Some(route) = self.routes.iter().find(|r| r.platform == platform) {
            return Some(route);
        }
        match platform {
            Platform::Kubernetes | Platform::Machines => {
                self.routes.iter().find(|r| r.platform == Platform::Local)
            }
            _ => None,
        }
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

    /// Validate no ambiguous prefixes (call at startup).
    pub fn validate(&self) -> Result<(), String> {
        Self::validate_unique_prefixes(&self.routes)
    }

    fn validate_unique_prefixes(routes: &[RegistryRoute]) -> Result<(), String> {
        let mut seen: HashMap<&str, &RegistryRoute> = HashMap::new();
        for route in routes {
            if let Some(existing) = seen.insert(route.prefix.as_str(), route) {
                let prefix = if route.prefix.is_empty() {
                    "<empty>"
                } else {
                    route.prefix.as_str()
                };
                return Err(format!(
                    "Duplicate artifact registry prefix '{}' for {} binding '{}' and {} binding '{}'",
                    prefix,
                    existing.platform,
                    existing.binding_name,
                    route.platform,
                    route.binding_name
                ));
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
    if let Err(refused) = require_literal_oci_path(&path) {
        return refused;
    }
    let oci = match parse_oci_path(&canonicalize_oci_push_path(path.trim_start_matches('/'))) {
        Ok(oci) => oci,
        Err(refused) => return refused,
    };
    // The signing flow in `rewrite_location_with_upload_session_auth` signs
    // the URL's full path (`/v2/...`). axum's `Path` extractor on the
    // `/v2/{*path}` route strips the leading `/v2/`, so rebuild it before
    // calling the verifier so the path-component of the HMAC matches the
    // one used when signing.
    let full_path = format!("/v2/{}", oci.as_path());
    let signed_session_repo = if oci.is_upload_session()
        && query.contains_key(UPLOAD_SESSION_VERSION_PARAM)
    {
        match verify_upload_session_auth(&state.config.response_signing_key, &full_path, &query) {
            Ok(repo) if !is_valid_repository_name(&repo) => return invalid_repository_name(),
            Ok(repo) if !same_route(&state.registry_routing_table, &repo, &oci.repo) => {
                return invalid_upload_session_auth()
            }
            Ok(repo) => Some(repo),
            Err(e) => return e,
        }
    } else {
        None
    };

    let (repo_name, upstream_query) = if let Some(ref repo) = signed_session_repo {
        // Signed upload session: the signature, not Bearer auth, carries the
        // repo. It can differ from the path's repo (GAR issues sessions under
        // its own package path) but not from its registry route.
        let mut query = strip_upload_session_auth_params(&query);
        strip_mount_params(&mut query);
        (repo.clone(), query)
    } else {
        let subject = match super::auth::require_auth(&state, &headers).await {
            Ok(s) => s,
            Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
        };
        if let Err(e) = require_push_auth(&state, &subject, &oci.repo) {
            return e;
        }
        let mut query = query;
        // Mount parameters apply only to the upload-init POST.
        if method == axum::http::Method::POST && oci.rest == "blobs/uploads/" {
            restrict_mount_source(&state, &subject, &oci.repo, &mut query).await;
        } else {
            strip_mount_params(&mut query);
        }
        (oci.repo.clone(), query)
    };
    let qs = query_string(&upstream_query);
    forward_to_upstream(
        &state,
        &method,
        &oci,
        &qs,
        &headers,
        Some(body),
        Some(&repo_name),
    )
    .await
}

/// The digest a cross-repository blob mount copies.
const MOUNT_PARAM: &str = "mount";
/// The repository a cross-repository blob mount copies from.
const MOUNT_SOURCE_PARAM: &str = "from";
/// The registry host of a cross-host mount source.
const MOUNT_ORIGIN_PARAM: &str = "origin";

/// A mount is forwarded only when `mount` is a valid digest and `from` names a repository on the
/// target's registry route that the caller may pull. `origin` is always dropped: the upstream
/// resolves `from` among its own repositories. Any other mount is forwarded as a plain upload, as
/// the registry does after a failed mount.
async fn restrict_mount_source(
    state: &AppState,
    subject: &Subject,
    repo_name: &str,
    query: &mut HashMap<String, String>,
) {
    // Only the spec's lowercase keys are checked, so other casings are dropped first.
    strip_mount_params_except(query, &[MOUNT_PARAM, MOUNT_SOURCE_PARAM]);
    let source = match (query.get(MOUNT_PARAM), query.get(MOUNT_SOURCE_PARAM)) {
        (Some(digest), Some(source))
            if is_valid_digest(digest)
                && is_mount_source_on_target_route(
                    &state.registry_routing_table,
                    source,
                    repo_name,
                ) =>
        {
            Some(source.clone())
        }
        _ => None,
    };
    let pullable = match source {
        Some(source) => match validate_pull_access(state, subject, &source).await {
            Ok(()) => true,
            Err(refused) => {
                if refused.status().is_server_error() {
                    warn!(%source, status = %refused.status(), "Mount source check failed; forwarding a plain upload");
                }
                false
            }
        },
        None => false,
    };
    if !pullable {
        strip_mount_params(query);
    }
}

/// Whether `source` is a valid repository name on the same registry route as `repo_name`.
fn is_mount_source_on_target_route(
    table: &RegistryRoutingTable,
    source: &str,
    repo_name: &str,
) -> bool {
    is_valid_repository_name(source) && same_route(table, source, repo_name)
}

/// Whether two repositories reach the same upstream registry, as a mount source and target or a
/// signed session and its path must. With no routing table there is a single registry.
fn same_route(table: &RegistryRoutingTable, a: &str, b: &str) -> bool {
    let prefix = |repo: &str| table.resolve(repo).map(|route| route.prefix.as_str());
    table.is_empty() || matches!((prefix(a), prefix(b)), (Some(x), Some(y)) if x == y)
}

fn strip_mount_params(query: &mut HashMap<String, String>) {
    strip_mount_params_except(query, &[]);
}

/// Removes `mount`, `from` and `origin` in any casing, except the exact keys in `keep`.
fn strip_mount_params_except(query: &mut HashMap<String, String>, keep: &[&str]) {
    query.retain(|key, _| {
        keep.contains(&key.as_str())
            || ![MOUNT_PARAM, MOUNT_SOURCE_PARAM, MOUNT_ORIGIN_PARAM]
                .iter()
                .any(|param| key.eq_ignore_ascii_case(param))
    });
}

/// The path capture is percent-decoded and the upstream URL is parsed from it again, so only a
/// path the URL parser keeps exactly as given is forwarded: no `?`, `#`, `\`, dot or empty
/// segment, or leftover `%`.
fn require_literal_oci_path(path: &str) -> Result<(), Response> {
    let path = path.trim_start_matches('/');
    let segments = path.strip_suffix('/').unwrap_or(path);
    let literal = !path.contains('%')
        && !segments.split('/').any(str::is_empty)
        && Url::parse(&format!("http://registry.invalid/v2/{path}")).is_ok_and(|url| {
            url.path() == format!("/v2/{path}") && url.query().is_none() && url.fragment().is_none()
        });
    if literal {
        Ok(())
    } else {
        Err(oci_error(
            StatusCode::BAD_REQUEST,
            "NAME_INVALID",
            "Registry paths must name a repository literally",
        ))
    }
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

/// An OCI upload-session URL, `/v2/{repo}/blobs/uploads/{id}`. The upload-init POST has no id and
/// still requires Bearer. The `/v2/{repo}/uploads/{id}` form is not signed: its clients send their
/// own auth.
fn is_oci_upload_session_path(path: &str) -> bool {
    path.trim_start_matches('/')
        .strip_prefix("v2/")
        .and_then(|oci| parse_oci_path(oci).ok())
        .is_some_and(|oci| oci.is_upload_session())
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
    if !is_safe_upload_session_path(original_uri.path()) {
        return oci_error(
            StatusCode::BAD_REQUEST,
            "BLOB_UPLOAD_INVALID",
            "Invalid upload session path",
        );
    }

    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return oci_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", e.to_string()),
    };

    let repo_name = match verify_upload_session_auth(
        &state.config.response_signing_key,
        original_uri.path(),
        &query,
    ) {
        Ok(repo_name) if is_valid_repository_name(&repo_name) => repo_name,
        Ok(_) => return invalid_repository_name(),
        Err(e) => return e,
    };

    if !same_route(
        &state.registry_routing_table,
        &extract_gar_upload_repo(original_uri.path()),
        &repo_name,
    ) {
        return invalid_upload_session_auth();
    }

    if let Err(e) = require_push_auth(&state, &subject, &repo_name) {
        return e;
    }

    // Forward the full path (including /artifacts-uploads/) to upstream.
    let mut upstream_query = strip_upload_session_auth_params(&query);
    strip_mount_params(&mut upstream_query);
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

    if let Err(refused) = require_literal_oci_path(&path) {
        return refused;
    }
    // Every pull passes the same name and reference validation before it
    // is routed, including charts and the Operator image.
    let oci_path_str = path.trim_start_matches('/');
    let oci = match parse_oci_path(oci_path_str) {
        Ok(oci) => oci,
        Err(refused) => return refused,
    };
    if oci_path_str.starts_with(super::charts::CHART_NAMESPACE) {
        return super::charts::serve(&state, &subject, &method, oci_path_str).await;
    }
    if oci.repo == super::operator_image::OPERATOR_REPOSITORY {
        return super::operator_image::serve(&state, &subject, &method, oci_path_str, &headers)
            .await;
    }
    if let Err(e) = validate_pull_access(&state, &subject, &oci.repo).await {
        return e;
    }

    forward_to_upstream(&state, &method, &oci, "", &headers, None, None).await
}

// ---------------------------------------------------------------------------
// Core forwarding logic
// ---------------------------------------------------------------------------

/// Transparent reverse proxy: forward the OCI request to the upstream registry
/// with injected credentials. The upstream path is the validated `oci` path.
async fn forward_to_upstream(
    state: &AppState,
    method: &axum::http::Method,
    oci: &OciPath,
    query_string: &str,
    original_headers: &HeaderMap,
    body: Option<Body>,
    upload_session_repo: Option<&str>,
) -> Response {
    let repo_name = oci.repo.as_str();

    let artifact_registry = match load_artifact_registry_for_repo(state, repo_name).await {
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
                .generate_credentials(repo_name, permissions, Some(3600))
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
        "{}/v2/{}{}",
        upstream_endpoint.trim_end_matches('/'),
        oci.as_path(),
        query_string
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
    for key in &[
        "content-type",
        "content-length",
        "content-range",
        "accept",
        "range",
        "if-match",
        "if-none-match",
        "if-modified-since",
        "if-unmodified-since",
    ] {
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
// GAR upload session auth
// ---------------------------------------------------------------------------

fn rewrite_location_with_upload_session_auth(
    location: &str,
    upload_session_repo: Option<&str>,
    signing_key: &[u8],
) -> Result<String, Response> {
    let mut url = match Url::parse(location) {
        Ok(url) => url,
        Err(_) => return Ok(location.to_string()),
    };

    // Sign URLs the proxy needs to keep authenticating itself: both
    // GAR's `/artifacts-uploads/...` (separate handler at
    // `proxy_upload_session`) and OCI's `/v2/{repo}/blobs/uploads/{id}`
    // (handled inline in `proxy_push` via the signed-URL bypass).
    // Anything else passes through unchanged.
    if !url.path().starts_with("/artifacts-uploads/") && !is_oci_upload_session_path(url.path()) {
        return Ok(location.to_string());
    }

    let repo_name = upload_session_repo.ok_or_else(|| {
        oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session authorization context is missing",
        )
    })?;

    if signing_key.is_empty() {
        return Err(oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session signing key is not configured",
        ));
    }

    let expires_at = chrono::Utc::now().timestamp() + UPLOAD_SESSION_TTL_SECONDS;
    let signature = sign_upload_session(signing_key, url.path(), repo_name, expires_at);

    url.query_pairs_mut()
        .append_pair(UPLOAD_SESSION_VERSION_PARAM, UPLOAD_SESSION_VERSION)
        .append_pair(UPLOAD_SESSION_REPO_PARAM, repo_name)
        .append_pair(UPLOAD_SESSION_EXPIRES_PARAM, &expires_at.to_string())
        .append_pair(UPLOAD_SESSION_SIGNATURE_PARAM, &signature);

    Ok(url.to_string())
}

fn verify_upload_session_auth(
    signing_key: &[u8],
    upload_path: &str,
    query: &HashMap<String, String>,
) -> Result<String, Response> {
    if signing_key.is_empty() {
        return Err(oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "Registry upload session signing key is not configured",
        ));
    }

    let Some(version) = query.get(UPLOAD_SESSION_VERSION_PARAM) else {
        return Err(invalid_upload_session_auth());
    };
    if version != UPLOAD_SESSION_VERSION {
        return Err(invalid_upload_session_auth());
    }

    let repo_name = query
        .get(UPLOAD_SESSION_REPO_PARAM)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_upload_session_auth)?;
    let expires_at = query
        .get(UPLOAD_SESSION_EXPIRES_PARAM)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(invalid_upload_session_auth)?;
    let signature = query
        .get(UPLOAD_SESSION_SIGNATURE_PARAM)
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid_upload_session_auth)?;

    if expires_at < chrono::Utc::now().timestamp() {
        return Err(invalid_upload_session_auth());
    }

    if !verify_upload_session_signature(signing_key, upload_path, repo_name, expires_at, signature)
    {
        return Err(invalid_upload_session_auth());
    }

    Ok(repo_name.clone())
}

fn invalid_upload_session_auth() -> Response {
    oci_error(
        StatusCode::FORBIDDEN,
        "DENIED",
        "Invalid registry upload session authorization.",
    )
}

fn strip_upload_session_auth_params(query: &HashMap<String, String>) -> HashMap<String, String> {
    query
        .iter()
        .filter(|(key, _)| !is_upload_session_auth_param(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn is_upload_session_auth_param(key: &str) -> bool {
    matches!(
        key,
        UPLOAD_SESSION_VERSION_PARAM
            | UPLOAD_SESSION_REPO_PARAM
            | UPLOAD_SESSION_EXPIRES_PARAM
            | UPLOAD_SESSION_SIGNATURE_PARAM
    )
}

fn sign_upload_session(
    signing_key: &[u8],
    upload_path: &str,
    repo_name: &str,
    expires_at: i64,
) -> String {
    let upload_signing_key = derive_upload_session_signing_key(signing_key);
    let mut mac = HmacSha256::new_from_slice(&upload_signing_key).expect("HMAC accepts any key");
    mac.update(upload_session_payload(upload_path, repo_name, expires_at).as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn verify_upload_session_signature(
    signing_key: &[u8],
    upload_path: &str,
    repo_name: &str,
    expires_at: i64,
    signature: &str,
) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };

    let upload_signing_key = derive_upload_session_signing_key(signing_key);
    let mut mac = HmacSha256::new_from_slice(&upload_signing_key).expect("HMAC accepts any key");
    mac.update(upload_session_payload(upload_path, repo_name, expires_at).as_bytes());
    mac.verify_slice(&signature).is_ok()
}

fn derive_upload_session_signing_key(signing_key: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(signing_key).expect("HMAC accepts any key");
    mac.update(UPLOAD_SESSION_SIGNING_CONTEXT);
    mac.finalize().into_bytes().to_vec()
}

fn upload_session_payload(upload_path: &str, repo_name: &str, expires_at: i64) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        UPLOAD_SESSION_VERSION, upload_path, repo_name, expires_at
    )
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
fn require_push_auth(state: &AppState, subject: &Subject, repo_name: &str) -> Result<(), Response> {
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

/// A project-scoped capability shares the scope whose pulls skip repo validation below, so it is
/// refused before that match.
fn refuse_capability_pull(subject: &Subject) -> Result<(), Response> {
    if subject.role == Role::ImageRepositoryProvisioner {
        return Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            "Image repository provisioning credentials cannot pull images",
        ));
    }
    if subject.role == Role::SandboxImagePusher {
        return Err(oci_error(
            StatusCode::FORBIDDEN,
            "DENIED",
            "Sandbox image push credentials cannot pull images",
        ));
    }
    Ok(())
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
    refuse_capability_pull(subject)?;
    let (deployment_id, own_project) = match &subject.scope {
        Scope::Workspace | Scope::Project { .. } => return Ok(()),
        Scope::DeploymentGroup { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Registry proxy pulls require a deployment token",
            ))
        }
        Scope::Commands { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Command credentials cannot access the registry proxy",
            ))
        }
        Scope::RemoteBindings { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Remote bindings credentials cannot access the registry proxy",
            ))
        }
        Scope::Telemetry { .. } => {
            return Err(oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "Telemetry credentials cannot access the registry proxy",
            ))
        }
        Scope::Deployment {
            project_id,
            deployment_id,
        } => {
            // A deployment token may always pull from its own project's
            // artifact repository. Source-built resources (Worker/Container/
            // Daemon) publish their images there under `{prefix}-{project_id}`,
            // and those repos are not discoverable from the stack's `image`
            // fields — so the per-release allow-list below would otherwise
            // reject them (e.g. a source-built Daemon).
            if state.registry_routing_table.project_id_for_repo(repo_name)
                == Some(project_id.as_str())
            {
                return Ok(());
            }
            (deployment_id.as_str(), project_id.as_str())
        }
    };

    // Check cache first.
    let repo_names = if let Some((_release_id, cached_repos)) =
        state.pull_validation_cache.get(deployment_id)
    {
        cached_repos
    } else {
        // Cache miss — query DB.
        // The caller has already been authenticated and scoped to this
        // deployment. Use that subject for the deployment lookup so platform
        // managers can hydrate pull deployments through their pull sync path.
        let system = crate::auth::Subject::system();
        let deployment = state
            .deployment_store
            .get_deployment(subject, deployment_id)
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

        let proxy_host = state.config.base_url();
        let proxy_host = alien_core::image_rewrite::strip_url_scheme(&proxy_host);
        let routes = &state.registry_routing_table;
        let own_repo = |repo: &str| sandbox_repo_in_own_project(routes, repo, own_project);
        let repos = release
            .stacks
            .values()
            .flat_map(|stack| extract_repo_names(stack, proxy_host, &own_repo))
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

/// Whether a Sandbox's image repo may enter the release's list: only one in the deployment's own
/// project, so a Sandbox naming another project's repo cannot open it to this deployment's token.
/// An unattributable repo counts as "default", as a push to it does.
fn sandbox_repo_in_own_project(
    routes: &RegistryRoutingTable,
    repo: &str,
    own_project: &str,
) -> bool {
    routes.project_id_for_repo(repo).unwrap_or("default") == own_project
}

/// Extract the set of repo names from a release's stack. `proxy_host` is this manager's own
/// registry host, the only one a sandbox image is pulled through, and `sandbox_repo_allowed`
/// filters the repos a sandbox image may add.
fn extract_repo_names(
    stack: &alien_core::Stack,
    proxy_host: &str,
    sandbox_repo_allowed: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    use alien_core::image_rewrite::strip_registry_host;
    use alien_core::{
        classify_azure_sandbox_image, AzureSandboxImage, Container, ContainerCode, Daemon,
        DaemonCode, Sandbox, SandboxCode, Worker, WorkerCode,
    };

    let mut repos = Vec::new();

    for (_resource_id, entry) in stack.resources() {
        let mut from_sandbox = false;
        let image = if let Some(func) = entry.config.downcast_ref::<Worker>() {
            match &func.code {
                WorkerCode::Image { image } => Some(image.as_str()),
                WorkerCode::Source { .. } => None,
            }
        } else if let Some(container) = entry.config.downcast_ref::<Container>() {
            match &container.code {
                ContainerCode::Image { image } => Some(image.as_str()),
                ContainerCode::Source { .. } => None,
            }
        } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            match &daemon.code {
                DaemonCode::Image { image } => Some(image.as_str()),
                DaemonCode::Source { .. } => None,
            }
        } else if let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() {
            // Only a registry image on this host, the rule the controller sends credentials by: a
            // public image, catalog name or `s3://` bundle is never pulled here.
            match &sandbox.code {
                SandboxCode::Image { image } => match classify_azure_sandbox_image(image) {
                    Some(AzureSandboxImage::Registry(reference))
                        if reference
                            .split_once('/')
                            .is_some_and(|(host, _)| host.eq_ignore_ascii_case(proxy_host)) =>
                    {
                        from_sandbox = true;
                        Some(reference)
                    }
                    _ => None,
                },
                SandboxCode::Source { .. } => None,
            }
        } else {
            None
        };

        if let Some(image_uri) = image {
            if let Some(stripped) = strip_registry_host(image_uri) {
                let repo = stripped.split(':').next().unwrap_or(&stripped);
                let repo = repo.split('@').next().unwrap_or(repo);
                if from_sandbox && !sandbox_repo_allowed(repo) {
                    continue;
                }
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

/// A `/v2/` request path split into a repository name that matches the OCI
/// distribution `<name>` grammar and one known operation suffix.
struct OciPath {
    repo: String,
    rest: String,
}

impl OciPath {
    fn as_path(&self) -> String {
        format!("{}/{}", self.repo, self.rest)
    }

    /// `blobs/uploads/{id}`: an upload session, not the upload-init `blobs/uploads/`.
    fn is_upload_session(&self) -> bool {
        self.rest.starts_with("blobs/uploads/") && self.rest != "blobs/uploads/"
    }
}

/// Names, tags, digests and session ids must match the OCI distribution grammar; the upstream
/// URL is built from the parsed parts. The operation is taken from the end of the path, as a
/// distribution registry routes it, so a name may contain a component such as `tags`.
fn parse_oci_path(path: &str) -> Result<OciPath, Response> {
    let segments: Vec<&str> = path.split('/').collect();
    let n = segments.len();
    let op = if n >= 4 && segments[n - 3..n - 1] == ["blobs", "uploads"] {
        n - 3
    } else if n >= 3
        && matches!(
            segments[n - 2],
            "manifests" | "blobs" | "uploads" | "tags" | "referrers"
        )
    {
        n - 2
    } else {
        return Err(invalid_repository_name());
    };
    let repo = segments[..op].join("/");
    if !is_valid_repository_name(&repo) {
        return Err(invalid_repository_name());
    }
    let refusal = match &segments[op..] {
        ["manifests", reference] if is_valid_tag(reference) || is_valid_digest(reference) => None,
        ["manifests", reference] if reference.contains(':') => {
            Some(("DIGEST_INVALID", "Invalid digest"))
        }
        ["manifests", _] => Some(("MANIFEST_INVALID", "Invalid manifest reference")),
        ["blobs", "uploads", ""] | ["tags", "list"] => None,
        // Some registries issue `/v2/<name>/uploads/<id>` session URLs.
        ["blobs", "uploads", session] | ["uploads", session]
            if is_valid_upload_session_id(session) =>
        {
            None
        }
        ["blobs", "uploads", _] | ["uploads", _] => {
            Some(("BLOB_UPLOAD_INVALID", "Invalid upload session id"))
        }
        ["blobs", digest] | ["referrers", digest] if is_valid_digest(digest) => None,
        ["blobs", _] | ["referrers", _] => Some(("DIGEST_INVALID", "Invalid digest")),
        _ => Some(("NAME_INVALID", "Invalid repository name or operation")),
    };
    if let Some((code, message)) = refusal {
        return Err(oci_error(StatusCode::BAD_REQUEST, code, message));
    }
    Ok(OciPath {
        repo,
        rest: segments[op..].join("/"),
    })
}

fn invalid_repository_name() -> Response {
    oci_error(
        StatusCode::BAD_REQUEST,
        "NAME_INVALID",
        "Invalid repository name",
    )
}

/// OCI `<name>`: components `[a-z0-9]+(?:(?:[._]|__|[-]*)[a-z0-9]+)*` joined by `/`.
fn is_valid_repository_name(name: &str) -> bool {
    !name.is_empty() && name.split('/').all(is_valid_name_component)
}

fn is_valid_name_component(component: &str) -> bool {
    let bytes = component.as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !bytes.first().is_some_and(alnum) || !bytes.last().is_some_and(alnum) {
        return false;
    }
    let mut rest = bytes;
    while !rest.is_empty() {
        let separator_len = rest.iter().take_while(|b| !alnum(b)).count();
        let (separator, tail) = rest.split_at(separator_len);
        let separator_ok =
            matches!(separator, b"" | b"." | b"_" | b"__") || separator.iter().all(|b| *b == b'-');
        if !separator_ok {
            return false;
        }
        let run = tail.iter().take_while(|b| alnum(b)).count();
        rest = &tail[run..];
    }
    true
}

/// OCI tag: `[A-Za-z0-9_][A-Za-z0-9._-]{0,127}`.
fn is_valid_tag(tag: &str) -> bool {
    let bytes = tag.as_bytes();
    bytes.len() <= 128
        && bytes
            .first()
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// OCI digest: `[a-z0-9]+(?:[+._-][a-z0-9]+)*:[a-zA-Z0-9=_-]+`.
fn is_valid_digest(digest: &str) -> bool {
    let Some((algorithm, encoded)) = digest.split_once(':') else {
        return false;
    };
    let algorithm_ok = algorithm.split(['+', '.', '_', '-']).all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    algorithm_ok
        && !encoded.is_empty()
        && encoded
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'=' | b'_' | b'-'))
}

/// Upload session ids are opaque upstream tokens, so any single segment but a dot segment is
/// accepted; `require_literal_oci_path` has already refused separators and `%`.
fn is_valid_upload_session_id(id: &str) -> bool {
    !matches!(id, "" | "." | "..")
}

/// The `/artifacts-uploads/` path is forwarded raw, so check each segment in
/// decoded form: the upstream id alphabet is unknown, but no segment may
/// collapse or split into another path.
fn is_safe_upload_session_path(raw_path: &str) -> bool {
    raw_path
        .trim_start_matches('/')
        .split('/')
        .all(|segment| match urlencoding::decode(segment) {
            Ok(decoded) => {
                !matches!(decoded.as_ref(), "" | "." | "..")
                    && !decoded.contains(['/', '\\', '?', '#'])
            }
            Err(_) => false,
        })
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
mod tests {
    use super::*;
    use alien_core::image_rewrite::strip_registry_host;
    use alien_core::{
        Daemon, DaemonCode, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
        SandboxLifecyclePolicy, Stack,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_image_capability_roles_cannot_pull() {
        let subject = |role| Subject {
            kind: crate::auth::SubjectKind::ServiceAccount {
                id: "platform".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Project {
                project_id: "default".to_string(),
            },
            role,
            bearer_token: String::new(),
        };

        for role in [Role::ImageRepositoryProvisioner, Role::SandboxImagePusher] {
            let refused = refuse_capability_pull(&subject(role))
                .expect_err("a capability role must not reach the project-scope pull bypass");
            assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        }
        assert!(refuse_capability_pull(&subject(Role::ProjectDeveloper)).is_ok());
    }

    #[test]
    fn only_a_literal_registry_path_is_forwarded() {
        for path in [
            "artifacts/prj_a/manifests/v1",
            "/artifacts/prj_a/blobs/sha256:abc",
            "artifacts/prj_a/blobs/uploads/",
            "artifacts/prj_a/blobs/uploads/4f1c-9e_2=",
            "artifacts/prj_a/tags/list",
        ] {
            assert!(require_literal_oci_path(path).is_ok(), "{path}");
        }
        for path in [
            "artifacts/prj_a/blobs/uploads/?mount=sha256:abc&from=artifacts/prj_b",
            "artifacts/prj_a/manifests/../../prj_b/manifests/v1",
            "artifacts/prj_a/manifests/..\\..\\prj_b/manifests/v1",
            "artifacts/prj_a/manifests/%2e%2e/%2e%2e/prj_b/manifests/v1",
            "artifacts/prj_a/manifests/./v1",
            "artifacts/prj_a/manifests/v1#frag",
            "artifacts//prj_b/manifests/v1",
            "artifacts/prj_a/blobs/uploads//",
        ] {
            let refused = require_literal_oci_path(path).expect_err(path);
            assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{path}");
        }
    }

    fn parsed(path: &str) -> (String, String) {
        let oci = parse_oci_path(path).unwrap_or_else(|_| panic!("{path} should parse"));
        assert_eq!(oci.as_path(), path, "the upstream path is the request path");
        (oci.repo, oci.rest)
    }

    #[test]
    fn parse_oci_path_accepts_the_names_providers_compose() {
        let digest = format!("sha256:{}", "a".repeat(64));
        // ECR (`{prefix}-{project}`), Local/ACR (`{prefix}/{project}`), GAR (`{gcp}/{repo}/{project}`).
        for repo in [
            "alien-prj_abc123",
            "artifacts/default-prj_a",
            "my-project/alien-repo/alien-prj-123",
            "a__b/c.d/e---f",
            "artifacts/default-prj_a/tags",
            "artifacts/default-prj_a/blobs/uploads",
            "artifacts/manifests/referrers",
        ] {
            for rest in [
                "manifests/v1".to_string(),
                "manifests/Latest_1.2-rc".to_string(),
                format!("manifests/{digest}"),
                format!("blobs/{digest}"),
                "blobs/uploads/".to_string(),
                "blobs/uploads/9f0b2f0f-3a1c-4e2b-8b1e-0c7d7f7e1a2b".to_string(),
                "uploads/9f0b2f0f-3a1c-4e2b-8b1e-0c7d7f7e1a2b".to_string(),
                "tags/list".to_string(),
                format!("referrers/{digest}"),
            ] {
                let path = format!("{repo}/{rest}");
                assert_eq!(parsed(&path), (repo.to_string(), rest));
            }
        }
    }

    /// Repository names in the shapes the artifact registry bindings compose, with the references
    /// and session ids push and pull clients send for them.
    #[test]
    fn parse_oci_path_accepts_the_repository_names_and_references_clients_send() {
        let project = "prj_0a1b2c3d4e5f6g7h8i9j0k1l2m3n";
        let repos = [
            // ECR `{prefix}-{project}`, and the prefix alone.
            format!("alien-artifacts-{project}"),
            format!("alien/agents-{project}"),
            format!("e2e-21-artifact-registry-{project}"),
            "alien-e2e".to_string(),
            // GAR `{gcp-project}/{repository}/{project}`.
            format!("my-gcp-project-1/alien-artifacts/{project}"),
            format!("a1b2c3d4-artifact-registry/{project}"),
            "alien-test-mgmt/alien-e2e/default".to_string(),
            // ACR with no prefix, and the prefix alone.
            project.to_string(),
            "azure-e2e".to_string(),
            // Local `{prefix}/{project}` and the embedded registry's two-segment names.
            format!("artifacts/{project}"),
            "artifacts/default".to_string(),
            "artifacts/sandbox-image".to_string(),
            // Nested names under a project.
            format!("artifacts/{project}/api/worker.v2"),
        ];
        let sha256 = format!("sha256:{}", "0123456789abcdef".repeat(4));
        let sha512 = format!("sha512:{}", "0123456789abcdef".repeat(8));
        let rests = [
            // Tags `alien-build` and the CLI generate, and common hand-written ones.
            "manifests/api-3k9x2m1q".to_string(),
            "manifests/remote-sandbox-src-0123456789abcdef".to_string(),
            "manifests/sandbox-v1".to_string(),
            "manifests/v1.2.3".to_string(),
            "manifests/latest".to_string(),
            "manifests/_internal".to_string(),
            format!("manifests/{}", "t".repeat(128)),
            format!("manifests/{sha256}"),
            format!("manifests/{sha512}"),
            format!("blobs/{sha256}"),
            format!("blobs/{sha512}"),
            format!("referrers/{sha256}"),
            "tags/list".to_string(),
            "blobs/uploads/".to_string(),
            // Session ids are opaque: UUIDs, base64url with padding, and other literal segments.
            "blobs/uploads/00000000-0000-4000-8000-000000000001".to_string(),
            "blobs/uploads/AJ-x_y0Zq==".to_string(),
            "blobs/uploads/Zm9v,YmFy;(1)!".to_string(),
            "uploads/9f0b2f0f-3a1c-4e2b-8b1e-0c7d7f7e1a2b".to_string(),
        ];
        for repo in &repos {
            for rest in &rests {
                let path = format!("{repo}/{rest}");
                assert_eq!(parsed(&path), (repo.clone(), rest.clone()));
                assert!(require_literal_oci_path(&path).is_ok(), "{path}");
            }
        }
    }

    #[tokio::test]
    async fn parse_oci_path_refuses_paths_outside_the_grammar_with_the_part_that_failed() {
        let digest = format!("sha256:{}", "a".repeat(64));
        for (path, code) in [
            ("artifacts/default-prj_a/../x/manifests/v1", "NAME_INVALID"),
            ("artifacts/default-prj_a/./manifests/v1", "NAME_INVALID"),
            ("artifacts//default-prj_a/manifests/v1", "NAME_INVALID"),
            ("/artifacts/default-prj_a/manifests/v1", "NAME_INVALID"),
            ("artifacts/Default-prj_a/manifests/v1", "NAME_INVALID"),
            // The router decodes `%XX` before this parser, so a `%` left here is literal.
            ("artifacts/default-prj_a%2e/manifests/v1", "NAME_INVALID"),
            ("artifacts/-prj/manifests/v1", "NAME_INVALID"),
            ("artifacts/prj_/manifests/v1", "NAME_INVALID"),
            ("artifacts/a___b/manifests/v1", "NAME_INVALID"),
            ("artifacts/a._b/manifests/v1", "NAME_INVALID"),
            ("manifests/v1", "NAME_INVALID"),
            ("_catalog", "NAME_INVALID"),
            ("artifacts/default-prj_a", "NAME_INVALID"),
            ("artifacts/default-prj_a/manifests/v1/../v2", "NAME_INVALID"),
            ("artifacts/default-prj_a/tags/other", "NAME_INVALID"),
            ("artifacts/default-prj_a/manifests/..", "MANIFEST_INVALID"),
            ("artifacts/default-prj_a/manifests/-v1", "MANIFEST_INVALID"),
            (
                "artifacts/default-prj_a/manifests/sha256:",
                "DIGEST_INVALID",
            ),
            ("artifacts/default-prj_a/manifests/v1?x", "MANIFEST_INVALID"),
            ("artifacts/default-prj_a/blobs/sha256", "DIGEST_INVALID"),
            ("artifacts/default-prj_a/blobs/SHA256:abc", "DIGEST_INVALID"),
            ("artifacts/default-prj_a/blobs/uploads", "DIGEST_INVALID"),
            (
                &*format!("artifacts/default-prj_a/referrers/{digest}x!"),
                "DIGEST_INVALID",
            ),
            (
                "artifacts/default-prj_a/blobs/uploads/..",
                "BLOB_UPLOAD_INVALID",
            ),
            ("artifacts/default-prj_a/uploads/", "BLOB_UPLOAD_INVALID"),
            ("artifacts/default-prj_a/uploads/..", "BLOB_UPLOAD_INVALID"),
            (
                "artifacts/default-prj_a/blobs/uploads/.",
                "BLOB_UPLOAD_INVALID",
            ),
            ("artifacts/default-prj_a/uploads/.", "BLOB_UPLOAD_INVALID"),
            ("artifacts/default-prj_a/blobs/uploads/a/b", "NAME_INVALID"),
        ] {
            let response = parse_oci_path(path)
                .err()
                .unwrap_or_else(|| panic!("{path} should be refused"));
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["errors"][0]["code"], code, "{path}");
        }
        assert!(!is_valid_tag(&"a".repeat(129)));
        assert!(is_valid_tag(&"a".repeat(128)));
    }

    #[test]
    fn mount_sources_must_be_valid_names_on_the_target_route() {
        let table = RegistryRoutingTable::new(vec![
            registry_route("alien", Platform::Aws, "aws"),
            registry_route("artifacts/default", Platform::Local, "local"),
        ])
        .unwrap();
        let target = "alien-prj_a";
        assert!(is_mount_source_on_target_route(
            &table,
            "alien-prj_a/base",
            target
        ));
        assert!(is_mount_source_on_target_route(
            &table,
            "alien-prj_b",
            target
        ));
        // Same project id, resolved on another route.
        assert!(!is_mount_source_on_target_route(
            &table,
            "artifacts/default-prj_a",
            target
        ));
        assert!(!is_mount_source_on_target_route(
            &table,
            "other/prj_a",
            target
        ));
        assert!(!is_mount_source_on_target_route(
            &table,
            "alien-prj_a/../x",
            target
        ));
        assert!(!is_mount_source_on_target_route(
            &table,
            "Alien-prj_a",
            target
        ));
        assert!(!is_mount_source_on_target_route(
            &table,
            "alien-prj_a",
            "other/prj_a"
        ));
        // With no routing table there is one registry, so any valid name shares its route.
        let single = RegistryRoutingTable::new(vec![]).unwrap();
        assert!(is_mount_source_on_target_route(
            &single,
            "artifacts/prj_b",
            "artifacts/prj_a"
        ));
        assert!(!is_mount_source_on_target_route(
            &single,
            "Artifacts/prj_b",
            "artifacts/prj_a"
        ));
    }

    #[test]
    fn signed_sessions_stay_on_one_registry_route() {
        let table = RegistryRoutingTable::new(vec![
            registry_route("alien", Platform::Aws, "aws"),
            registry_route("my-gcp/alien-repo", Platform::Gcp, "gcp"),
        ])
        .unwrap();
        assert!(same_route(&table, "alien-prj_a", "alien-prj_b"));
        assert!(same_route(
            &table,
            "my-gcp/alien-repo/prj_a",
            "my-gcp/alien-repo/pkg"
        ));
        assert!(!same_route(
            &table,
            "alien-prj_a",
            "my-gcp/alien-repo/prj_a"
        ));
        assert!(!same_route(&table, "elsewhere/x", "other/y"));
        let single = RegistryRoutingTable::new(vec![]).unwrap();
        assert!(same_route(&single, "elsewhere/x", "other/y"));
    }

    #[test]
    fn upload_session_paths_refuse_dot_and_encoded_separator_segments() {
        assert!(is_safe_upload_session_path(
            "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/AJ-x_y=="
        ));
        for path in [
            "/artifacts-uploads/namespaces/p/repositories/r/../s/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r/%2e%2E/s/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r/.%2e/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r/./uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r/%2e/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r%2fs/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r%5cs/uploads/1",
            "/artifacts-uploads/namespaces/p//uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r%3fs/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r%23s/uploads/1",
            "/artifacts-uploads/namespaces/p/repositories/r%c0%ae/uploads/1",
        ] {
            assert!(!is_safe_upload_session_path(path), "{path}");
        }
    }

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
    fn extract_repo_names_includes_daemon_image_resources() {
        let daemon = Daemon::new("host-loader".to_string())
            .code(DaemonCode::Image {
                image: "manager.example.com/artifacts/prj_test:host-loader-v1".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(daemon, ResourceLifecycle::Live)
            .build();

        assert_eq!(
            extract_repo_names(&stack, "manager.example.com", &|_| true),
            vec!["artifacts/prj_test".to_string()]
        );
    }

    /// A sandbox image on this host is pulled through the proxy, so its repo is in the release; a
    /// public image elsewhere, a catalog name or an `s3://` bundle is never pulled here.
    #[test]
    fn extract_repo_names_includes_sandbox_registry_images_only() {
        let sandbox = |id: &str, image: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                })
                .egress(SandboxEgress::Allow)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        let stack = Stack::new("test-stack".to_string())
            .add(
                sandbox(
                    "agents",
                    "manager.example.com/artifacts/prj_test:sandbox-v1",
                ),
                ResourceLifecycle::Frozen,
            )
            .add(sandbox("catalog", "ubuntu"), ResourceLifecycle::Frozen)
            .add(
                sandbox("public", "docker.io/library/python:3.14-slim"),
                ResourceLifecycle::Frozen,
            )
            .add(
                sandbox("bundle", "s3://bucket/sandbox/bundle.zip"),
                ResourceLifecycle::Frozen,
            )
            .build();

        assert_eq!(
            extract_repo_names(&stack, "manager.example.com", &|_| true),
            vec!["artifacts/prj_test".to_string()]
        );
    }

    /// A sandbox image in another project's repository is left out of the release's list.
    #[test]
    fn extract_repo_names_leaves_out_a_sandbox_image_of_another_project() {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "manager.example.com/artifacts/prj_other:sandbox-v1".to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(sandbox, ResourceLifecycle::Frozen)
            .build();

        assert!(extract_repo_names(&stack, "manager.example.com", &|repo| {
            repo != "artifacts/prj_other"
        })
        .is_empty());
    }

    #[test]
    fn a_sandbox_repo_counts_only_in_its_own_project() {
        let routes =
            RegistryRoutingTable::new(vec![registry_route("artifacts", Platform::Aws, "aws")])
                .unwrap();

        assert!(sandbox_repo_in_own_project(
            &routes,
            "artifacts/prj_a",
            "prj_a"
        ));
        assert!(!sandbox_repo_in_own_project(
            &routes,
            "artifacts/prj_b",
            "prj_a"
        ));
        assert!(!sandbox_repo_in_own_project(&routes, "elsewhere", "prj_a"));
        assert!(sandbox_repo_in_own_project(&routes, "elsewhere", "default"));
    }

    #[test]
    fn proxy_base_url_prefers_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:8080".parse().unwrap());
        headers.insert("x-forwarded-host", "manager.example.com".parse().unwrap());
        headers.insert("x-forwarded-proto", "https".parse().unwrap());

        assert_eq!(
            proxy_base_url(&headers, "http://localhost:8080"),
            "https://manager.example.com"
        );
    }

    #[test]
    fn proxy_base_url_uses_request_host_for_public_requests() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "alien-manager.example.com".parse().unwrap());

        assert_eq!(
            proxy_base_url(&headers, "http://localhost:8080"),
            "https://alien-manager.example.com"
        );
    }

    #[test]
    fn proxy_base_url_keeps_localhost_http() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "localhost:8080".parse().unwrap());

        assert_eq!(
            proxy_base_url(&headers, "http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[tokio::test]
    async fn credential_cache_serializes_generation_for_same_key() {
        let cache = Arc::new(CredentialCache::new());
        let generation_count = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..16 {
            let cache = cache.clone();
            let generation_count = generation_count.clone();
            tasks.push(tokio::spawn(async move {
                let key = "https://ecr.example.test:alien-e2e:PushPull";
                if cache.get(key).is_none() {
                    let generation_lock = cache.generation_lock(key);
                    let _guard = generation_lock.lock().await;
                    if cache.get(key).is_none() {
                        generation_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        cache.insert(
                            key.to_string(),
                            ArtifactRegistryCredentials {
                                auth_method: alien_bindings::traits::RegistryAuthMethod::Basic,
                                username: "AWS".to_string(),
                                password: "token".to_string(),
                                expires_at: None,
                            },
                            Duration::from_secs(300),
                        );
                    }
                }
            }));
        }

        for task in tasks {
            task.await.expect("credential cache task should complete");
        }

        assert_eq!(generation_count.load(Ordering::SeqCst), 1);
        assert!(cache
            .get("https://ecr.example.test:alien-e2e:PushPull")
            .is_some());
    }

    // -----------------------------------------------------------------------
    // RegistryRoutingTable
    // -----------------------------------------------------------------------

    #[derive(Debug)]
    struct RegistryRouteTestProvider;

    fn registry_route(prefix: &str, platform: Platform, binding_name: &str) -> RegistryRoute {
        RegistryRoute {
            prefix: prefix.to_string(),
            platform,
            provider: Arc::new(RegistryRouteTestProvider),
            binding_name: binding_name.to_string(),
        }
    }

    fn route_test_error(binding_name: &str) -> alien_bindings::error::Error {
        alien_error::AlienError::new(alien_bindings::error::ErrorData::config_invalid(
            binding_name,
            "test provider has no bindings",
        ))
    }

    #[async_trait::async_trait]
    impl BindingsProviderApi for RegistryRouteTestProvider {
        async fn load_storage(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Storage>> {
            Err(route_test_error(binding_name))
        }

        async fn load_build(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Build>> {
            Err(route_test_error(binding_name))
        }

        async fn load_artifact_registry(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn ArtifactRegistry>> {
            Err(route_test_error(binding_name))
        }

        async fn load_vault(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Vault>> {
            Err(route_test_error(binding_name))
        }

        async fn load_kv(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Kv>> {
            Err(route_test_error(binding_name))
        }

        async fn load_postgres(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Postgres>> {
            Err(route_test_error(binding_name))
        }

        async fn load_queue(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Queue>> {
            Err(route_test_error(binding_name))
        }

        async fn load_worker(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Worker>> {
            Err(route_test_error(binding_name))
        }

        async fn load_container(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Container>> {
            Err(route_test_error(binding_name))
        }

        async fn load_service_account(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::ServiceAccount>>
        {
            Err(route_test_error(binding_name))
        }

        async fn load_sandbox(
            &self,
            binding_name: &str,
        ) -> alien_bindings::error::Result<Arc<dyn alien_bindings::traits::Sandbox>> {
            Err(route_test_error(binding_name))
        }
    }

    #[test]
    fn registry_routing_table_specific_prefix_beats_catch_all_regardless_registration_order() {
        for routes in [
            vec![
                registry_route("", Platform::Local, "local"),
                registry_route("artifacts", Platform::Aws, "aws"),
            ],
            vec![
                registry_route("artifacts", Platform::Aws, "aws"),
                registry_route("", Platform::Local, "local"),
            ],
        ] {
            let table =
                RegistryRoutingTable::new(routes).expect("routing table should be unambiguous");
            let route = table
                .resolve("artifacts/prj_test")
                .expect("specific route should match");

            assert_eq!(route.platform, Platform::Aws);
        }
    }

    #[test]
    fn registry_routing_table_nested_prefixes_pick_longest_match() {
        let table = RegistryRoutingTable::new(vec![
            registry_route("artifacts", Platform::Aws, "aws"),
            registry_route("artifacts/team-a", Platform::Gcp, "gcp"),
            registry_route("", Platform::Local, "local"),
        ])
        .expect("nested prefixes should be valid");
        let route = table
            .resolve("artifacts/team-a/prj_test")
            .expect("nested route should match");

        assert_eq!(route.platform, Platform::Gcp);
    }

    #[test]
    fn registry_routing_table_rejects_duplicate_prefixes_at_construction() {
        let result = RegistryRoutingTable::new(vec![
            registry_route("", Platform::Local, "local"),
            registry_route("", Platform::Azure, "azure"),
        ]);
        let Err(error) = result else {
            panic!("duplicate catch-all prefixes should fail");
        };

        assert!(error.contains("Duplicate artifact registry prefix '<empty>'"));
    }

    #[test]
    fn registry_routing_table_rejects_duplicate_non_empty_prefixes_at_construction() {
        let result = RegistryRoutingTable::new(vec![
            registry_route("artifacts", Platform::Aws, "aws"),
            registry_route("artifacts", Platform::Gcp, "gcp"),
        ]);
        let Err(error) = result else {
            panic!("duplicate non-empty prefixes should fail");
        };

        assert!(error.contains("Duplicate artifact registry prefix 'artifacts'"));
    }

    // -----------------------------------------------------------------------
    // project_id_after_prefix — the algorithm behind
    // RegistryRoutingTable::project_id_for_repo. Tests target the free
    // helper so separator handling is tested independently from routing.
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
            project_id_after_prefix("artifacts/default/prj_xxx/release-v1", "artifacts/default",),
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

    #[test]
    fn gar_upload_session_location_gets_signed_repo_context() {
        let signing_key = b"test-registry-upload-session-key";
        let repo_name = "cloud-project/artifacts/prj_123";
        let location = "https://manager.example.com/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1?digest=sha256:abc";

        let rewritten =
            rewrite_location_with_upload_session_auth(location, Some(repo_name), signing_key)
                .expect("GAR upload session location should be signed");
        let url = Url::parse(&rewritten).expect("rewritten location should be a URL");
        let query = url
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<HashMap<_, _>>();

        assert_eq!(query.get("digest").map(String::as_str), Some("sha256:abc"));
        assert_eq!(
            query.get(UPLOAD_SESSION_REPO_PARAM).map(String::as_str),
            Some(repo_name)
        );
        assert!(verify_upload_session_auth(signing_key, url.path(), &query).is_ok());

        let upstream_query = strip_upload_session_auth_params(&query);
        assert_eq!(upstream_query.len(), 1);
        assert_eq!(
            upstream_query.get("digest").map(String::as_str),
            Some("sha256:abc")
        );
    }

    #[test]
    fn gar_upload_session_auth_rejects_tampering() {
        let signing_key = b"test-registry-upload-session-key";
        let path =
            "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1";
        let repo_name = "cloud-project/artifacts/prj_123";
        let expires_at = chrono::Utc::now().timestamp() + UPLOAD_SESSION_TTL_SECONDS;
        let signature = sign_upload_session(signing_key, path, repo_name, expires_at);

        let mut query = HashMap::from([
            (
                UPLOAD_SESSION_VERSION_PARAM.to_string(),
                UPLOAD_SESSION_VERSION.to_string(),
            ),
            (UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string()),
            (
                UPLOAD_SESSION_EXPIRES_PARAM.to_string(),
                expires_at.to_string(),
            ),
            (UPLOAD_SESSION_SIGNATURE_PARAM.to_string(), signature),
        ]);

        assert!(verify_upload_session_auth(signing_key, path, &query).is_ok());

        query.insert(
            UPLOAD_SESSION_REPO_PARAM.to_string(),
            "cloud-project/artifacts/prj_other".to_string(),
        );
        assert!(verify_upload_session_auth(signing_key, path, &query).is_err());

        query.insert(UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string());
        assert!(verify_upload_session_auth(
            signing_key,
            "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/other-session",
            &query,
        )
        .is_err());
    }

    #[test]
    fn gar_upload_session_auth_rejects_expired_token() {
        let signing_key = b"test-registry-upload-session-key";
        let path =
            "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1";
        let repo_name = "cloud-project/artifacts/prj_123";
        let expires_at = chrono::Utc::now().timestamp() - 1;
        let signature = sign_upload_session(signing_key, path, repo_name, expires_at);
        let query = HashMap::from([
            (
                UPLOAD_SESSION_VERSION_PARAM.to_string(),
                UPLOAD_SESSION_VERSION.to_string(),
            ),
            (UPLOAD_SESSION_REPO_PARAM.to_string(), repo_name.to_string()),
            (
                UPLOAD_SESSION_EXPIRES_PARAM.to_string(),
                expires_at.to_string(),
            ),
            (UPLOAD_SESSION_SIGNATURE_PARAM.to_string(), signature),
        ]);

        assert!(verify_upload_session_auth(signing_key, path, &query).is_err());
    }

    #[test]
    fn oci_upload_session_location_gets_signed_for_dockdash_compat() {
        // Cloud OCI registries (ECR, GCR, …) return `/v2/{repo}/blobs/
        // uploads/{session-id}` Location URLs that push clients treat as
        // self-authenticating (no further Bearer token sent). The proxy
        // has to sign these URLs the same way it signs GAR's
        // `/artifacts-uploads/` URLs, otherwise the subsequent PUT/PATCH
        // arrives at the proxy with no Authorization and fails with 401.
        let location = "https://manager.example.com/v2/repo/blobs/uploads/session-1";

        let signed = rewrite_location_with_upload_session_auth(
            location,
            Some("cloud-project/artifacts/prj_123"),
            b"test-key",
        )
        .expect("OCI upload-session location should be signed");

        assert_ne!(signed, location, "URL should have been signed");
        assert!(signed.contains(UPLOAD_SESSION_VERSION_PARAM));
        assert!(signed.contains(UPLOAD_SESSION_SIGNATURE_PARAM));
        assert!(signed.contains(UPLOAD_SESSION_EXPIRES_PARAM));
    }

    #[test]
    fn non_session_location_is_not_signed() {
        // Locations that aren't upload-session URLs (e.g. a manifest URL
        // returned on push, or any other generic OCI path) must NOT get
        // signed — they go through Bearer auth like the rest of the API.
        let location = "https://manager.example.com/v2/repo/manifests/latest";

        assert_eq!(
            rewrite_location_with_upload_session_auth(
                location,
                Some("cloud-project/artifacts/prj_123"),
                b"test-key",
            )
            .expect("non-session location should be unchanged"),
            location
        );
    }

    #[test]
    fn is_oci_upload_session_path_matches_session_urls_only() {
        // Initial upload POST has no session-id suffix — must NOT be
        // recognized as a session URL, so Bearer auth still kicks in.
        assert!(!is_oci_upload_session_path("/v2/repo/blobs/uploads/"));
        assert!(!is_oci_upload_session_path("v2/repo/blobs/uploads/"));

        // Real session URLs.
        assert!(is_oci_upload_session_path(
            "/v2/repo/blobs/uploads/3403bc14-cbcd-3760-a4b1-c678a3c6ea61"
        ));
        assert!(is_oci_upload_session_path(
            "v2/alien-artifacts/host-loader/blobs/uploads/abc-123"
        ));

        assert!(is_oci_upload_session_path(
            "/v2/repo/blobs/uploads/a.b_c~d=e+f:g-h"
        ));

        // Other OCI paths.
        assert!(!is_oci_upload_session_path("/v2/repo/manifests/latest"));
        assert!(!is_oci_upload_session_path("/v2/repo/blobs/sha256:abc"));
        assert!(!is_oci_upload_session_path("/artifacts-uploads/something"));
        // A name may contain `blobs/uploads` components; the operation is the path's end.
        assert!(!is_oci_upload_session_path(
            "/v2/repo/blobs/uploads/blobs/uploads/"
        ));
        assert!(!is_oci_upload_session_path(
            "/v2/repo/blobs/uploads/manifests/latest"
        ));
        assert!(is_oci_upload_session_path(
            "/v2/repo/blobs/uploads/blobs/uploads/abc-123"
        ));
    }

    #[test]
    fn raw_gar_upload_session_path_does_not_identify_project_repo() {
        assert_eq!(
            project_id_after_prefix(
                "/artifacts-uploads/namespaces/cloud-project/repositories/artifacts/uploads/session-1",
                "cloud-project/artifacts",
            ),
            None
        );
        assert_eq!(
            project_id_after_prefix("cloud-project/artifacts/prj_123", "cloud-project/artifacts",),
            Some("prj_123")
        );
    }
}

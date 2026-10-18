use std::sync::{atomic::Ordering, Arc};

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::server::Gateway;

/// Admin listener state. The token is a deployment secret, distinct from all sandbox tokens.
pub struct Admin {
    pub gateway: Arc<Gateway>,
    token_hash: [u8; 32],
}
impl Admin {
    /// Require a high-entropy deployment token, supplied outside the public configuration.
    pub fn new(gateway: Arc<Gateway>, token: &str) -> crate::error::Result<Self> {
        if token.len() < 32 {
            return Err(crate::error::invalid(
                "admin token must contain at least 32 bytes",
            ));
        }
        Ok(Self {
            gateway,
            token_hash: Sha256::digest(token.as_bytes()).into(),
        })
    }
    fn authorized(&self, headers: &HeaderMap) -> bool {
        if headers.get_all("authorization").iter().count() != 1 {
            return false;
        }
        let Some(token) = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        let actual: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        // Fixed-length comparison. Compare digests so input length cannot affect the comparison.
        actual
            .iter()
            .zip(self.token_hash)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    aud: String,
    sub: String,
    gen: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    aud: String,
    sub: String,
}

/// Authenticated management API, health and aggregate metrics.
pub fn router(admin: Arc<Admin>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok\n" }))
        .route("/metrics", get(metrics))
        .route("/v1/generations", put(advance))
        .route("/v1/ca", post(issue_ca))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(admin)
}

fn identity_valid(admin: &Admin, aud: &str, sub: &str) -> bool {
    admin.gateway.config.bounds.contains_key(aud) && !sub.is_empty() && sub.len() <= 256
}

async fn advance(
    State(admin): State<Arc<Admin>>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> StatusCode {
    if !admin.authorized(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    if !identity_valid(&admin, &update.aud, &update.sub) {
        return StatusCode::BAD_REQUEST;
    }
    match admin
        .gateway
        .generations
        .advance(&update.aud, &update.sub, update.gen)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn issue_ca(
    State(admin): State<Arc<Admin>>,
    headers: HeaderMap,
    Json(identity): Json<Identity>,
) -> Response {
    if !admin.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !identity_valid(&admin, &identity.aud, &identity.sub) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match admin
        .gateway
        .credentials
        .issue_ca(&identity.aud, &identity.sub)
        .await
    {
        Ok(certificate) => Json(serde_json::json!({"certificate": certificate})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn metrics(State(admin): State<Arc<Admin>>, headers: HeaderMap) -> Response {
    if !admin.authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    format!("# TYPE alien_egress_connections_total counter\nalien_egress_connections_total {}\n# TYPE alien_egress_denied_total counter\nalien_egress_denied_total {}\n",
        admin.gateway.accepted.load(Ordering::Relaxed), admin.gateway.denied.load(Ordering::Relaxed)).into_response()
}

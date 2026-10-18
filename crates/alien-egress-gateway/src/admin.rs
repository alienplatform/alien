use std::sync::{atomic::Ordering, Arc};

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::server::Gateway;

/// Admin listener state; only deployment public keys are needed.
pub struct Admin {
    pub gateway: Arc<Gateway>,
}
impl Admin {
    fn authorized(&self, headers: &HeaderMap) -> Option<crate::token::AdminClaims> {
        if headers.get_all("authorization").iter().count() != 1 {
            return None;
        }
        let token = headers
            .get("authorization")?
            .to_str()
            .ok()?
            .strip_prefix("Bearer ")?;
        crate::token::verify_admin(
            token,
            &self.gateway.keys,
            chrono::Utc::now().timestamp().max(0) as u64,
        )
        .ok()
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
        .route("/v1/generations", post(advance))
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
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED;
    };
    if grant.aud != update.aud || grant.sub != update.sub || grant.gen != update.gen {
        return StatusCode::FORBIDDEN;
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
        Err(error) if error.code == "EGRESS_GENERATION_SUPERSEDED" => StatusCode::CONFLICT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn issue_ca(
    State(admin): State<Arc<Admin>>,
    headers: HeaderMap,
    Json(identity): Json<Identity>,
) -> Response {
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.aud != identity.aud || grant.sub != identity.sub {
        return StatusCode::FORBIDDEN.into_response();
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
    if admin.authorized(&headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    format!("# TYPE alien_egress_connections_total counter\nalien_egress_connections_total {}\n# TYPE alien_egress_denied_total counter\nalien_egress_denied_total {}\n",
        admin.gateway.accepted.load(Ordering::Relaxed), admin.gateway.denied.load(Ordering::Relaxed)).into_response()
}

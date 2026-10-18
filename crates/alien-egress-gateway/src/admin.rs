use std::sync::{atomic::Ordering, Arc};

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::{runtime::LiveGateway, server::Gateway};

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
    policy: crate::policy::Policy,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identity {
    aud: String,
    sub: String,
}

/// Authenticated management API, health and aggregate metrics.
pub fn router(admin: Arc<LiveGateway>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/policies", put(update).delete(remove))
        .route("/v1/ca", post(issue_ca))
        .layer(DefaultBodyLimit::max(
            crate::policy::MAX_POLICY_BYTES + 4096,
        ))
        .with_state(admin)
}

fn identity_valid(admin: &Admin, aud: &str, sub: &str) -> bool {
    admin
        .gateway
        .config
        .audiences
        .iter()
        .any(|value| value == aud)
        && !sub.is_empty()
        && sub.len() <= 256
}

async fn update(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.aud != update.aud || grant.sub != update.sub || grant.method != "PUT" {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !identity_valid(&admin, &update.aud, &update.sub) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let bytes = match crate::policy::canonical(&update.policy) {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"code":error.code,"message":error.message})),
            )
                .into_response()
        }
    };
    if grant.policy.as_deref() != Some(crate::policy::digest(&bytes).as_str()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match admin
        .gateway
        .policies
        .put(&update.aud, &update.sub, &update.policy)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
    .into_response()
}

async fn remove(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Json(identity): Json<Identity>,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.aud != identity.aud
        || grant.sub != identity.sub
        || grant.method != "DELETE"
        || grant.policy.is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !identity_valid(&admin, &identity.aud, &identity.sub) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match admin
        .gateway
        .policies
        .delete(&identity.aud, &identity.sub)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
    .into_response()
}

async fn issue_ca(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Json(identity): Json<Identity>,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.aud != identity.aud || grant.sub != identity.sub || grant.method != "POST" {
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

async fn metrics(State(live): State<Arc<LiveGateway>>, headers: HeaderMap) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    if !admin
        .authorized(&headers)
        .is_some_and(|grant| grant.method == "GET")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    format!("# TYPE alien_egress_connections_total counter\nalien_egress_connections_total {}\n# TYPE alien_egress_denied_total counter\nalien_egress_denied_total {}\n",
        admin.gateway.accepted.load(Ordering::Relaxed), admin.gateway.denied.load(Ordering::Relaxed)).into_response()
}

async fn health(State(live): State<Arc<LiveGateway>>) -> Response {
    if live.snapshot().is_some() {
        "ok\n".into_response()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

use std::sync::{atomic::Ordering, Arc};

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    record::{Mutation, Record, Summary},
    runtime::LiveGateway,
    server::Gateway,
};

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
#[serde(deny_unknown_fields)]
struct Identity {
    aud: String,
    sub: String,
}

/// Authenticated management API, health and aggregate metrics.
pub fn router(admin: Arc<LiveGateway>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/policies", put(update).delete(remove).get(version))
        .route("/v1/ca", post(issue_ca))
        .route("/v1/policies/record", get(read_record))
        .route("/v1/replicas/{operation}", post(exchange))
        .layer(DefaultBodyLimit::max(crate::replication::MAX_SNAPSHOT))
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
    Json(mutation): Json<Mutation>,
) -> Response {
    mutate(live, headers, mutation, "PUT").await
}
async fn remove(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Json(mutation): Json<Mutation>,
) -> Response {
    mutate(live, headers, mutation, "DELETE").await
}
fn failure(error: alien_error::AlienError<crate::error::ErrorData>) -> Response {
    let status = match error.code.as_str() {
        "SANDBOX_EGRESS_POLICY_STORE_FULL" | "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE" => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(serde_json::json!({"code":error.code,"message":error.message})),
    )
        .into_response()
}
async fn mutate(
    live: Arc<LiveGateway>,
    headers: HeaderMap,
    mutation: Mutation,
    method: &str,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let record = match Record::verify(
        mutation.record,
        &admin.gateway.keys,
        &admin.gateway.config.audiences,
        chrono::Utc::now().timestamp_millis().max(0) as u64,
    ) {
        Ok(record) => record,
        Err(error) => return failure(error),
    };
    if grant.method != method
        || grant.aud != record.claims.aud
        || grant.sub != record.claims.sub
        || record.claims.deleted != (method == "DELETE")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    publish(&admin.gateway, record).await
}
async fn publish(gateway: &Gateway, record: Record) -> Response {
    if !gateway.policies.ready() {
        return failure(alien_error::AlienError::new(
            crate::error::ErrorData::GatewayUnavailable,
        ));
    }
    if let Err(error) = gateway.policies.apply(record.clone(), false) {
        return failure(error);
    }
    match gateway.replication.fanout(&record, false).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => failure(alien_error::AlienError::new(
            crate::error::ErrorData::GatewayUnavailable,
        )),
    }
}

async fn read_record(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Query(identity): Query<Identity>,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.method != "GET"
        || grant.aud != identity.aud
        || grant.sub != identity.sub
        || grant.policy.is_some()
        || grant.version.is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !admin.gateway.policies.ready() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    match admin.gateway.policies.record(&identity.aud, &identity.sub) {
        Ok(Some(record)) => Json(Mutation {
            record: record.signed,
        })
        .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn exchange(
    State(live): State<Arc<LiveGateway>>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let path = format!("/v1/replicas/{operation}");
    if gateway
        .replication
        .authenticate(&headers, "POST", &path, &body)
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match operation.as_str() {
        "summary" => match gateway.policies.summary() {
            Ok(summary) => Json(summary).into_response(),
            Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
        "pull" | "snapshot" => {
            let Ok(known) = serde_json::from_slice::<Vec<Summary>>(&body) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            match gateway.policies.newer_than(&known) {
                Ok(records) => Json(
                    records
                        .into_iter()
                        .map(|record| record.signed)
                        .collect::<Vec<_>>(),
                )
                .into_response(),
                Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
        "write" | "register" => {
            let Ok(mutation) = serde_json::from_slice::<Mutation>(&body) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let record = match Record::verify(
                mutation.record,
                &gateway.keys,
                &gateway.config.audiences,
                chrono::Utc::now().timestamp_millis().max(0) as u64,
            ) {
                Ok(record) => record,
                Err(error) => return failure(error),
            };
            match gateway.policies.apply(record, operation == "register") {
                Ok(_) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => failure(error),
            }
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn version(
    State(live): State<Arc<LiveGateway>>,
    headers: HeaderMap,
    Query(identity): Query<Identity>,
) -> Response {
    let Some(gateway) = live.snapshot() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let admin = Admin { gateway };
    let Some(grant) = admin.authorized(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if grant.method != "GET"
        || grant.aud != identity.aud
        || grant.sub != identity.sub
        || grant.policy.is_some()
        || grant.version.is_some()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !identity_valid(&admin, &identity.aud, &identity.sub) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match admin
        .gateway
        .policies
        .version(&identity.aud, &identity.sub)
        .await
    {
        Ok(version) => Json(serde_json::json!({"version":version})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
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
    let mut metrics = format!("# TYPE alien_egress_connections_total counter\nalien_egress_connections_total {}\n# TYPE alien_egress_denied_total counter\nalien_egress_denied_total {}\n# TYPE alien_egress_active_connections gauge\nalien_egress_active_connections {}\n# TYPE alien_egress_denials_total counter\n",
        admin.gateway.accepted.load(Ordering::Relaxed), admin.gateway.denied.load(Ordering::Relaxed), admin.gateway.connections.active.load(Ordering::Relaxed));
    for (index, reason) in [
        "capacity",
        "authentication",
        "policy",
        "protocol",
        "dependency",
    ]
    .iter()
    .enumerate()
    {
        metrics.push_str(&format!(
            "alien_egress_denials_total{{reason=\"{reason}\"}} {}\n",
            admin.gateway.connections.denials[index].load(Ordering::Relaxed)
        ));
    }
    metrics.into_response()
}

async fn health(State(live): State<Arc<LiveGateway>>) -> Response {
    if live
        .snapshot()
        .is_some_and(|gateway| gateway.policies.ready())
    {
        "ok\n".into_response()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

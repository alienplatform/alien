//! The sandbox session broker, served by the operator.
//!
//! Mounted on the operator's existing HTTP server, which the Helm chart already exposes through
//! a Service, so this adds no deployment surface and no chart change.
//!
//! **Why a route at all.** Claiming a warm pod is a `PATCH` on pods. Putting that in the binding
//! would give the customer's application pod-write on the namespace, and `pods/exec` on top of
//! that reaches every pod there. The application asks for a session and gets back an address and
//! a capability scoped to it; the cluster credential stays with the operator.
//!
//! **Why no token of ours.** The caller authenticates with the ServiceAccount token Kubernetes
//! already mounted in its pod, checked with a `TokenReview`. `alien-bindings`'
//! `credential_source` states the rule this follows: managed workloads use their
//! platform-projected identity and do not receive Alien bearer tokens.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::sandbox::kubernetes::capability_secret_name;
use crate::sandbox::kubernetes_broker::{claim_session, release_session};
use alien_error::Context;
use alien_k8s_clients::kubernetes::pods::PodApi;
use alien_k8s_clients::kubernetes::secrets::SecretsApi;
use alien_k8s_clients::kubernetes::token_reviews::{
    authenticated_user, is_service_account_in, review_for, TokenReviewsApi,
};

/// What the broker needs to serve one deployment's sandboxes.
#[derive(Clone)]
pub struct BrokerState {
    /// Claims and releases pods
    pub pods: Arc<dyn PodApi>,
    /// Reads the capability signing key
    pub secrets: Arc<dyn SecretsApi>,
    /// Verifies the caller's ServiceAccount token
    pub token_reviews: Arc<dyn TokenReviewsApi>,
    /// Namespace the sandbox pods live in, and the only namespace a caller may come from
    pub namespace: String,
}

/// A request for a session.
///
/// Carries ids only. Limits, image and egress come from the pod template the controller built,
/// and the signing key is derived from the sandbox id rather than named by the caller, so an
/// application cannot widen its own confinement by asking.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimRequest {
    /// Sandbox whose pool to claim from
    pub sandbox_id: String,
    /// Session id the pod will carry
    pub session_id: String,
}

/// What the application needs to reach its session.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimResponse {
    /// Session id, which every later call addresses
    pub session_id: String,
    /// `http://<pod ip>:<agent port>`
    pub endpoint: String,
    /// Bearer capability for the agent, scoped to this session
    pub capability: String,
    /// Unix seconds after which the capability is void
    pub expires_at: i64,
}

impl BrokerState {
    /// Builds broker state from the pod's own in-cluster credentials.
    ///
    /// The operator asks for this rather than assembling a Kubernetes client itself: the client
    /// crate is an implementation detail of this crate, and threading it through the caller
    /// would make the operator depend on it for one line.
    pub async fn in_cluster(namespace: String) -> crate::error::Result<Self> {
        let client = std::sync::Arc::new(
            alien_k8s_clients::kubernetes::kubernetes_client::KubernetesClient::new(
                alien_k8s_clients::KubernetesClientConfig::InCluster {
                    additional_headers: None,
                    namespace: Some(namespace.clone()),
                },
            )
            .await
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "the sandbox broker needs in-cluster Kubernetes credentials".to_string(),
                resource_id: None,
            })?,
        );

        Ok(Self {
            pods: client.clone(),
            secrets: client.clone(),
            token_reviews: client,
            namespace,
        })
    }
}

/// The broker's routes, for the operator to mount.
pub fn broker_router(state: BrokerState) -> Router {
    Router::new()
        .route("/v1/sandbox/sessions", post(claim))
        .route(
            "/v1/sandbox/{sandbox}/sessions/{session}",
            axum::routing::delete(release),
        )
        .with_state(state)
}

/// Verifies the caller is a ServiceAccount in this deployment's namespace.
///
/// The namespace check is the authorization: any pod on the cluster network can reach this port,
/// and a valid token from another tenant's namespace is a valid token for the wrong sandbox.
async fn authorize(state: &BrokerState, headers: &HeaderMap) -> Result<String, StatusCode> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let verdict = state
        .token_reviews
        .create_token_review(&review_for(token))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // A rejected token comes back 200 with `authenticated: false`, so the verdict is read rather
    // than the status code.
    let user = authenticated_user(&verdict).ok_or(StatusCode::UNAUTHORIZED)?;

    if !is_service_account_in(&user, &state.namespace) {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(user)
}

async fn claim(
    State(state): State<BrokerState>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, StatusCode> {
    authorize(&state, &headers).await?;

    let claimed = claim_session(
        &state.pods,
        &state.secrets,
        &request.sandbox_id,
        &state.namespace,
        &request.session_id,
        &capability_secret_name(&request.sandbox_id),
        chrono::Utc::now().timestamp(),
    )
    .await
    // Retryable rather than fatal: an empty pool refills on the controller's next health tick,
    // and 503 is what tells a caller to wait rather than to give up.
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    Ok(Json(ClaimResponse {
        session_id: claimed.session_id,
        endpoint: claimed.endpoint,
        capability: claimed.capability,
        expires_at: claimed.expires_at,
    }))
}

async fn release(
    State(state): State<BrokerState>,
    headers: HeaderMap,
    Path((sandbox_id, session)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    authorize(&state, &headers).await?;

    // A pod that is not a claimed session of this sandbox is refused rather than deleted, so the
    // route cannot be used to reach anything else sharing the namespace.
    release_session(&state.pods, &state.namespace, &sandbox_id, &session)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_k8s_clients::kubernetes::pods::MockPodApi;
    use alien_k8s_clients::kubernetes::secrets::MockSecretsApi;
    use alien_k8s_clients::kubernetes::token_reviews::MockTokenReviewsApi;
    use k8s_openapi::api::authentication::v1::{TokenReview, TokenReviewStatus, UserInfo};

    fn verdict(authenticated: bool, username: &str) -> TokenReview {
        TokenReview {
            status: Some(TokenReviewStatus {
                authenticated: Some(authenticated),
                user: Some(UserInfo {
                    username: Some(username.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn state_with(reviews: MockTokenReviewsApi) -> BrokerState {
        BrokerState {
            pods: Arc::new(MockPodApi::new()),
            secrets: Arc::new(MockSecretsApi::new()),
            token_reviews: Arc::new(reviews),
            namespace: "alien-sandbox-sbx".to_string(),
        }
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("valid header"),
        );
        headers
    }

    /// The signing key is what makes a capability valid for a pod. If a caller could name it, a
    /// workload holding one sandbox's handle could mint a capability under a sibling's key by
    /// naming that sibling's secret, so the broker derives the name from the sandbox id instead.
    #[test]
    fn a_caller_cannot_name_the_signing_key() {
        let named = serde_json::from_str::<ClaimRequest>(
            r#"{"sandboxId":"agent","sessionId":"s1","keyName":"alien-sandbox-other-capability"}"#,
        );
        assert!(named.is_err(), "naming the key must not deserialize");

        let request: ClaimRequest =
            serde_json::from_str(r#"{"sandboxId":"agent","sessionId":"s1"}"#)
                .expect("ids alone are the whole request");
        assert_eq!(
            capability_secret_name(&request.sandbox_id),
            "alien-sandbox-agent-capability"
        );
    }

    #[tokio::test]
    async fn a_request_without_a_token_is_refused_before_any_cluster_call() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews
            .expect_create_token_review()
            .never()
            .returning(|_| Ok(TokenReview::default()));

        let error = authorize(&state_with(reviews), &HeaderMap::new())
            .await
            .expect_err("no token is unauthorized");
        assert_eq!(error, StatusCode::UNAUTHORIZED);
    }

    /// The apiserver answers 200 for a bad token with `authenticated: false`. Reading the status
    /// code as the verdict would authenticate everything.
    #[tokio::test]
    async fn a_token_the_apiserver_rejects_is_unauthorized() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews
            .expect_create_token_review()
            .returning(|_| Ok(verdict(false, "system:serviceaccount:alien-sandbox-sbx:app")));

        let error = authorize(&state_with(reviews), &bearer("nonsense"))
            .await
            .expect_err("a rejected token is unauthorized");
        assert_eq!(error, StatusCode::UNAUTHORIZED);
    }

    /// Any pod on the cluster network can reach this port, so a valid token from another
    /// namespace is a valid token for the wrong sandbox.
    #[tokio::test]
    async fn a_valid_token_from_another_namespace_is_forbidden() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews
            .expect_create_token_review()
            .returning(|_| Ok(verdict(true, "system:serviceaccount:someone-else:app")));

        let error = authorize(&state_with(reviews), &bearer("valid-elsewhere"))
            .await
            .expect_err("another namespace is forbidden");
        assert_eq!(error, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_service_account_in_this_namespace_is_allowed() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews
            .expect_create_token_review()
            .returning(|_| Ok(verdict(true, "system:serviceaccount:alien-sandbox-sbx:worker")));

        let user = authorize(&state_with(reviews), &bearer("valid"))
            .await
            .expect("the deployment's own ServiceAccount is allowed");
        assert_eq!(user, "system:serviceaccount:alien-sandbox-sbx:worker");
    }
}

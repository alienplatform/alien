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

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::sandbox::kubernetes::capability_secret_name;
use crate::sandbox::kubernetes_broker::claim_session_for_deployment;
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

/// The exact deployment scope and callers the broker may operate for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokerDeploymentAuthorization {
    /// Kubernetes label key that identifies this deployment's runtime objects.
    pub deployment_label_key: String,
    /// Exact value for this deployment's runtime objects.
    pub deployment_label_value: String,
    /// Exact ServiceAccount names created for workloads in this deployment.
    pub service_accounts: BTreeSet<String>,
}

/// Supplies the broker's current authorization boundary.
///
/// Remote operators learn their resource prefix only after their first sync, so the provider is
/// consulted per request instead of freezing an empty scope at process startup.
#[async_trait::async_trait]
pub trait BrokerAuthorizationProvider: Send + Sync {
    /// Returns the current exact deployment authorization, or fails closed while unavailable.
    async fn authorization(&self) -> crate::error::Result<BrokerDeploymentAuthorization>;
}

#[derive(Clone)]
struct BrokerRouteState {
    broker: BrokerState,
    authorization: Arc<dyn BrokerAuthorizationProvider>,
}

struct EnvironmentBrokerAuthorization;

#[async_trait::async_trait]
impl BrokerAuthorizationProvider for EnvironmentBrokerAuthorization {
    async fn authorization(&self) -> crate::error::Result<BrokerDeploymentAuthorization> {
        let configuration_error = |message: &str| {
            alien_error::AlienError::new(crate::error::ErrorData::ResourceConfigInvalid {
                resource_id: Some("sandbox-broker".to_string()),
                message: message.to_string(),
            })
        };
        let deployment_label_key = std::env::var("ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY")
            .map_err(|_| configuration_error("runtime deployment label key is unavailable"))?;
        let deployment_label_value = std::env::var("ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE")
            .map_err(|_| configuration_error("runtime deployment label value is unavailable"))?;
        let service_accounts = std::env::var("ALIEN_SANDBOX_BROKER_SERVICE_ACCOUNTS")
            .map_err(|_| configuration_error("sandbox broker ServiceAccounts are unavailable"))?
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        if deployment_label_key.is_empty()
            || deployment_label_value.is_empty()
            || service_accounts.is_empty()
        {
            return Err(configuration_error(
                "sandbox broker deployment authorization is incomplete",
            ));
        }
        Ok(BrokerDeploymentAuthorization {
            deployment_label_key,
            deployment_label_value,
            service_accounts,
        })
    }
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
///
/// This compatibility constructor reads its authorization boundary for every
/// request from `ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY`,
/// `ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE`, and the comma-separated
/// `ALIEN_SANDBOX_BROKER_SERVICE_ACCOUNTS`. Missing or empty values fail closed
/// with HTTP 503. New embedders should pass an explicit, dynamically refreshed
/// provider to [`broker_router_with_authorization`] instead.
#[deprecated(
    since = "3.3.22",
    note = "use broker_router_with_authorization with an explicit authorization provider"
)]
pub fn broker_router(state: BrokerState) -> Router {
    broker_router_with_authorization(state, Arc::new(EnvironmentBrokerAuthorization))
}

/// The broker's routes with an authorization provider owned by the embedding operator.
pub fn broker_router_with_authorization(
    state: BrokerState,
    authorization: Arc<dyn BrokerAuthorizationProvider>,
) -> Router {
    Router::new()
        .route("/v1/sandbox/sessions", post(claim))
        .route(
            "/v1/sandbox/{sandbox}/sessions/{session}",
            axum::routing::delete(release),
        )
        .with_state(BrokerRouteState {
            broker: state,
            authorization,
        })
}

/// Verifies the caller is a ServiceAccount owned by this deployment.
///
/// Namespace alone is not an ownership boundary because multiple product releases may share it.
async fn authorize(
    state: &BrokerRouteState,
    headers: &HeaderMap,
) -> Result<BrokerDeploymentAuthorization, StatusCode> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let verdict = state
        .broker
        .token_reviews
        .create_token_review(&review_for(token))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // A rejected token comes back 200 with `authenticated: false`, so the verdict is read rather
    // than the status code.
    let user = authenticated_user(&verdict).ok_or(StatusCode::UNAUTHORIZED)?;

    if !is_service_account_in(&user, &state.broker.namespace) {
        return Err(StatusCode::FORBIDDEN);
    }

    let username_prefix = format!("system:serviceaccount:{}:", state.broker.namespace);
    let service_account_name = user
        .strip_prefix(&username_prefix)
        .ok_or(StatusCode::FORBIDDEN)?;
    let authorization = state
        .authorization
        .authorization()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !authorization
        .service_accounts
        .contains(service_account_name)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(authorization)
}

async fn claim(
    State(state): State<BrokerRouteState>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, StatusCode> {
    let authorization = authorize(&state, &headers).await?;

    let claimed = claim_session_for_deployment(
        &state.broker.pods,
        &state.broker.secrets,
        &request.sandbox_id,
        &state.broker.namespace,
        &request.session_id,
        &capability_secret_name(&request.sandbox_id),
        chrono::Utc::now().timestamp(),
        &authorization.deployment_label_key,
        &authorization.deployment_label_value,
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
    State(state): State<BrokerRouteState>,
    headers: HeaderMap,
    Path((sandbox_id, session)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let authorization = authorize(&state, &headers).await?;

    // A pod that is not a claimed session of this sandbox is refused rather than deleted, so the
    // route cannot be used to reach anything else sharing the namespace.
    crate::sandbox::kubernetes_broker::release_session_for_deployment(
        &state.broker.pods,
        &state.broker.secrets,
        &state.broker.namespace,
        &sandbox_id,
        &session,
        &capability_secret_name(&sandbox_id),
        &authorization.deployment_label_key,
        &authorization.deployment_label_value,
    )
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
    use k8s_openapi::api::core::v1::Secret;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    struct FixedAuthorization(BrokerDeploymentAuthorization);

    #[async_trait::async_trait]
    impl BrokerAuthorizationProvider for FixedAuthorization {
        async fn authorization(&self) -> crate::error::Result<BrokerDeploymentAuthorization> {
            Ok(self.0.clone())
        }
    }

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

    fn state_with(reviews: MockTokenReviewsApi) -> BrokerRouteState {
        state_with_allowed_accounts(reviews, ["worker"])
    }

    fn state_with_allowed_accounts(
        reviews: MockTokenReviewsApi,
        accounts: impl IntoIterator<Item = &'static str>,
    ) -> BrokerRouteState {
        BrokerRouteState {
            broker: BrokerState {
                pods: Arc::new(MockPodApi::new()),
                secrets: Arc::new(MockSecretsApi::new()),
                token_reviews: Arc::new(reviews),
                namespace: "alien-sandbox-sbx".to_string(),
            },
            authorization: Arc::new(FixedAuthorization(BrokerDeploymentAuthorization {
                deployment_label_key: "alien.dev/deployment".to_string(),
                deployment_label_value: "release-a".to_string(),
                service_accounts: accounts.into_iter().map(str::to_string).collect(),
            })),
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

    fn scoped_state(
        pods: MockPodApi,
        secrets: MockSecretsApi,
        reviews: MockTokenReviewsApi,
    ) -> BrokerRouteState {
        BrokerRouteState {
            broker: BrokerState {
                pods: Arc::new(pods),
                secrets: Arc::new(secrets),
                token_reviews: Arc::new(reviews),
                namespace: "shared".to_string(),
            },
            authorization: Arc::new(FixedAuthorization(BrokerDeploymentAuthorization {
                deployment_label_key: "alien.dev/deployment".to_string(),
                deployment_label_value: "release-a".to_string(),
                service_accounts: BTreeSet::from(["release-a-manager-sa".to_string()]),
            })),
        }
    }

    fn capability_secret_for(scope: &str) -> Secret {
        let pair = ed25519_compact::KeyPair::generate();
        Secret {
            metadata: ObjectMeta {
                labels: Some(BTreeMap::from([
                    ("alien.dev/deployment".to_string(), scope.to_string()),
                    ("alien.dev/sandbox".to_string(), "sbx".to_string()),
                ])),
                ..Default::default()
            },
            data: Some(BTreeMap::from([(
                "signingKey".to_string(),
                k8s_openapi::ByteString(pair.as_ref().to_vec()),
            )])),
            ..Default::default()
        }
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
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                false,
                "system:serviceaccount:alien-sandbox-sbx:app",
            ))
        });

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
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                true,
                "system:serviceaccount:alien-sandbox-sbx:worker",
            ))
        });

        let authorization = authorize(&state_with(reviews), &bearer("valid"))
            .await
            .expect("the deployment's own ServiceAccount is allowed");
        assert_eq!(
            authorization.service_accounts,
            BTreeSet::from(["worker".to_string()])
        );
    }

    #[tokio::test]
    async fn an_overlapping_service_account_prefix_does_not_authorize_a_sibling_release() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                true,
                "system:serviceaccount:alien-sandbox-sbx:release-a-evil-worker-sa",
            ))
        });

        let error = authorize(&state_with(reviews), &bearer("sibling"))
            .await
            .expect_err("a sibling ServiceAccount needs an exact allow-list entry");
        assert_eq!(error, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_truncated_long_service_account_name_is_authorized_by_its_exact_scope_label() {
        let mut reviews = MockTokenReviewsApi::new();
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                true,
                "system:serviceaccount:alien-sandbox-sbx:release-a-extraordinarily-long-permission-profile-name-truncated",
            ))
        });

        authorize(
            &state_with_allowed_accounts(
                reviews,
                ["release-a-extraordinarily-long-permission-profile-name-truncated"],
            ),
            &bearer("long-name"),
        )
        .await
        .expect("the exact generated ServiceAccount name is authorized");
    }

    #[tokio::test]
    async fn release_a_cannot_claim_release_b_sandbox_in_a_shared_namespace() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().never();
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(capability_secret_for("release-b")));
        let mut reviews = MockTokenReviewsApi::new();
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                true,
                "system:serviceaccount:shared:release-a-manager-sa",
            ))
        });

        let error = claim(
            State(scoped_state(pods, secrets, reviews)),
            bearer("release-a"),
            Json(ClaimRequest {
                sandbox_id: "sbx".to_string(),
                session_id: "session".to_string(),
            }),
        )
        .await
        .expect_err("a foreign deployment Secret must not authorize a claim");
        assert_eq!(error, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn release_a_cannot_release_release_b_sandbox_in_a_shared_namespace() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().never();
        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(capability_secret_for("release-b")));
        let mut reviews = MockTokenReviewsApi::new();
        reviews.expect_create_token_review().returning(|_| {
            Ok(verdict(
                true,
                "system:serviceaccount:shared:release-a-manager-sa",
            ))
        });

        let error = release(
            State(scoped_state(pods, secrets, reviews)),
            bearer("release-a"),
            Path(("sbx".to_string(), "session".to_string())),
        )
        .await
        .expect_err("a foreign deployment Secret must not authorize a release");
        assert_eq!(error, StatusCode::FORBIDDEN);
    }
}

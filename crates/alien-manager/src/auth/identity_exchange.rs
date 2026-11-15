//! Exchange verified component identity for a narrowly scoped runtime credential.

use std::sync::Arc;

use alien_error::AlienError;
use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, Extension, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::runtime_capability::{RuntimeCredentials, RuntimeGrant, RuntimePurpose};
use crate::{
    error::ErrorData,
    routes::{auth, AppState},
    traits::DeploymentRecord,
};

/// Proof material is intentionally not Debug: providers must never log it.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeIdentityProof {
    /// An audience-signed STS GetCallerIdentity request.
    Aws {
        #[cfg_attr(feature = "openapi", schema(rename = "presignedUrl"))]
        presigned_url: String,
    },
    /// A metadata identity token for this manager's audience.
    Gcp {
        #[cfg_attr(feature = "openapi", schema(rename = "idToken"))]
        id_token: String,
    },
    /// A bounded parent session; it cannot renew itself.
    Session { token: String },
    /// The explicit daily-key exception for enrolled components.
    DailyKey { key: String },
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityExchangeRequest {
    /// Selects the binding to verify; never establishes membership by itself.
    pub deployment_id: String,
    pub purpose: RuntimePurpose,
    pub proof: RuntimeIdentityProof,
}

/// A client must durably save the corresponding key before sending this request.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityEnrollmentRequest {
    pub code: String,
    pub initial_key_hash: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityEnrollmentResponse {
    pub deployment_id: String,
    pub component_id: String,
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
}

/// A client saves its generated successor before the atomic hash replacement.
#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityRotationRequest {
    pub deployment_id: String,
    pub key: String,
    pub successor_hash: String,
}

/// No successor credential or hash is disclosed to a retired key holder.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityRotationResponse {
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
}

/// A one-hour, one-use code that enrolls the deployment's operator.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssuedEnrollmentCode {
    pub deployment_id: String,
    pub code: String,
    pub expires_at: DateTime<Utc>,
    /// The audience the operator must use for this manager.
    pub manager_audience: String,
}

/// A key that updates one installation for a few minutes, and its deadline.
/// Not `Debug`: it holds a key.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssuedSetupCredential {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Trusted result of verifying both proof and recorded component membership.
pub struct VerifiedRuntimeIdentity {
    pub grant: RuntimeGrant,
    /// Fixed proof/session authority bound. Replays must not move this deadline.
    pub authority_deadline: DateTime<Utc>,
}

/// An embedder supplies its authoritative resource/session lookup here. The
/// adapter must validate issuer, audience and cloud principal before granting
/// rights, and fail closed when the principal has no recorded binding.
#[async_trait]
pub trait RuntimeIdentityVerifier: Send + Sync {
    /// Atomically consume a pre-authorized one-use reservation. The implementation
    /// must restrict it to this manager's currently assigned component.
    async fn enroll(
        &self,
        _request: &IdentityEnrollmentRequest,
    ) -> Result<IdentityEnrollmentResponse, AlienError> {
        Err(ErrorData::unauthorized("Enrollment unavailable").into_generic())
    }

    /// Replace an unexpired current hash by CAS, never returning the winning key.
    async fn rotate(
        &self,
        _request: &IdentityRotationRequest,
    ) -> Result<IdentityRotationResponse, AlienError> {
        Err(ErrorData::unauthorized("Rotation unavailable").into_generic())
    }

    async fn verify(
        &self,
        request: &IdentityExchangeRequest,
        audience: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError>;

    /// Whether `code` is an unexpired enrollment code. Before it enrolls, an
    /// operator pulls its own image from this manager with the code. This
    /// never consumes the code. Implementations without codes refuse.
    async fn enrollment_code_is_live(&self, _code: &str) -> Result<bool, AlienError> {
        Ok(false)
    }

    /// Issue a short-lived key that updates `deployment_id`, to the Operator
    /// that proves it holds that deployment's live key. The key is made for
    /// one deployment, lives 15 minutes, and replaces the deployment's earlier
    /// setup keys, so at most one is live. The implementation refuses any other
    /// caller: another deployment's key, a setup key itself, an expired key.
    /// By default no key is issued.
    async fn issue_setup_credential(
        &self,
        _deployment_id: &str,
        _operator_key: &str,
    ) -> Result<IssuedSetupCredential, AlienError> {
        Err(ErrorData::unauthorized("Setup credentials are unavailable").into_generic())
    }

    /// Issue a one-hour, one-use enrollment code for this deployment's operator.
    /// The caller is already authorized to update the deployment.
    async fn issue_enrollment_code(
        &self,
        _deployment: &DeploymentRecord,
    ) -> Result<(String, DateTime<Utc>), AlienError> {
        Err(ErrorData::unauthorized("Enrollment unavailable").into_generic())
    }

    /// Issue a code for a caller that holds only the deployment's own token.
    /// A verifier that keeps one key per deployment refuses once the operator
    /// has enrolled, with `ENROLLMENT_KEY_EXISTS`, so a copy of the token
    /// cannot replace the key. By default this is [`Self::issue_enrollment_code`].
    async fn issue_enrollment_code_for_deployment_token(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<(String, DateTime<Utc>), AlienError> {
        self.issue_enrollment_code(deployment).await
    }

    /// Issue a code for a caller that holds a deployment group credential.
    /// A verifier that tracks the operator key refuses with
    /// `ENROLLMENT_KEY_EXISTS` while that key is live. By default this is
    /// [`Self::issue_enrollment_code`].
    async fn issue_enrollment_code_for_group_token(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<(String, DateTime<Utc>), AlienError> {
        self.issue_enrollment_code(deployment).await
    }
}

/// Mount the exchange with the same issuer used by the runtime auth validator.
pub fn router(
    credentials: Arc<RuntimeCredentials>,
    verifier: Arc<dyn RuntimeIdentityVerifier>,
) -> Router<AppState> {
    Router::new()
        .route("/v1/identity/exchange", post(exchange))
        .route("/v1/identity/enroll", post(enroll))
        .route("/v1/identity/rotate", post(rotate))
        .route(
            "/v1/deployments/{id}/enrollment-code",
            post(issue_enrollment_code),
        )
        .route(
            "/v1/deployments/{id}/setup-credential",
            post(issue_setup_credential),
        )
        .layer(Extension(credentials))
        .layer(Extension(verifier))
        .layer(DefaultBodyLimit::max(16 * 1024))
}

/// Exchange a verified component proof for one bounded runtime purpose.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/identity/exchange",
    operation_id = "exchangeRuntimeIdentity",
    tag = "identity",
    request_body = IdentityExchangeRequest,
    responses(
        (status = 200, description = "Bounded runtime credential", body = super::runtime_capability::IssuedRuntimeCredential),
        (status = 401, description = "Identity proof or requested purpose refused"),
        (status = 503, description = "Identity verification unavailable")
    )
))]
pub async fn exchange(
    Extension(credentials): Extension<Arc<RuntimeCredentials>>,
    Extension(verifier): Extension<Arc<dyn RuntimeIdentityVerifier>>,
    Json(request): Json<IdentityExchangeRequest>,
) -> Response {
    let identity = match verifier.verify(&request, credentials.audience()).await {
        Ok(identity) => identity,
        Err(error) => {
            // Provider errors can contain signed URLs or tokens.
            // Never log or return their source message.
            return if error.retryable {
                AlienError::new(ErrorData::AuthenticationUnavailable).into_response()
            } else {
                ErrorData::unauthorized("Identity verification failed").into_response()
            };
        }
    };
    // A verifier cannot accidentally answer a different requested binding
    // or purpose. Its result still supplies all tenant/component identity.
    if identity.grant.deployment_id != request.deployment_id
        || identity.grant.purpose != request.purpose
    {
        return ErrorData::unauthorized("Identity does not authorize this request").into_response();
    }
    match credentials.issue(identity.grant, identity.authority_deadline) {
        Ok(issued) => ([(http::header::CACHE_CONTROL, "no-store")], Json(issued)).into_response(),
        Err(error) => error.into_response(),
    }
}

fn credential_result<T: Serialize>(result: Result<T, AlienError>) -> Response {
    match result {
        Ok(value) => ([(http::header::CACHE_CONTROL, "no-store")], Json(value)).into_response(),
        Err(error) if error.retryable => {
            AlienError::new(ErrorData::AuthenticationUnavailable).into_response()
        }
        Err(_) => ErrorData::unauthorized("Component credential refused").into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post, path = "/v1/identity/enroll", operation_id = "enrollRuntimeIdentity", tag = "identity",
    request_body = IdentityEnrollmentRequest,
    responses((status = 200, body = IdentityEnrollmentResponse), (status = 401), (status = 503))
))]
pub async fn enroll(
    State(state): State<AppState>,
    Extension(verifier): Extension<Arc<dyn RuntimeIdentityVerifier>>,
    Json(request): Json<IdentityEnrollmentRequest>,
) -> Response {
    let result = verifier.enroll(&request).await;
    if let Err(error) = &result {
        if is_key_exists_refusal(error) {
            let deployment_id = error
                .context
                .as_ref()
                .and_then(|context| context.get("deployment_id"))
                .and_then(|id| id.as_str())
                .unwrap_or("unknown");
            record_refused_token_code(&state, deployment_id, "redeem").await;
        }
    }
    credential_result(result)
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post, path = "/v1/identity/rotate", operation_id = "rotateRuntimeIdentity", tag = "identity",
    request_body = IdentityRotationRequest,
    responses((status = 200, body = IdentityRotationResponse), (status = 401), (status = 503))
))]
pub async fn rotate(
    Extension(verifier): Extension<Arc<dyn RuntimeIdentityVerifier>>,
    Json(request): Json<IdentityRotationRequest>,
) -> Response {
    credential_result(verifier.rotate(&request).await)
}

/// Issue the code that `management.enrollmentCode` carries into a Helm install.
#[cfg_attr(feature = "openapi", utoipa::path(
    post, path = "/v1/deployments/{id}/enrollment-code", operation_id = "issueEnrollmentCode",
    tag = "identity",
    params(("id" = String, Path, description = "Deployment ID")),
    responses(
        (status = 200, body = IssuedEnrollmentCode),
        (status = 401), (status = 403), (status = 404)
    )
))]
pub async fn issue_enrollment_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Extension(credentials): Extension<Arc<RuntimeCredentials>>,
    Extension(verifier): Extension<Arc<dyn RuntimeIdentityVerifier>>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    // A runtime capability never mints longer-lived authority.
    if super::runtime_capability::is_runtime_capability(&subject.bearer_token) {
        return ErrorData::forbidden("Cannot issue an enrollment code").into_response();
    }
    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(deployment)) => deployment,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(error) => return error.into_response(),
    };
    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot issue an enrollment code").into_response();
    }
    // A deployment token's code is issued only while the deployment has no
    // operator key, and a deployment group credential's while that key is not
    // live. An administrator credential can always enroll again.
    let issued = match subject.role {
        super::Role::DeploymentManager => {
            verifier
                .issue_enrollment_code_for_deployment_token(&deployment)
                .await
        }
        super::Role::DeploymentGroupDeployer => {
            verifier
                .issue_enrollment_code_for_group_token(&deployment)
                .await
        }
        _ => verifier.issue_enrollment_code(&deployment).await,
    };
    match issued {
        Ok((code, expires_at)) => (
            [(http::header::CACHE_CONTROL, "no-store")],
            Json(IssuedEnrollmentCode {
                deployment_id: deployment.id,
                code,
                expires_at,
                manager_audience: credentials.audience().to_owned(),
            }),
        )
            .into_response(),
        Err(error) if error.retryable => {
            AlienError::new(ErrorData::AuthenticationUnavailable).into_response()
        }
        Err(error) => {
            if is_key_exists_refusal(&error) {
                record_refused_token_code(&state, &deployment.id, "issue").await;
            }
            error.into_response()
        }
    }
}

/// Issue a short-lived key for rerunning an installation, to its own Operator.
///
/// The caller proves itself with the Operator's live key as the bearer token.
/// It is not a session: the key is checked against this deployment only, so
/// every other caller gets the same refusal.
#[cfg_attr(feature = "openapi", utoipa::path(
    post, path = "/v1/deployments/{id}/setup-credential", operation_id = "issueSetupCredential",
    tag = "identity",
    params(("id" = String, Path, description = "Deployment ID")),
    responses((status = 200, body = IssuedSetupCredential), (status = 401), (status = 409), (status = 503))
))]
pub async fn issue_setup_credential(
    headers: HeaderMap,
    Path(id): Path<String>,
    Extension(verifier): Extension<Arc<dyn RuntimeIdentityVerifier>>,
) -> Response {
    let Some(key) = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|key| !key.is_empty())
    else {
        return ErrorData::unauthorized("Setup credential refused").into_response();
    };
    // Provider errors can carry the key. Never log or return their message.
    match verifier.issue_setup_credential(&id, key).await {
        Ok(issued) => ([(http::header::CACHE_CONTROL, "no-store")], Json(issued)).into_response(),
        Err(error) if error.retryable => {
            AlienError::new(ErrorData::AuthenticationUnavailable).into_response()
        }
        // Only the Operator's own key reaches this point, so saying why is no leak.
        Err(error) if error.code == "DEPLOYMENT_REMOVING" => {
            AlienError::new(ErrorData::DeploymentRemoving { deployment_id: id }).into_response()
        }
        Err(_) => ErrorData::unauthorized("Setup credential refused").into_response(),
    }
}

/// Whether `error` is the refusal of a token-made code for a deployment that
/// already has an operator key.
fn is_key_exists_refusal(error: &AlienError) -> bool {
    error.code == "ENROLLMENT_KEY_EXISTS"
}

/// Make a refused token-made code visible. Either the real operator was
/// refused after a copy of the token enrolled first, or a copy of the token
/// is trying to replace the key.
async fn record_refused_token_code(state: &AppState, deployment_id: &str, stage: &str) {
    tracing::warn!(
        %deployment_id,
        stage,
        "Refused an enrollment code made from a deployment token: the deployment already has an operator key"
    );
    state
        .log_buffer
        .push(crate::dev::LogEntry {
            timestamp: Utc::now(),
            deployment_id: deployment_id.to_owned(),
            body: format!(
                "Refused an enrollment code made from the deployment token ({stage}): \
                 the deployment already has an operator key"
            ),
            severity: "WARN".to_owned(),
            resource_name: None,
            attributes: vec![
                ("event".to_owned(), "enrollment-code-refused".to_owned()),
                ("stage".to_owned(), stage.to_owned()),
            ],
        })
        .await;
}

#[cfg(test)]
mod setup_credential_route_tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Accepts one key for one deployment and records what it was asked.
    struct OneKey;

    #[async_trait]
    impl RuntimeIdentityVerifier for OneKey {
        async fn verify(
            &self,
            _: &IdentityExchangeRequest,
            _: &str,
        ) -> Result<VerifiedRuntimeIdentity, AlienError> {
            Err(ErrorData::unauthorized("unused").into_generic())
        }

        async fn issue_setup_credential(
            &self,
            deployment_id: &str,
            operator_key: &str,
        ) -> Result<IssuedSetupCredential, AlienError> {
            if deployment_id == "dep-1" && operator_key == "ax_day_v1_live" {
                Ok(IssuedSetupCredential {
                    token: "ax_setup_minted".into(),
                    expires_at: Utc::now() + chrono::Duration::minutes(15),
                })
            } else {
                Err(ErrorData::unauthorized("a key with the secret in it").into_generic())
            }
        }
    }

    async fn call(deployment: &str, authorization: Option<&str>) -> (u16, String, Option<String>) {
        let verifier: Arc<dyn RuntimeIdentityVerifier> = Arc::new(OneKey);
        let app = Router::new()
            .route(
                "/v1/deployments/{id}/setup-credential",
                post(issue_setup_credential),
            )
            .layer(Extension(verifier));
        let mut request = Request::post(format!("/v1/deployments/{deployment}/setup-credential"));
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let cache = response
            .headers()
            .get("cache-control")
            .map(|v| v.to_str().unwrap().to_string());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned(), cache)
    }

    #[tokio::test]
    async fn only_the_live_operator_key_gets_a_key_and_the_answer_is_not_cached() {
        let (status, body, cache) = call("dep-1", Some("Bearer ax_day_v1_live")).await;
        assert_eq!(status, 200);
        assert!(body.contains("ax_setup_minted"));
        assert_eq!(cache.as_deref(), Some("no-store"));

        for (deployment, header) in [
            ("dep-1", None),
            ("dep-1", Some("Basic abc")),
            ("dep-1", Some("Bearer ")),
            ("dep-1", Some("Bearer ax_day_v1_other")),
            ("dep-2", Some("Bearer ax_day_v1_live")),
        ] {
            let (status, body, _) = call(deployment, header).await;
            assert_eq!(status, 401, "{deployment} {header:?}");
            assert!(
                !body.contains("secret"),
                "the verifier's message leaked: {body}"
            );
            assert!(!body.contains("ax_day_v1"), "a key was echoed: {body}");
        }
    }
}

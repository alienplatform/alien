//! Asking the apiserver who a bearer token belongs to.
//!
//! This is how a service verifies a caller that presented its own projected ServiceAccount
//! token. The alternative — issuing a token of our own and distributing it — creates a secret
//! that has to be rotated and torn down, where Kubernetes already mounts one in every pod.

use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::authentication::v1::TokenReview;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait TokenReviewsApi: Send + Sync + std::fmt::Debug {
    /// Submits a token for review and returns the apiserver's verdict.
    ///
    /// A successful call does **not** mean the token is valid: the verdict is in
    /// `status.authenticated`, and a rejected token comes back 200 with that flag false. Treating
    /// the HTTP status as the answer would authenticate everything.
    async fn create_token_review(&self, review: &TokenReview) -> Result<TokenReview>;
}

impl KubernetesClient {
    /// Submits a `TokenReview` to the apiserver.
    pub async fn create_token_review(&self, review: &TokenReview) -> Result<TokenReview> {
        let body = serde_json::to_string(review)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize TokenReview".to_string(),
            })?;

        let url = format!(
            "{}/apis/authentication.k8s.io/v1/tokenreviews",
            self.get_base_url()
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);

        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl TokenReviewsApi for KubernetesClient {
    async fn create_token_review(&self, review: &TokenReview) -> Result<TokenReview> {
        KubernetesClient::create_token_review(self, review).await
    }
}

/// Builds a review request for one bearer token.
pub fn review_for(token: &str) -> TokenReview {
    TokenReview {
        spec: k8s_openapi::api::authentication::v1::TokenReviewSpec {
            token: Some(token.to_string()),
            audiences: None,
        },
        ..Default::default()
    }
}

/// The authenticated username from a verdict, or `None` if the token was rejected.
///
/// `None` covers both "not authenticated" and "authenticated with no username", because a caller
/// we cannot name is a caller we cannot authorize.
pub fn authenticated_user(review: &TokenReview) -> Option<String> {
    let status = review.status.as_ref()?;
    if !status.authenticated.unwrap_or(false) {
        return None;
    }
    status.user.as_ref()?.username.clone()
}

/// Whether an authenticated username is a ServiceAccount in `namespace`.
///
/// Kubernetes formats these as `system:serviceaccount:<namespace>:<name>`. Matching the whole
/// prefix rather than searching for the namespace anywhere in the string: a namespace name can
/// appear inside a ServiceAccount name, and a substring match would accept the wrong tenant.
pub fn is_service_account_in(username: &str, namespace: &str) -> bool {
    username.starts_with(&format!("system:serviceaccount:{namespace}:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::authentication::v1::{TokenReviewStatus, UserInfo};

    fn verdict(authenticated: bool, username: Option<&str>) -> TokenReview {
        TokenReview {
            status: Some(TokenReviewStatus {
                authenticated: Some(authenticated),
                user: username.map(|name| UserInfo {
                    username: Some(name.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// A rejected token comes back 200 with `authenticated: false`. Reading the HTTP status as
    /// the answer would authenticate every caller.
    #[test]
    fn a_rejected_token_yields_no_user() {
        assert_eq!(
            authenticated_user(&verdict(false, Some("system:serviceaccount:ns:app"))),
            None
        );
        assert_eq!(authenticated_user(&verdict(true, None)), None);
        assert_eq!(
            authenticated_user(&verdict(true, Some("system:serviceaccount:ns:app"))).as_deref(),
            Some("system:serviceaccount:ns:app")
        );
    }

    /// A namespace name can appear inside a ServiceAccount name, so the check is on the whole
    /// prefix. A substring match would accept a caller from another tenant.
    #[test]
    fn namespace_matching_is_not_a_substring_search() {
        assert!(is_service_account_in(
            "system:serviceaccount:alien-app:worker",
            "alien-app"
        ));
        assert!(!is_service_account_in(
            "system:serviceaccount:other:alien-app",
            "alien-app"
        ));
        assert!(!is_service_account_in(
            "system:serviceaccount:alien-app-staging:worker",
            "alien-app"
        ));
        assert!(!is_service_account_in("system:node:node-1", "alien-app"));
    }
}

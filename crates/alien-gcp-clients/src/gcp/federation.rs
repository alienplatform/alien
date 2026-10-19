//! Renewable manager assertion -> customer pool -> target account credentials.

use std::time::Duration;

use alien_client_core::{ErrorData, Result};
use alien_core::{GcpClientConfig, GcpCredentials, GcpFederationSource};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use reqwest::{Client, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use url::Url;

use super::iam::{GenerateAccessTokenRequest, GenerateIdTokenRequest, IamApi, IamClient};
use super::GcpClientConfigExt;

const STS_ENDPOINT: &str = "https://sts.googleapis.com/v1/token";
const CLOUD_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Token values stay inside the authentication path and must not be logged.
pub(crate) struct FederatedAccessToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct StsResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
    issued_token_type: String,
}

#[derive(Deserialize)]
struct TokenRequestResponse {
    status: TokenRequestStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequestStatus {
    token: String,
    expiration_timestamp: DateTime<Utc>,
}

pub(crate) async fn exchange(
    config: &GcpClientConfig,
    http: &Client,
) -> Result<FederatedAccessToken> {
    let GcpCredentials::Federated {
        access,
        source,
        lifetime_seconds,
    } = &config.credentials
    else {
        return Err(invalid("Federated GCP credentials are required"));
    };
    access.validate().context(ErrorData::InvalidClientConfig {
        message: "GCP setup does not contain valid federation access; regenerate setup".to_string(),
        errors: None,
    })?;
    let lifetime = lifetime_seconds.unwrap_or(3600);
    if !(1..=3600).contains(&lifetime) {
        return Err(invalid(
            "Federated target token lifetime must be between 1 and 3600 seconds",
        ));
    }
    let audience = access
        .provider_audience()
        .context(ErrorData::InvalidClientConfig {
            message: "Cannot identify the deployment's federation provider".to_string(),
            errors: None,
        })?;
    let assertion = match source {
        GcpFederationSource::GoogleServiceAccount {
            source,
            service_account_email,
            service_account_unique_id,
        } => {
            if access.trust.issuer != "https://accounts.google.com"
                || &access.trust.subject != service_account_unique_id
            {
                return Err(identity_changed());
            }
            // Use the container caller directly. Impersonating the linked account
            // first would add a self-signing grant that the installation does not need.
            IamClient::new(http.clone(), (**source).clone())
                .generate_id_token(
                    service_account_email.clone(),
                    GenerateIdTokenRequest::builder()
                        .audience(access.assertion_audience.clone())
                        .build(),
                )
                .await?
                .token
        }
        GcpFederationSource::KubernetesServiceAccount {
            issuer,
            namespace,
            service_account,
            ..
        } => {
            if issuer != &access.trust.issuer
                || access.trust.subject
                    != format!("system:serviceaccount:{namespace}:{service_account}")
            {
                return Err(identity_changed());
            }
            kubernetes_assertion(source, &access.assertion_audience).await?
        }
    };
    if assertion.is_empty() {
        return Err(invalid("Manager assertion source returned an empty token"));
    }
    let sts_url = config.get_service_endpoint("sts", STS_ENDPOINT);
    let response = http
        .post(&sts_url)
        .form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            ),
            ("audience", &audience),
            (
                "requested_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            ),
            ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
            ("subject_token", &assertion),
            ("scope", CLOUD_SCOPE),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Cannot exchange manager assertion through the deployment's identity pool"
                .to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        // Credential endpoint errors must never retain the submitted assertion,
        // response body or a provider-controlled description in a loggable error.
        return Err(rejected(status, "Google STS rejected the manager assertion; check the setup's issuer, subject, audience and organization policy", STS_ENDPOINT));
    }
    let sts: StsResponse = token_response(response, "Google STS").await?;
    if sts.access_token.is_empty()
        || sts.expires_in == 0
        || !sts.token_type.eq_ignore_ascii_case("bearer")
        || sts.issued_token_type != "urn:ietf:params:oauth:token-type:access_token"
    {
        return Err(invalid("Google STS returned an unusable access token"));
    }
    let target = GcpClientConfig {
        project_id: config.project_id.clone(),
        region: config.region.clone(),
        project_number: Some(access.project_number.clone()),
        service_overrides: config.service_overrides.clone(),
        credentials: GcpCredentials::AccessToken {
            token: sts.access_token,
        },
    };
    let minted = IamClient::new(http.clone(), target)
        .generate_access_token(
            access.service_account_email.clone(),
            GenerateAccessTokenRequest::builder()
                .scope(vec![CLOUD_SCOPE.to_string()])
                .lifetime(format!("{lifetime}s"))
                .build(),
        )
        .await?;
    let expires_at = DateTime::parse_from_rfc3339(&minted.expire_time)
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Target account token has an invalid expiration time".to_string(),
        })?
        .with_timezone(&Utc);
    if minted.access_token.is_empty() || expires_at <= Utc::now() {
        return Err(invalid("Target account returned an empty or expired token"));
    }
    Ok(FederatedAccessToken {
        token: minted.access_token,
        expires_at,
    })
}

async fn kubernetes_assertion(source: &GcpFederationSource, audience: &str) -> Result<String> {
    let GcpFederationSource::KubernetesServiceAccount {
        api_server,
        api_token_file,
        ca_file,
        namespace,
        service_account,
        ..
    } = source
    else {
        return Err(invalid("Kubernetes assertion source is required"));
    };
    let endpoint =
        Url::parse(api_server)
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: "Manager Kubernetes API URL is invalid".to_string(),
                errors: None,
            })?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(invalid(
            "Manager Kubernetes API URL must be HTTPS without credentials, query or fragment",
        ));
    }
    // Read both files on every acquisition; kubelet replaces projected tokens.
    let api_token = std::fs::read_to_string(api_token_file)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Cannot read the manager's rotating Kubernetes API token".to_string(),
            errors: None,
        })?;
    let ca = std::fs::read(ca_file)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Cannot read the manager's Kubernetes API CA".to_string(),
            errors: None,
        })?;
    let certificate = reqwest::Certificate::from_pem(&ca)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Manager Kubernetes API CA is invalid".to_string(),
            errors: None,
        })?;
    let client = Client::builder()
        .add_root_certificate(certificate)
        .timeout(Duration::from_secs(30))
        .build()
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Cannot build manager Kubernetes assertion client".to_string(),
            errors: None,
        })?;
    let url = format!(
        "{}/api/v1/namespaces/{}/serviceaccounts/{}/token",
        api_server.trim_end_matches('/'),
        urlencoding::encode(namespace),
        urlencoding::encode(service_account)
    );
    let response = client
        .post(url)
        .bearer_auth(api_token.trim())
        .json(&serde_json::json!({
            "apiVersion": "authentication.k8s.io/v1",
            "kind": "TokenRequest",
            "spec": {"audiences": [audience], "expirationSeconds": 3600}
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Cannot request an audience-specific manager Kubernetes assertion".to_string(),
        })?;
    if !response.status().is_success() {
        return Err(rejected(response.status(), "Kubernetes rejected the manager's assertion request; allow TokenRequest only for its own service account", api_server));
    }
    let minted: TokenRequestResponse = token_response(response, "Kubernetes TokenRequest").await?;
    if minted.status.token.is_empty() || minted.status.expiration_timestamp <= Utc::now() {
        return Err(invalid("Kubernetes returned an empty or expired assertion"));
    }
    Ok(minted.status.token)
}

async fn token_response<T: DeserializeOwned>(response: Response, endpoint: &str) -> Result<T> {
    // A parser error may quote a credential value. Keep it out of error chains.
    response.json().await.map_err(|_| {
        AlienError::new(ErrorData::SerializationError {
            message: format!("{endpoint} returned an invalid credential response"),
        })
    })
}

fn rejected(status: StatusCode, message: &str, url: &str) -> AlienError<ErrorData> {
    let mut error = AlienError::new(ErrorData::HttpResponseError {
        message: message.to_string(),
        url: url.to_string(),
        http_status: status.as_u16(),
        http_request_text: None,
        http_response_text: None,
    });
    error.retryable = status.is_server_error() || matches!(status.as_u16(), 408 | 429);
    error
}

fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InvalidClientConfig {
        message: message.to_string(),
        errors: None,
    })
}

fn identity_changed() -> AlienError<ErrorData> {
    invalid("Selected manager identity differs from the deployment's federation trust; rerun setup with the selected manager")
}

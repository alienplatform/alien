use std::collections::HashMap;

use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use super::{expires_at_from_expires_in, ExpiringAccessToken};

pub(super) async fn generate_jwt_token_with_expiry(
    service_account_json: &str,
) -> Result<ExpiringAccessToken> {
    use jwt_simple::prelude::*;

    #[derive(serde::Deserialize)]
    struct ServiceAccountKey {
        client_email: String,
        private_key_id: String,
        private_key: String,
    }

    let service_account: ServiceAccountKey = serde_json::from_str(service_account_json)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: "Failed to parse service account JSON".to_string(),
            errors: None,
        })?;

    let mut extra = HashMap::new();
    extra.insert(
        "scope".to_string(),
        serde_json::Value::String("https://www.googleapis.com/auth/cloud-platform".to_string()),
    );
    let claims = Claims::with_custom_claims(extra, Duration::from_secs(3600))
        .with_issuer(&service_account.client_email)
        .with_subject(&service_account.client_email)
        .with_audience("https://oauth2.googleapis.com/token");

    let key_pair = RS256KeyPair::from_pem(&service_account.private_key)
        .map_err(|error| {
            AlienError::new(ErrorData::InvalidClientConfig {
                message: format!(
                    "Failed to parse private key from service account. Internal error: {error}"
                ),
                errors: None,
            })
        })?
        .with_key_id(&service_account.private_key_id);
    let assertion = key_pair.sign(claims).map_err(|error| {
        AlienError::new(ErrorData::RequestSignError {
            message: format!("Failed to sign JWT token: {error}"),
        })
    })?;

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: i64,
    }

    let response = Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &assertion),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to exchange JWT for access token".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!("OAuth2 token exchange failed with status {status}: {error_text}"),
            url: "https://oauth2.googleapis.com/token".to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }
    let token_response: TokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to parse OAuth2 token response".to_string(),
            })?;
    Ok(ExpiringAccessToken {
        token: token_response.access_token,
        expires_at: expires_at_from_expires_in("GCP OAuth2", token_response.expires_in)?,
    })
}

pub(super) async fn fetch_metadata_token_with_expiry() -> Result<ExpiringAccessToken> {
    const URL: &str = "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: i64,
    }

    let response = Client::new()
        .get(URL)
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to fetch token from GCP metadata server".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!("Metadata server returned error {status}: {error_text}"),
            url: URL.to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }
    let token_response: TokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to parse token response from GCP metadata server".to_string(),
            })?;
    Ok(ExpiringAccessToken {
        token: token_response.access_token,
        expires_at: expires_at_from_expires_in("GCP metadata", token_response.expires_in)?,
    })
}

pub(super) async fn exchange_refresh_token_with_expiry(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<ExpiringAccessToken> {
    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: i64,
    }

    let response = Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to exchange refresh token for access token".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!("OAuth2 token exchange failed with status {status}: {error_text}"),
            url: "https://oauth2.googleapis.com/token".to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }
    let token_response: TokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to parse OAuth2 token exchange response".to_string(),
            })?;
    Ok(ExpiringAccessToken {
        token: token_response.access_token,
        expires_at: expires_at_from_expires_in("GCP OAuth2", token_response.expires_in)?,
    })
}

pub(super) async fn exchange_external_account_token_with_expiry(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Result<ExpiringAccessToken> {
    #[derive(Deserialize)]
    struct StsTokenResponse {
        access_token: String,
        expires_in: i64,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ImpersonationTokenResponse {
        access_token: String,
        expire_time: String,
    }

    let subject_token = std::fs::read_to_string(credential_source_file)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!(
                "Failed to read external account subject token from: {credential_source_file}"
            ),
            errors: None,
        })?
        .trim()
        .to_string();
    let scope = "https://www.googleapis.com/auth/cloud-platform";
    let client = Client::new();
    let response = client
        .post(token_url)
        .form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            ),
            ("audience", audience),
            (
                "requested_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            ),
            ("subject_token_type", subject_token_type),
            ("subject_token", &subject_token),
            ("scope", scope),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to exchange external account token".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "External account token exchange failed with status {status}: {error_text}"
            ),
            url: token_url.to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }
    let sts_token: StsTokenResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to parse external account token exchange response".to_string(),
            })?;

    let Some(impersonation_url) = service_account_impersonation_url else {
        return Ok(ExpiringAccessToken {
            token: sts_token.access_token,
            expires_at: expires_at_from_expires_in("GCP STS", sts_token.expires_in)?,
        });
    };
    let response = client
        .post(impersonation_url)
        .bearer_auth(&sts_token.access_token)
        .json(&serde_json::json!({
            "scope": [scope],
            "lifetime": "3600s",
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: "Failed to impersonate external account service account".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "External account service account impersonation failed with status {status}: {error_text}"
            ),
            url: impersonation_url.to_string(),
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(error_text),
        }));
    }
    let token_response: ImpersonationTokenResponse = response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to parse service account impersonation response".to_string(),
        })?;
    let expires_at = DateTime::parse_from_rfc3339(&token_response.expire_time)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            message: "GCP returned an invalid external-account token expiry".to_string(),
            field_name: None,
        })?
        .with_timezone(&Utc);
    Ok(ExpiringAccessToken {
        token: token_response.access_token,
        expires_at,
    })
}

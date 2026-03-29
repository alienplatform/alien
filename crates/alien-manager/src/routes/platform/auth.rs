use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::Subject;
use axum::http::HeaderMap;

use crate::providers::platform_api::error::{ErrorData, Result};

/// Authenticate the caller by forwarding the bearer token to `/v1/whoami`.
pub async fn resolve_subject(alien_api_url: &str, headers: &HeaderMap) -> Result<Subject> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: "Missing Authorization header".to_string(),
            })
        })?;

    let auth_value =
        auth_header
            .to_str()
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                message: "Invalid Authorization header".to_string(),
            })?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/v1/whoami", alien_api_url))
        .header(axum::http::header::AUTHORIZATION, auth_value)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "Failed to authenticate request".to_string(),
        })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: format!("Authentication failed with status {}", response.status()),
        }));
    }

    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "Failed to parse authentication response".to_string(),
        })
}

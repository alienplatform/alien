use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

use super::error::{ErrorData, Result};

/// Extract Bearer token from Authorization header.
pub fn extract_bearer_token(headers: &HeaderMap) -> Result<String> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: "Missing Authorization header".to_string(),
            })
        })?;

    let auth_str =
        auth_header
            .to_str()
            .into_alien_error()
            .context(ErrorData::AuthenticationFailed {
                message: "Invalid Authorization header".to_string(),
            })?;

    auth_str
        .strip_prefix("Bearer ")
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: "Authorization header must use Bearer scheme".to_string(),
            })
        })
}

/// Create an alien-platform-api Client with a specific Bearer token (for token passthrough).
pub fn create_client_with_token(api_url: &str, token: &str) -> Result<alien_platform_api::Client> {
    let auth_value = format!("Bearer {}", token);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-manager"));

    let reqwest_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to build HTTP client".to_string(),
        })?;

    Ok(alien_platform_api::Client::new_with_client(
        api_url,
        reqwest_client,
    ))
}

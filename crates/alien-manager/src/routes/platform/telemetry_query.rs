use std::sync::Arc;

use alien_error::{AlienError, Context, IntoAlienError};
use axum::{extract::Extension, http::HeaderMap, Json};
use jwt_simple::prelude::*;
use serde_json::Value as JsonValue;

use crate::providers::platform_api::{
    error::{ErrorData, Result},
    PlatformState,
};

pub async fn search_logs(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>> {
    let deepstore_url = ext.deepstore.query_url.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeepStore query URL not configured".to_string(),
        })
    })?;

    let public_key = ext
        .deepstore.jwt_public_key
        .as_deref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "DEEPSTORE_JWT_PUBLIC_KEY is required for query endpoints".to_string(),
            })
        })?;
    let scopes = validate_deepstore_query_jwt(&headers, public_key)?;

    if scopes.is_empty() {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "No scopes available for search request".to_string(),
        }));
    }

    let allowed_scopes = scopes.join(",");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/search", deepstore_url))
        .header("Content-Type", "application/json")
        .header("X-Allowed-Scopes", allowed_scopes)
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to forward search to DeepStore".to_string(),
        })?;

    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AlienError::new(ErrorData::TelemetryFailed {
            message: format!("DeepStore search error {}: {}", status, response_body),
        }));
    }

    let json: JsonValue = serde_json::from_str(&response_body)
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to parse DeepStore search response".to_string(),
        })?;
    Ok(Json(json))
}

pub async fn field_capabilities(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>> {
    let deepstore_url = ext.deepstore.query_url.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeepStore query URL not configured".to_string(),
        })
    })?;

    let public_key = ext
        .deepstore.jwt_public_key
        .as_deref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "DEEPSTORE_JWT_PUBLIC_KEY is required for query endpoints".to_string(),
            })
        })?;
    let scopes = validate_deepstore_query_jwt(&headers, public_key)?;

    if scopes.is_empty() {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "No scopes available for field capabilities request".to_string(),
        }));
    }

    let allowed_scopes = scopes.join(",");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/field-capabilities", deepstore_url))
        .header("Content-Type", "application/json")
        .header("X-Allowed-Scopes", allowed_scopes)
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to forward field capabilities to DeepStore".to_string(),
        })?;

    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AlienError::new(ErrorData::TelemetryFailed {
            message: format!(
                "DeepStore field capabilities error {}: {}",
                status, response_body
            ),
        }));
    }

    let json: JsonValue = serde_json::from_str(&response_body)
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to parse DeepStore field capabilities response".to_string(),
        })?;
    Ok(Json(json))
}

pub async fn fetch_draft_documents(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Json(body): Json<JsonValue>,
) -> Result<Json<JsonValue>> {
    let deepstore_url = ext.deepstore.query_url.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "DeepStore query URL not configured".to_string(),
        })
    })?;

    let public_key = ext
        .deepstore.jwt_public_key
        .as_deref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "DEEPSTORE_JWT_PUBLIC_KEY is required for query endpoints".to_string(),
            })
        })?;
    let scopes = validate_deepstore_query_jwt(&headers, public_key)?;

    if scopes.is_empty() {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "No scopes available for draft documents request".to_string(),
        }));
    }

    let allowed_scopes = scopes.join(",");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/drafts/documents", deepstore_url))
        .header("Content-Type", "application/json")
        .header("X-Allowed-Scopes", allowed_scopes)
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to forward draft documents to DeepStore".to_string(),
        })?;

    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(AlienError::new(ErrorData::TelemetryFailed {
            message: format!(
                "DeepStore draft document error {}: {}",
                status, response_body
            ),
        }));
    }

    let json: JsonValue = serde_json::from_str(&response_body)
        .into_alien_error()
        .context(ErrorData::TelemetryFailed {
            message: "Failed to parse DeepStore draft documents response".to_string(),
        })?;

    Ok(Json(json))
}

fn validate_deepstore_query_jwt(headers: &HeaderMap, public_key_pem: &str) -> Result<Vec<String>> {
    #[derive(Serialize, Deserialize)]
    struct QueryJwtClaims {
        #[serde(default)]
        scopes: Vec<String>,
    }

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

    let token = auth_value.strip_prefix("Bearer ").ok_or_else(|| {
        AlienError::new(ErrorData::AuthenticationFailed {
            message: "Invalid Authorization header format".to_string(),
        })
    })?;

    let public_key = RS256PublicKey::from_pem(public_key_pem).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Invalid DeepStore JWT public key: {}", e),
        })
    })?;

    let claims = public_key
        .verify_token::<QueryJwtClaims>(token, None)
        .map_err(|e| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: format!("Invalid or expired DeepStore JWT: {}", e),
            })
        })?;

    let custom_claims = claims.custom;

    if custom_claims.scopes.is_empty() {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "DeepStore JWT missing scopes claim".to_string(),
        }));
    }

    Ok(custom_claims.scopes)
}

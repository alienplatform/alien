use std::sync::Arc;

use alien_error::{AlienError, Context, IntoAlienError};
use axum::{
    extract::{Extension, Query},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::providers::platform_api::{
    error::{ErrorData, Result},
    PlatformState,
};

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub error: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadataRequest {
    pub access_token: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadataResponse {
    pub project_id: String,
    pub project_number: String,
    pub name: String,
    pub has_permissions: bool,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_in: u64,
    #[allow(dead_code)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GcpProjectMetadata {
    project_id: String,
    project_number: String,
    name: String,
}

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GCP_RESOURCE_MANAGER_URL: &str = "https://cloudresourcemanager.googleapis.com/v1/projects";

const OAUTH_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform.read-only",
    "https://www.googleapis.com/auth/cloudplatformprojects.readonly",
];

pub async fn google_cloud_login(
    Extension(ext): Extension<Arc<PlatformState>>,
) -> Result<Response> {
    let client_id = ext.gcp_oauth.client_id.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "GCP OAuth client ID not configured".to_string(),
        })
    })?;

    let callback_url = format!("{}/v1/google-cloud-login/callback", ext.base_url);

    let scopes = OAUTH_SCOPES.join(" ");
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
        GOOGLE_AUTH_URL,
        urlencoding::encode(client_id),
        urlencoding::encode(&callback_url),
        urlencoding::encode(&scopes),
    );

    info!("Redirecting to Google OAuth");
    Ok(Redirect::temporary(&auth_url).into_response())
}

pub async fn google_cloud_callback(
    Extension(ext): Extension<Arc<PlatformState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response> {
    if let Some(error) = query.error {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: format!("Google OAuth error: {}", error),
        }));
    }

    let code = query.code.ok_or_else(|| {
        AlienError::new(ErrorData::AuthenticationFailed {
            message: "No authorization code provided".to_string(),
        })
    })?;

    let client_id = ext.gcp_oauth.client_id.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "GCP OAuth client ID not configured".to_string(),
        })
    })?;

    let client_secret = ext
        .gcp_oauth.client_secret
        .as_ref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "GCP OAuth client secret not configured".to_string(),
            })
        })?;

    let callback_url = format!("{}/v1/google-cloud-login/callback", ext.base_url);

    let client = reqwest::Client::new();
    let token_response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", callback_url.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "Failed to exchange authorization code".to_string(),
        })?;

    if !token_response.status().is_success() {
        let error_text = token_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: format!("Token exchange failed: {}", error_text),
        }));
    }

    let tokens: GoogleTokenResponse = token_response.json().await.into_alien_error().context(
        ErrorData::AuthenticationFailed {
            message: "Failed to parse token response".to_string(),
        },
    )?;

    info!("Successfully obtained GCP access token");

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head><title>GCP OAuth Success</title></head>
<body>
<script>
    window.opener && window.opener.postMessage({{ type: 'gcp-oauth-success', accessToken: '{}' }}, '{}');
    window.close();
</script>
<p>Authentication successful. You can close this window.</p>
</body>
</html>"#,
        tokens.access_token, ext.base_url
    );

    Ok(axum::response::Html(html).into_response())
}

pub async fn get_project_metadata(
    Json(request): Json<ProjectMetadataRequest>,
) -> Result<Json<ProjectMetadataResponse>> {
    info!(project_id = %request.project_id, "Fetching GCP project metadata");

    let client = reqwest::Client::new();

    let project_url = format!("{}/{}", GCP_RESOURCE_MANAGER_URL, request.project_id);
    let response = client
        .get(&project_url)
        .header("Authorization", format!("Bearer {}", request.access_token))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::StackOperationFailed {
            operation: "fetch project metadata".to_string(),
        })?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "Access token is invalid or expired".to_string(),
        }));
    }

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AlienError::new(ErrorData::StackOperationFailed {
            operation: format!("GCP project '{}' not found", request.project_id),
        }));
    }

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::StackOperationFailed {
            operation: format!("fetch project metadata: {}", error_text),
        }));
    }

    let metadata: GcpProjectMetadata =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::StackOperationFailed {
                operation: "parse project metadata".to_string(),
            })?;

    info!(
        project_id = %metadata.project_id,
        project_number = %metadata.project_number,
        "Successfully retrieved GCP project metadata"
    );

    Ok(Json(ProjectMetadataResponse {
        project_id: metadata.project_id,
        project_number: metadata.project_number,
        name: metadata.name,
        has_permissions: true,
    }))
}

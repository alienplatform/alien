use alien_core::GcpClientConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use google_cloud_auth::credentials::CacheableResource;
use http::{Extensions, HeaderMap};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::core::gcp_credentials_from_alien_config;
use crate::error::{ErrorData, Result};

pub(crate) async fn insert_notification(
    config: &GcpClientConfig,
    bucket_name: &str,
    notification: serde_json::Value,
) -> Result<serde_json::Value> {
    let url = format!(
        "{}/b/{}/notificationConfigs",
        gcs_rest_endpoint(config),
        bucket_name
    );
    send_gcs_json(
        config,
        reqwest::Client::new().post(url),
        "insert_notification",
        Some(bucket_name.to_string()),
        Some(&notification),
    )
    .await
}

pub(crate) async fn list_notifications(
    config: &GcpClientConfig,
    bucket_name: &str,
) -> Result<Vec<serde_json::Value>> {
    let url = format!(
        "{}/b/{}/notificationConfigs",
        gcs_rest_endpoint(config),
        bucket_name
    );
    let response: serde_json::Value = send_gcs_json(
        config,
        reqwest::Client::new().get(url),
        "list_notifications",
        Some(bucket_name.to_string()),
        Option::<&()>::None,
    )
    .await?;

    Ok(response
        .get("items")
        .and_then(|items| items.as_array())
        .cloned()
        .unwrap_or_default())
}

pub(crate) async fn delete_notification(
    config: &GcpClientConfig,
    bucket_name: &str,
    notification_id: &str,
) -> Result<()> {
    let url = format!(
        "{}/b/{}/notificationConfigs/{}",
        gcs_rest_endpoint(config),
        bucket_name,
        notification_id
    );
    send_gcs_empty(
        config,
        reqwest::Client::new().delete(url),
        "delete_notification",
        Some(bucket_name.to_string()),
    )
    .await
}

fn gcs_rest_endpoint(config: &GcpClientConfig) -> String {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("storage"))
        .map(|endpoint| endpoint.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "https://storage.googleapis.com/storage/v1".to_string())
}

async fn auth_headers(config: &GcpClientConfig, resource_id: Option<String>) -> Result<HeaderMap> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    match credentials
        .headers(Extensions::new())
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to get GCP Cloud Storage authorization headers".to_string(),
            resource_id: resource_id.clone(),
        })? {
        CacheableResource::New { data, .. } => Ok(data),
        CacheableResource::NotModified => Err(AlienError::new(
            ErrorData::CloudPlatformError {
                message: "GCP Cloud Storage authorization headers were not refreshed and no cached headers are available".to_string(),
                resource_id,
            },
        )),
    }
}

async fn send_gcs_json<T, B>(
    config: &GcpClientConfig,
    request: reqwest::RequestBuilder,
    operation: &str,
    resource_id: Option<String>,
    body: Option<&B>,
) -> Result<T>
where
    T: DeserializeOwned,
    B: Serialize + ?Sized,
{
    let headers = auth_headers(config, resource_id.clone()).await?;
    let request = request.headers(headers);
    let request = if let Some(body) = body {
        request.json(body)
    } else {
        request
    };
    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("GCS {operation} request failed"),
                resource_id: resource_id.clone(),
            })?;

    if !response.status().is_success() {
        return Err(gcs_http_error(operation, resource_id, response).await);
    }

    response
        .json::<T>()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to parse GCS {operation} response"),
            resource_id,
        })
}

async fn send_gcs_empty(
    config: &GcpClientConfig,
    request: reqwest::RequestBuilder,
    operation: &str,
    resource_id: Option<String>,
) -> Result<()> {
    let headers = auth_headers(config, resource_id.clone()).await?;
    let response = request
        .headers(headers)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("GCS {operation} request failed"),
            resource_id: resource_id.clone(),
        })?;

    if !response.status().is_success() {
        return Err(gcs_http_error(operation, resource_id, response).await);
    }

    Ok(())
}

async fn gcs_http_error(
    operation: &str,
    resource_id: Option<String>,
    response: reqwest::Response,
) -> AlienError<ErrorData> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == StatusCode::NOT_FOUND {
        AlienError::new(ErrorData::CloudResourceNotFound {
            resource_type: format!("GCS {operation}"),
            resource_name: resource_id.unwrap_or_else(|| "unknown".to_string()),
        })
    } else if status == StatusCode::CONFLICT {
        AlienError::new(ErrorData::CloudResourceConflict {
            resource_type: format!("GCS {operation}"),
            resource_name: resource_id.unwrap_or_else(|| "unknown".to_string()),
            message: format!("HTTP {}: {text}", status.as_u16()),
        })
    } else {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("GCS {operation} returned HTTP {}: {text}", status.as_u16()),
            resource_id,
        })
    }
}

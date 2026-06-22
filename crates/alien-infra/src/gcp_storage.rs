use alien_core::GcpClientConfig;
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError, IntoAlienErrorDirect};
use google_cloud_auth::credentials::CacheableResource;
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_v1::model::Policy;
use google_cloud_storage::{
    client::StorageControl,
    model::{Bucket, DeleteObjectRequest},
};
use http::{Extensions, HeaderMap};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::core::gcp_credentials_from_alien_config;
use crate::error::{ErrorData, Result};

pub(crate) async fn create_bucket(
    client: &StorageControl,
    project_id: &str,
    bucket_name: &str,
    bucket: Bucket,
) -> Result<Bucket> {
    match client
        .create_bucket()
        .set_parent(format!("projects/{project_id}"))
        .set_bucket_id(bucket_name)
        .set_bucket(bucket)
        .send()
        .await
    {
        Ok(bucket) => Ok(bucket),
        Err(error) if gax_error_is_conflict(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceConflict {
                resource_type: "GCS bucket".to_string(),
                resource_name: bucket_name.to_string(),
                message: "create_bucket reported the bucket already exists".to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "GCS create_bucket request failed".to_string(),
                resource_id: Some(bucket_name.to_string()),
            })),
    }
}

pub(crate) async fn get_bucket(client: &StorageControl, bucket_name: &str) -> Result<Bucket> {
    match client
        .get_bucket()
        .set_name(bucket_resource_name(bucket_name))
        .send()
        .await
    {
        Ok(bucket) => Ok(bucket),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCS bucket".to_string(),
                resource_name: bucket_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "GCS get_bucket request failed".to_string(),
                resource_id: Some(bucket_name.to_string()),
            })),
    }
}

pub(crate) async fn update_bucket(
    client: &StorageControl,
    bucket_name: &str,
    bucket_patch: Bucket,
) -> Result<Bucket> {
    let update_mask = bucket_update_mask(&bucket_patch);
    let mut bucket_patch = bucket_patch;
    if bucket_patch.name.is_empty() {
        bucket_patch.name = bucket_resource_name(bucket_name);
    }

    let mut request =
        google_cloud_storage::model::UpdateBucketRequest::new().set_bucket(bucket_patch);
    if !update_mask.paths.is_empty() {
        request = request.set_update_mask(update_mask);
    }

    match client.update_bucket().with_request(request).send().await {
        Ok(bucket) => Ok(bucket),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCS bucket".to_string(),
                resource_name: bucket_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "GCS update_bucket request failed".to_string(),
                resource_id: Some(bucket_name.to_string()),
            })),
    }
}

pub(crate) async fn delete_bucket(client: &StorageControl, bucket_name: &str) -> Result<()> {
    match client
        .delete_bucket()
        .set_name(bucket_resource_name(bucket_name))
        .send()
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCS bucket".to_string(),
                resource_name: bucket_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "GCS delete_bucket request failed".to_string(),
                resource_id: Some(bucket_name.to_string()),
            })),
    }
}

pub(crate) async fn get_bucket_iam_policy(
    client: &StorageControl,
    bucket_name: &str,
) -> Result<Policy> {
    client
        .get_iam_policy()
        .set_resource(bucket_resource_name(bucket_name))
        .send()
        .await
        .map_err(|error| {
            if gax_error_is_not_found(&error) {
                AlienError::new(ErrorData::CloudResourceNotFound {
                    resource_type: "GCS bucket IAM policy".to_string(),
                    resource_name: bucket_name.to_string(),
                })
            } else {
                error
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "GCS get_bucket_iam_policy request failed".to_string(),
                        resource_id: Some(bucket_name.to_string()),
                    })
            }
        })
}

pub(crate) async fn set_bucket_iam_policy(
    client: &StorageControl,
    bucket_name: &str,
    iam_policy: Policy,
) -> Result<Policy> {
    client
        .set_iam_policy()
        .set_resource(bucket_resource_name(bucket_name))
        .set_policy(iam_policy)
        .send()
        .await
        .map_err(|error| {
            if gax_error_is_not_found(&error) {
                AlienError::new(ErrorData::CloudResourceNotFound {
                    resource_type: "GCS bucket IAM policy".to_string(),
                    resource_name: bucket_name.to_string(),
                })
            } else {
                error
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "GCS set_bucket_iam_policy request failed".to_string(),
                        resource_id: Some(bucket_name.to_string()),
                    })
            }
        })
}

pub(crate) async fn empty_bucket(client: &StorageControl, bucket_name: &str) -> Result<()> {
    let mut page_token = String::new();
    loop {
        let response = match client
            .list_objects()
            .set_parent(bucket_resource_name(bucket_name))
            .set_page_size(1000)
            .set_page_token(page_token.clone())
            .set_versions(true)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) if gax_error_is_not_found(&error) => return Ok(()),
            Err(error) => {
                return Err(error
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: "GCS list_objects request failed while emptying bucket"
                            .to_string(),
                        resource_id: Some(bucket_name.to_string()),
                    }));
            }
        };

        for object in response.objects {
            let generation = if object.generation == 0 {
                None
            } else {
                Some(object.generation)
            };
            let mut request = DeleteObjectRequest::new()
                .set_bucket(bucket_resource_name(bucket_name))
                .set_object(object.name.clone());
            if let Some(generation) = generation {
                request = request.set_generation(generation);
            }
            match client.delete_object().with_request(request).send().await {
                Ok(()) => {}
                Err(error) if gax_error_is_not_found(&error) => {}
                Err(error) => {
                    return Err(error
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                            "GCS delete_object request failed while emptying bucket object '{}'",
                            object.name
                        ),
                            resource_id: Some(bucket_name.to_string()),
                        }));
                }
            }
        }

        if response.next_page_token.is_empty() {
            break;
        }
        page_token = response.next_page_token;
    }

    Ok(())
}

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

fn bucket_resource_name(bucket_name: &str) -> String {
    if bucket_name.starts_with("projects/") {
        bucket_name.to_string()
    } else {
        format!("projects/_/buckets/{bucket_name}")
    }
}

fn bucket_update_mask(bucket: &Bucket) -> wkt::FieldMask {
    let mut paths = Vec::new();
    if bucket.versioning.is_some() {
        paths.push("versioning".to_string());
    }
    if bucket.lifecycle.is_some() {
        paths.push("lifecycle".to_string());
    }
    if bucket.iam_config.is_some() {
        paths.push("iam_config".to_string());
    }
    if !bucket.labels.is_empty() {
        paths.push("labels".to_string());
    }
    wkt::FieldMask::default().set_paths(paths)
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

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::CONFLICT.as_u16())
}

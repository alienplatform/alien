use crate::traits::{CreateReleaseParams, ReleaseRecord, ReleaseStore};
use alien_error::{AlienError, GenericError, IntoAlienError};
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;

/// Convert a value to/from another serde-compatible type via JSON round-trip.
fn convert_via_json<T: serde::Serialize, U: serde::de::DeserializeOwned>(
    value: &T,
) -> Result<U, AlienError> {
    let json = serde_json::to_value(value)
        .into_alien_error()
        .map_err(|e| {
            AlienError::new(GenericError {
                message: format!("JSON serialize failed: {}", e),
            })
        })?;
    serde_json::from_value(json).map_err(|e| {
        AlienError::new(GenericError {
            message: format!("JSON deserialize failed: {}", e),
        })
    })
}

fn is_not_found(e: &AlienError) -> bool {
    e.http_status_code == Some(404) || e.code.to_uppercase().contains("NOT_FOUND")
}

/// Delegates `ReleaseStore` operations to the Platform API release endpoints.
pub struct PlatformApiReleaseStore {
    platform_client: alien_platform_api::Client,
}

impl PlatformApiReleaseStore {
    pub fn new(platform_client: alien_platform_api::Client) -> Self {
        Self { platform_client }
    }
}

#[async_trait]
impl ReleaseStore for PlatformApiReleaseStore {
    async fn create_release(
        &self,
        params: CreateReleaseParams,
    ) -> Result<ReleaseRecord, AlienError> {
        let body: alien_platform_api::types::CreateReleaseRequest =
            convert_via_json(&serde_json::json!({
                "stack": params.stack,
                "platform": params.platform.map(|p| p.as_str()),
                "gitMetadata": {
                    "commitSha": params.git_commit_sha,
                    "commitRef": params.git_commit_ref,
                    "commitMessage": params.git_commit_message,
                },
            }))?;

        let release = self
            .platform_client
            .create_release()
            .body(body)
            .send()
            .await
            .into_sdk_error()?;

        convert_via_json(&*release)
    }

    async fn get_release(&self, id: &str) -> Result<Option<ReleaseRecord>, AlienError> {
        let result = self
            .platform_client
            .get_release()
            .id(id)
            .send()
            .await
            .into_sdk_error();

        match result {
            Ok(release) => Ok(Some(convert_via_json(&*release)?)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn get_latest_release(&self) -> Result<Option<ReleaseRecord>, AlienError> {
        let response = self
            .platform_client
            .list_releases()
            .send()
            .await
            .into_sdk_error()?;

        match response.items.first() {
            Some(release) => Ok(Some(convert_via_json(release)?)),
            None => Ok(None),
        }
    }
}

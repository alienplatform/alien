use crate::error::{ErrorData, Result};
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use google_cloud_run_v2::client::Services;

pub(crate) async fn cloud_run_services_from_alien_config(
    config: &GcpClientConfig,
) -> Result<Services> {
    let credentials = crate::core::gcp_credentials_from_alien_config(config).map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("Failed to build GCP Cloud Run credentials: {error}"),
            resource_id: None,
        })
    })?;
    let mut builder = Services::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("cloudrun"))
    {
        builder = builder.with_endpoint(endpoint.trim_end_matches('/').to_string());
    }

    builder.build().await.map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("Failed to build official GCP Cloud Run client: {error}"),
            resource_id: None,
        })
    })
}

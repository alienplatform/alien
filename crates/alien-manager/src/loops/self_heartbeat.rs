use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

use alien_core::ManagementConfig;
use alien_platform_api::SdkResultExt;

use crate::providers::platform_api::{
    error::{ErrorData, Result},
    PlatformState,
};

/// Run the self-heartbeat loop that reports manager health to the Platform API.
///
/// This loop runs indefinitely and is cancelled automatically when the tokio
/// runtime (or the spawned task) is dropped.
pub async fn run_self_heartbeat_loop(ext: Arc<PlatformState>) -> Result<()> {
    let interval = Duration::from_secs(ext.heartbeat_interval_secs);

    info!(
        interval_secs = ext.heartbeat_interval_secs,
        "Starting self-heartbeat loop"
    );

    // Send first heartbeat immediately
    match report_heartbeat(&ext).await {
        Ok(()) => {
            debug!("Initial self-heartbeat reported successfully");
        }
        Err(e) => {
            error!(error = %e, "Failed to report initial self-heartbeat");
        }
    }

    loop {
        tokio::time::sleep(interval).await;
        match report_heartbeat(&ext).await {
            Ok(()) => {
                debug!("Self-heartbeat reported successfully");
            }
            Err(e) => {
                error!(error = %e, "Failed to report self-heartbeat");
            }
        }
    }
}

async fn load_management_config(ext: &PlatformState) -> Result<Option<ManagementConfig>> {
    use alien_bindings::ServiceAccountInfo;

    let service_account = match ext.bindings.load_service_account("management").await {
        Ok(sa) => sa,
        Err(_) => {
            debug!("No management ServiceAccount binding found");
            return Ok(None);
        }
    };

    let info = service_account.get_info().await.map_err(|e| {
        alien_error::AlienError::new(ErrorData::SelfHeartbeatFailed {
            message: format!("Failed to get service account info: {}", e),
        })
    })?;

    let management_config = match info {
        ServiceAccountInfo::Aws(aws_info) => {
            Some(ManagementConfig::Aws(alien_core::AwsManagementConfig {
                managing_role_arn: aws_info.role_arn,
            }))
        }
        ServiceAccountInfo::Gcp(gcp_info) => {
            Some(ManagementConfig::Gcp(alien_core::GcpManagementConfig {
                service_account_email: gcp_info.email,
            }))
        }
        ServiceAccountInfo::Azure(azure_info) => {
            let tenant_id = match std::env::var("AZURE_TENANT_ID") {
                Ok(id) => id,
                Err(_) => {
                    debug!(
                        "AZURE_TENANT_ID environment variable not set, skipping ManagementConfig"
                    );
                    return Ok(None);
                }
            };
            let oidc_issuer = std::env::var("AZURE_MANAGEMENT_OIDC_ISSUER")
                .ok()
                .filter(|value| !value.is_empty());
            let oidc_subject = std::env::var("AZURE_MANAGEMENT_OIDC_SUBJECT")
                .ok()
                .filter(|value| !value.is_empty());
            let management_principal_id = if oidc_issuer.is_none() {
                Some(azure_info.principal_id)
            } else {
                None
            };

            Some(ManagementConfig::Azure(alien_core::AzureManagementConfig {
                managing_tenant_id: tenant_id,
                oidc_issuer,
                oidc_subject,
                management_principal_id,
            }))
        }
    };

    Ok(management_config)
}

fn convert_management_config_to_sdk(
    config: ManagementConfig,
) -> alien_platform_api::types::ManagerHeartbeatRequestManagementConfig {
    use alien_platform_api::types::{
        ManagerHeartbeatRequestManagementConfig,
        ManagerHeartbeatRequestManagementConfigVariant0Platform,
        ManagerHeartbeatRequestManagementConfigVariant1Platform,
        ManagerHeartbeatRequestManagementConfigVariant2Platform,
    };

    match config {
        ManagementConfig::Aws(aws_config) => {
            ManagerHeartbeatRequestManagementConfig::Variant0 {
                managing_role_arn: aws_config.managing_role_arn,
                platform: ManagerHeartbeatRequestManagementConfigVariant0Platform::Aws,
            }
        }
        ManagementConfig::Gcp(gcp_config) => {
            ManagerHeartbeatRequestManagementConfig::Variant1 {
                service_account_email: gcp_config.service_account_email,
                platform: ManagerHeartbeatRequestManagementConfigVariant1Platform::Gcp,
            }
        }
        ManagementConfig::Azure(azure_config) => {
            ManagerHeartbeatRequestManagementConfig::Variant2 {
                managing_tenant_id: azure_config.managing_tenant_id,
                management_principal_id: azure_config
                    .management_principal_id
                    .unwrap_or_else(|| "oidc".to_string()),
                platform: ManagerHeartbeatRequestManagementConfigVariant2Platform::Azure,
            }
        }
        ManagementConfig::Kubernetes => {
            ManagerHeartbeatRequestManagementConfig::Variant3 {
                platform: alien_platform_api::types::ManagerHeartbeatRequestManagementConfigVariant3Platform::Kubernetes,
            }
        }
    }
}

async fn report_heartbeat(ext: &PlatformState) -> Result<()> {
    let management_config = load_management_config(ext).await?;

    let metrics_builder = alien_platform_api::types::ManagerHeartbeatRequestMetrics::builder()
        .active_deployments(None);

    let metrics =
        alien_platform_api::types::ManagerHeartbeatRequestMetrics::try_from(metrics_builder)
            .map_err(|e| {
                alien_error::AlienError::new(ErrorData::SelfHeartbeatFailed {
                    message: format!("Failed to build metrics: {}", e),
                })
            })?;

    let mut request_builder = alien_platform_api::types::ManagerHeartbeatRequest::builder()
        .status(alien_platform_api::types::ManagerHeartbeatRequestStatus::Healthy)
        .url(ext.base_url.to_string())
        .version(Some(env!("CARGO_PKG_VERSION").to_string()))
        .metrics(Some(metrics));

    if let Some(config) = management_config {
        request_builder =
            request_builder.management_config(Some(convert_management_config_to_sdk(config)));
    }

    let request = alien_platform_api::types::ManagerHeartbeatRequest::try_from(request_builder)
        .map_err(|e| {
            alien_error::AlienError::new(ErrorData::SelfHeartbeatFailed {
                message: format!("Failed to build heartbeat request: {}", e),
            })
        })?;

    let _response = ext
        .client
        .report_manager_heartbeat()
        .id(&ext.manager_id)
        .body(request)
        .send()
        .await
        .into_sdk_error()
        .map_err(|e| {
            alien_error::AlienError::new(ErrorData::SelfHeartbeatFailed {
                message: format!("Failed to send heartbeat: {}", e),
            })
        })?;

    debug!(
        manager_id = %ext.manager_id,
        base_url = %ext.base_url,
        version = env!("CARGO_PKG_VERSION"),
        "Self-heartbeat sent successfully"
    );

    Ok(())
}

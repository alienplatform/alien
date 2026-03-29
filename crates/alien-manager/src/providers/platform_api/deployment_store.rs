use crate::traits::{
    AcquiredDeployment, CreateDeploymentGroupParams, CreateDeploymentParams, DeploymentFilter,
    DeploymentGroupRecord, DeploymentRecord, DeploymentStore, ReconcileData,
};
use alien_core::{DeploymentStatus, Platform};
use alien_error::{AlienError, GenericError, IntoAlienError};
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use chrono::Utc;
use tracing::{error, warn};

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

/// Check if an `AlienError` represents an HTTP 404 response.
fn is_not_found(e: &AlienError) -> bool {
    e.http_status_code == Some(404) || e.code.to_uppercase().contains("NOT_FOUND")
}

/// Bridges alien-manager's `DeploymentStore` trait to the Platform API sync endpoints.
pub struct PlatformApiDeploymentStore {
    platform_client: alien_platform_api::Client,
    manager_id: String,
}

impl PlatformApiDeploymentStore {
    pub fn new(platform_client: alien_platform_api::Client, manager_id: String) -> Self {
        Self {
            platform_client,
            manager_id,
        }
    }
}

#[async_trait]
impl DeploymentStore for PlatformApiDeploymentStore {
    async fn acquire(
        &self,
        session: &str,
        filter: &DeploymentFilter,
        limit: u32,
    ) -> Result<Vec<AcquiredDeployment>, AlienError> {
        let statuses: Vec<alien_platform_api::types::SyncAcquireRequestStatusesItem> = filter
            .statuses
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter_map(|s| {
                convert_via_json::<_, alien_platform_api::types::SyncAcquireRequestStatusesItem>(
                    &serde_json::Value::String(s.clone()),
                )
                .ok()
            })
            .collect();

        let platforms: Vec<alien_platform_api::types::SyncAcquireRequestPlatformsItem> = filter
            .platforms
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| {
                convert_via_json::<_, alien_platform_api::types::SyncAcquireRequestPlatformsItem>(p)
                    .ok()
            })
            .collect();

        let manager_id: alien_platform_api::types::ManagerId =
            self.manager_id.as_str().try_into().map_err(
                |_: alien_platform_api::types::error::ConversionError| {
                    AlienError::new(GenericError {
                        message: "Invalid manager ID format for sync acquire".to_string(),
                    })
                },
            )?;

        let response = self
            .platform_client
            .sync_acquire()
            .body(alien_platform_api::types::SyncAcquireRequest {
                manager_id: Some(manager_id),
                session: session.to_string(),
                deployment_ids: vec![],
                statuses,
                platforms,
                deployment_model: Some(
                    alien_platform_api::types::SyncAcquireRequestDeploymentModel::Push,
                ),
                limit: std::num::NonZeroU64::new(limit as u64),
            })
            .send()
            .await
            .into_sdk_error()?;

        for failure in &response.failures {
            error!(
                deployment_id = %failure.deployment_id.as_str(),
                error_code = ?failure.error.code,
                error_message = ?failure.error.message,
                "Deployment context-build failure during acquire (lock released by API)"
            );
        }

        let mut acquired = Vec::new();
        for context in &response.deployments {
            let deployment_id = context.deployment_id.to_string();

            let current_state: alien_core::DeploymentState =
                match convert_via_json(&context.current) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(
                            deployment_id = %deployment_id,
                            error = %e,
                            "Failed to parse deployment state, skipping"
                        );
                        continue;
                    }
                };

            let deployment_config: Option<alien_core::DeploymentConfig> =
                convert_via_json(&context.config).ok();

            let stack_settings = deployment_config
                .as_ref()
                .map(|c| c.stack_settings.clone())
                .unwrap_or_default();

            let user_env_vars = deployment_config.as_ref().and_then(|c| {
                let vars = c.environment_variables.variables.clone();
                if vars.is_empty() {
                    None
                } else {
                    Some(vars)
                }
            });

            let management_config = deployment_config
                .as_ref()
                .and_then(|c| c.management_config.clone());

            let record = DeploymentRecord {
                id: deployment_id.clone(),
                name: deployment_id.clone(),
                deployment_group_id: context.project_id.to_string(),
                platform: current_state.platform,
                status: status_to_string(current_state.status),
                stack_settings,
                stack_state: current_state.stack_state,
                environment_info: current_state.environment_info,
                runtime_metadata: current_state.runtime_metadata,
                current_release_id: current_state
                    .current_release
                    .as_ref()
                    .map(|r| r.release_id.clone()),
                desired_release_id: current_state
                    .target_release
                    .as_ref()
                    .map(|r| r.release_id.clone()),
                user_environment_variables: user_env_vars,
                management_config,
                retry_requested: current_state.retry_requested,
                locked_by: None,
                locked_at: None,
                created_at: Utc::now(),
                updated_at: None,
                error: None,
            };

            acquired.push(AcquiredDeployment { deployment: record });
        }

        Ok(acquired)
    }

    async fn reconcile(&self, data: ReconcileData) -> Result<DeploymentRecord, AlienError> {
        let sdk_state: alien_platform_api::types::SyncReconcileRequestState =
            convert_via_json(&data.state)?;

        let deployment_id: alien_platform_api::types::SyncReconcileRequestDeploymentId =
            data.deployment_id.as_str().try_into().map_err(
                |_: alien_platform_api::types::error::ConversionError| {
                    AlienError::new(GenericError {
                        message: format!(
                            "Invalid deployment ID format for reconcile: {}",
                            data.deployment_id
                        ),
                    })
                },
            )?;

        let reconcile_response = self
            .platform_client
            .sync_reconcile()
            .body(alien_platform_api::types::SyncReconcileRequest {
                deployment_id,
                session: Some(data.session),
                state: sdk_state,
                error: data.error.and_then(|e| convert_via_json(&e).ok()),
                update_heartbeat: Some(data.update_heartbeat),
            })
            .send()
            .await
            .into_sdk_error()?;

        let updated_state: alien_core::DeploymentState =
            convert_via_json(&reconcile_response.current)?;

        Ok(DeploymentRecord {
            id: data.deployment_id,
            name: String::new(),
            deployment_group_id: String::new(),
            platform: updated_state.platform,
            status: status_to_string(updated_state.status),
            stack_settings: alien_core::StackSettings::default(),
            stack_state: updated_state.stack_state,
            environment_info: updated_state.environment_info,
            runtime_metadata: updated_state.runtime_metadata,
            current_release_id: updated_state.current_release.map(|r| r.release_id),
            desired_release_id: updated_state.target_release.map(|r| r.release_id),
            user_environment_variables: None,
            management_config: None,
            retry_requested: updated_state.retry_requested,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
            error: None,
        })
    }

    async fn release(&self, deployment_id: &str, session: &str) -> Result<(), AlienError> {
        let deployment_id_typed: alien_platform_api::types::SyncReleaseRequestDeploymentId =
            deployment_id.try_into().map_err(
                |_: alien_platform_api::types::error::ConversionError| {
                    AlienError::new(GenericError {
                        message: format!(
                            "Invalid deployment ID format for release: {}",
                            deployment_id
                        ),
                    })
                },
            )?;

        self.platform_client
            .sync_release()
            .body(alien_platform_api::types::SyncReleaseRequest {
                deployment_id: deployment_id_typed,
                session: session.to_string(),
            })
            .send()
            .await
            .into_sdk_error()?;

        Ok(())
    }

    async fn get_deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError> {
        let result = self
            .platform_client
            .get_deployment()
            .id(id)
            .send()
            .await
            .into_sdk_error();

        match result {
            Ok(detail) => Ok(Some(convert_via_json(&*detail)?)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn list_deployments(
        &self,
        filter: &DeploymentFilter,
    ) -> Result<Vec<DeploymentRecord>, AlienError> {
        let mut req = self.platform_client.list_deployments();

        if let Some(ref group_id) = filter.deployment_group_id {
            req = req.deployment_group(group_id);
        }

        let response = req.send().await.into_sdk_error()?;

        response.items.iter().map(|d| convert_via_json(d)).collect()
    }

    async fn delete_deployment(&self, id: &str) -> Result<(), AlienError> {
        self.platform_client
            .delete_deployment()
            .id(id)
            .send()
            .await
            .into_sdk_error()?;
        Ok(())
    }

    async fn set_retry_requested(&self, id: &str) -> Result<(), AlienError> {
        self.platform_client
            .retry_deployment()
            .id(id)
            .send()
            .await
            .into_sdk_error()?;
        Ok(())
    }

    async fn set_redeploy(&self, id: &str) -> Result<(), AlienError> {
        self.platform_client
            .redeploy_deployment()
            .id(id)
            .send()
            .await
            .into_sdk_error()?;
        Ok(())
    }

    async fn set_deployment_desired_release(
        &self,
        deployment_id: &str,
        release_id: &str,
    ) -> Result<(), AlienError> {
        let body: alien_platform_api::types::PinReleaseRequest =
            convert_via_json(&serde_json::json!({ "releaseId": release_id }))?;

        self.platform_client
            .pin_deployment_release()
            .id(deployment_id)
            .body(body)
            .send()
            .await
            .into_sdk_error()?;
        Ok(())
    }

    async fn set_desired_release(
        &self,
        _release_id: &str,
        _platform: Option<Platform>,
    ) -> Result<(), AlienError> {
        // The Platform API handles auto-deployment propagation when a new release is created.
        Ok(())
    }

    async fn create_deployment(
        &self,
        params: CreateDeploymentParams,
    ) -> Result<DeploymentRecord, AlienError> {
        let body: alien_platform_api::types::NewDeploymentRequest =
            convert_via_json(&serde_json::json!({
                "name": params.name,
                "deploymentGroupId": params.deployment_group_id,
                "platform": params.platform,
                "stackSettings": params.stack_settings,
            }))?;

        let detail = self
            .platform_client
            .create_deployment()
            .body(body)
            .send()
            .await
            .into_sdk_error()?;

        convert_via_json(&*detail)
    }

    async fn create_deployment_group(
        &self,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError> {
        let body: alien_platform_api::types::CreateDeploymentGroupRequest =
            convert_via_json(&serde_json::json!({
                "name": params.name,
                "maxDeployments": params.max_deployments,
            }))?;

        let group = self
            .platform_client
            .create_deployment_group()
            .body(body)
            .send()
            .await
            .into_sdk_error()?;

        convert_via_json(&*group)
    }

    async fn create_deployment_group_with_id(
        &self,
        _id: &str,
        params: CreateDeploymentGroupParams,
    ) -> Result<DeploymentGroupRecord, AlienError> {
        // The Platform API assigns IDs; the requested ID is ignored.
        self.create_deployment_group(params).await
    }

    async fn get_deployment_group(
        &self,
        id: &str,
    ) -> Result<Option<DeploymentGroupRecord>, AlienError> {
        let result = self
            .platform_client
            .get_deployment_group()
            .id(id)
            .send()
            .await
            .into_sdk_error();

        match result {
            Ok(group) => Ok(Some(convert_via_json(&*group)?)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn list_deployment_groups(&self) -> Result<Vec<DeploymentGroupRecord>, AlienError> {
        let response = self
            .platform_client
            .list_deployment_groups()
            .send()
            .await
            .into_sdk_error()?;

        response.items.iter().map(|g| convert_via_json(g)).collect()
    }
}

/// Serialize `DeploymentStatus` to its kebab-case string representation.
fn status_to_string(status: DeploymentStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "pending".to_string())
}

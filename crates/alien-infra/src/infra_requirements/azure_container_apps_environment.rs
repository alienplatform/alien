use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, error, info};

use crate::azure_utils::{azure_container_apps_environment_resource_id, get_resource_group_name};
use crate::core::{
    map_azure_core_021_delete_lro_response, map_azure_core_021_lro_response,
    map_azure_core_021_sdk_error, LongRunningOperation, OperationResult, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    AzureClientConfig, AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentHeartbeatData,
    AzureContainerAppsEnvironmentHeartbeatStatus, AzureContainerAppsEnvironmentOutputs,
    AzureContainerAppsEnvironmentWorkloadProfile, HeartbeatBackend, Network, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use azure_mgmt_app::package_preview_2024_08 as azure_app_2024_08;
use azure_mgmt_app::package_preview_2024_08::models::{
    managed_environment, managed_environment::properties::ProvisioningState, ManagedEnvironment,
    TrackedResource, VnetConfiguration,
};
use chrono::Utc;
use futures_util::StreamExt;

#[controller]
pub struct AzureContainerAppsEnvironmentController {
    /// The actual Azure environment name (may differ from config.id).
    pub(crate) environment_name: Option<String>,
    /// The Azure resource ID of the environment.
    pub(crate) resource_id: Option<String>,
    /// The resource group containing the environment.
    pub(crate) resource_group_name: Option<String>,
    /// The default domain for applications in this environment.
    pub(crate) default_domain: Option<String>,
    /// The static IP address of the environment (if applicable).
    pub(crate) static_ip: Option<String>,
    /// Azure Container Apps custom domain verification ID.
    pub(crate) custom_domain_verification_id: Option<String>,
    /// Long-running operation information for monitoring Azure operations.
    pub(crate) long_running_operation: Option<LongRunningOperation>,
}

#[controller]
impl AzureContainerAppsEnvironmentController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        info!(id=%desired_config.id, "Initiating Azure Container Apps Environment creation");
        let azure_config = ctx.get_azure_config()?;
        self.environment_name = Some(generate_container_apps_environment_name(
            &ctx.resource_prefix,
            &desired_config.id,
        ));
        let resource_group_name = get_resource_group_name(ctx.state)?;
        self.resource_group_name = Some(resource_group_name.clone());

        // Create the managed environment via Azure client
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;
        let managed_env = self.build_managed_environment(azure_config, ctx);

        let environment_name = self.environment_name.as_ref().unwrap();
        let result = client
            .managed_environments_client()
            .create_or_update(
                azure_config.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
                managed_env,
            )
            .send()
            .await;
        let operation_result = map_azure_core_021_lro_response(
            "Azure Container Apps",
            result,
            "managed environment create or update",
            "Azure Container Apps Managed Environment",
            environment_name,
            |response| response.into_body(),
        )
        .await
        .map_err(|e| {
            // Azure quota errors (e.g., MaxNumberOfRegionalEnvironmentsInSubExceeded) are
            // returned as 409 Conflict, which gets classified as retryable. However, quota
            // limits won't self-resolve within the retry window, so mark them non-retryable
            // to fail fast instead of wasting ~8 minutes on futile retries.
            let is_quota_error =
                e.to_string().contains("MaxNumberOf") || e.to_string().contains("Exceeded");
            let mut err = e.context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure Container Apps Environment".to_string(),
                resource_id: Some(desired_config.id.clone()),
            });
            if is_quota_error {
                err.retryable = false;
            }
            err
        })?;

        match operation_result {
            OperationResult::Completed(result) => {
                info!(environment_name=%self.environment_name.as_ref().unwrap(), "Environment creation completed immediately");
                self.handle_creation_completed(&result, azure_config);
                // Always go to Ready after immediate completion - linear flow
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(long_running_op) => {
                debug!(environment_name=%self.environment_name.as_ref().unwrap(), "Environment creation initiated as long-running operation");
                self.long_running_operation = Some(long_running_op.clone());

                // Always go to CreatingEnvironmentOperation for long-running ops - linear flow
                Ok(HandlerAction::Continue {
                    state: CreatingEnvironmentOperation,
                    suggested_delay: long_running_op.retry_after,
                })
            }
        }
    }

    #[handler(
        state = CreatingEnvironmentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        let long_running_op = self.long_running_operation.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Long-running operation not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, "Azure environment create accepted, checking managed environment resource status");
        let delay = long_running_op
            .retry_after
            .unwrap_or(Duration::from_secs(10));
        self.long_running_operation = None;
        Ok(HandlerAction::Continue {
            state: CreatingEnvironment,
            suggested_delay: Some(delay),
        })
    }

    #[handler(
        state = CreatingEnvironment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn wait_for_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, "Checking environment creation status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        let result = client
            .managed_environments_client()
            .get(
                azure_config.subscription_id.clone(),
                resource_group_name,
                environment_name.clone(),
            )
            .await;
        match map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "managed environment get",
            "Azure Container Apps Managed Environment",
            environment_name,
        ) {
            Ok(managed_env) => {
                if let Some(properties) = &managed_env.properties {
                    match properties.provisioning_state.as_ref() {
                        Some(ProvisioningState::Succeeded) => {
                            info!(environment_name=%environment_name, "Environment creation completed");
                            self.handle_creation_completed(&managed_env, azure_config);

                            // Always go to ApplyingResourcePermissions next
                            Ok(HandlerAction::Continue {
                                state: ApplyingResourcePermissions,
                                suggested_delay: None,
                            })
                        }
                        Some(ProvisioningState::InitializationInProgress)
                        | Some(ProvisioningState::InfrastructureSetupInProgress)
                        | Some(ProvisioningState::Waiting) => {
                            debug!(environment_name=%environment_name, "Environment still being created");
                            Ok(HandlerAction::Stay {
                                max_times: 20,
                                suggested_delay: Some(Duration::from_secs(15)),
                            })
                        }
                        Some(ProvisioningState::Failed) => {
                            error!(environment_name=%environment_name, "Environment creation failed");
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Environment creation failed with status: Failed"
                                    .to_string(),
                                resource_id: Some(desired_config.id.clone()),
                            }));
                        }
                        _ => {
                            debug!(environment_name=%environment_name, "Provisioning state not available, continuing to wait");
                            Ok(HandlerAction::Stay {
                                max_times: 20,
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                    }
                } else {
                    debug!(environment_name=%environment_name, "Properties not available, continuing to wait");
                    Ok(HandlerAction::Stay {
                        max_times: 20,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(AlienError {
                error: Some(ErrorData::CloudResourceNotFound { .. }),
                ..
            }) => {
                debug!(environment_name=%environment_name, "Environment not yet available, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check environment creation status".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self
            .environment_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Environment name not set".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                })
            })?
            .clone();

        info!(environment_name=%environment_name, "Applying resource-scoped permissions for container apps environment");

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx, &environment_name)
            .await?;

        info!(environment_name=%environment_name, "Resource-scoped permissions applied successfully");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;

        // Heartbeat check: verify environment provisioning state
        if let Some(environment_name) = &self.environment_name {
            let resource_group_name = get_resource_group_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_container_apps_management_client(azure_config)?;

            let result = client
                .managed_environments_client()
                .get(
                    azure_config.subscription_id.clone(),
                    resource_group_name.clone(),
                    environment_name.clone(),
                )
                .await;
            let managed_env = map_azure_core_021_sdk_error(
                "Azure Container Apps",
                result,
                "managed environment get",
                "Azure Container Apps Managed Environment",
                environment_name,
            )
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get Container Apps Environment '{}'",
                    environment_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

            emit_azure_container_apps_environment_heartbeat(
                ctx,
                &config.id,
                &resource_group_name,
                &managed_env,
            );

            if let Some(properties) = &managed_env.properties {
                match properties.provisioning_state.as_ref() {
                    Some(ProvisioningState::Succeeded) => {
                        // Environment is healthy
                    }
                    Some(state) => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: format!(
                                "Provisioning state changed from Succeeded to {:?}",
                                state
                            ),
                        }));
                    }
                    None => {
                        return Err(AlienError::new(ErrorData::ResourceDrift {
                            resource_id: config.id.clone(),
                            message: "Provisioning state is no longer available".to_string(),
                        }));
                    }
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        // If the resource group is also being deleted, skip environment deletion.
        // RG deletion cascades — Azure deletes all resources within the RG.
        // This avoids the DeletingEnvironmentOperation polling timeout (~5 min)
        // that occurs because Azure env deletion takes longer than our poll limit.
        if let Some(rg_state) = ctx.state.resources.get("default-resource-group") {
            if matches!(
                rg_state.status,
                ResourceStatus::Deleting | ResourceStatus::Deleted
            ) {
                info!(
                    environment_name = %environment_name,
                    "Resource group is being deleted, skipping environment deletion (cascade)"
                );
                self.clear_state();
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        }

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let container_apps_client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        // Check if Container Apps still exist in this environment before
        // attempting deletion. Azure rejects environment deletion with
        // ManagedEnvironmentHasContainerApps (409) if apps remain.
        // Use the stored resource_id (full ARM path) rather than constructing it,
        // since the environment may be in a different resource group than the stack.
        let env_resource_id = self.resource_id.clone().unwrap_or_else(|| {
            format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}",
                azure_config.subscription_id, resource_group_name, environment_name
            )
        });
        // List container apps from the stack's resource group (where the apps live)
        let apps = {
            let mut stream = container_apps_client
                .container_apps_client()
                .list_by_resource_group(
                    azure_config.subscription_id.clone(),
                    resource_group_name.clone(),
                )
                .into_stream();
            let mut apps = Vec::new();
            let mut result = Ok(());
            while let Some(page) = stream.next().await {
                match map_azure_core_021_sdk_error(
                    "Azure Container Apps",
                    page,
                    "container apps list by resource group",
                    "Azure Container Apps",
                    &resource_group_name,
                ) {
                    Ok(page) => apps.extend(page.value),
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            result.map(|()| azure_app_2024_08::models::ContainerAppCollection::new(apps))
        };
        if let Ok(collection) = apps {
            let remaining: Vec<_> = collection
                .value
                .iter()
                .filter(|app| {
                    app.properties
                        .as_ref()
                        .and_then(|p| p.managed_environment_id.as_deref())
                        .map(|id| id.eq_ignore_ascii_case(&env_resource_id))
                        .unwrap_or(false)
                })
                .collect();
            if !remaining.is_empty() {
                info!(
                    environment_name=%environment_name,
                    remaining_apps=%remaining.len(),
                    "Waiting for Container Apps to be deleted before removing environment"
                );
                return Ok(HandlerAction::Continue {
                    state: DeleteStart,
                    suggested_delay: Some(Duration::from_secs(15)),
                });
            }
        }

        info!(environment_name=%environment_name, "Initiating Azure Container Apps Environment deletion");

        let management_client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        let result = management_client
            .managed_environments_client()
            .delete(
                azure_config.subscription_id.clone(),
                resource_group_name.clone(),
                environment_name.clone(),
            )
            .send()
            .await;
        match map_azure_core_021_delete_lro_response(
            "Azure Container Apps",
            result,
            "managed environment delete",
            "Azure Container Apps Managed Environment",
            environment_name,
        )
        .await
        {
            Ok(operation_result) => match operation_result {
                OperationResult::Completed(_) => {
                    info!(environment_name=%environment_name, "Environment deletion completed immediately");
                    self.clear_state();
                    Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    })
                }
                OperationResult::LongRunning(long_running_op) => {
                    debug!(environment_name=%environment_name, "Environment deletion initiated as long-running operation");
                    self.long_running_operation = Some(long_running_op);
                    Ok(HandlerAction::Continue {
                        state: DeletingEnvironmentOperation,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            },
            Err(AlienError {
                error: Some(ErrorData::CloudResourceNotFound { .. }),
                ..
            }) => {
                info!(environment_name=%environment_name, "Environment already deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete Azure Container Apps Environment".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = DeletingEnvironmentOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        let long_running_op = self.long_running_operation.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Long-running operation not set in state".to_string(),
            })
        })?;

        // If the resource group is now being deleted, skip polling — RG cascade handles it.
        if let Some(rg_state) = ctx.state.resources.get("default-resource-group") {
            if matches!(
                rg_state.status,
                ResourceStatus::Deleting | ResourceStatus::Deleted
            ) {
                info!(
                    environment_name = %environment_name,
                    "Resource group is being deleted, skipping environment deletion polling (cascade)"
                );
                self.clear_state();
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        }

        debug!(environment_name=%environment_name, "Azure environment delete accepted, checking managed environment deletion status");
        let delay = long_running_op
            .retry_after
            .unwrap_or(Duration::from_secs(10));
        self.long_running_operation = None;
        Ok(HandlerAction::Continue {
            state: DeletingEnvironment,
            suggested_delay: Some(delay),
        })
    }

    #[handler(
        state = DeletingEnvironment,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn wait_for_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let desired_config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;
        let environment_name = self.environment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: desired_config.id.clone(),
                message: "Environment name not set in state".to_string(),
            })
        })?;

        debug!(environment_name=%environment_name, "Checking environment deletion status");

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_management_client(azure_config)?;

        let result = client
            .managed_environments_client()
            .get(
                azure_config.subscription_id.clone(),
                resource_group_name,
                environment_name.clone(),
            )
            .await;
        match map_azure_core_021_sdk_error(
            "Azure Container Apps",
            result,
            "managed environment get",
            "Azure Container Apps Managed Environment",
            environment_name,
        ) {
            Ok(_) => {
                debug!(environment_name=%environment_name, "Environment still exists, continuing to wait");
                Ok(HandlerAction::Stay {
                    max_times: 20,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(AlienError {
                error: Some(ErrorData::CloudResourceNotFound { .. }),
                ..
            }) => {
                info!(environment_name=%environment_name, "Environment successfully deleted");
                self.clear_state();
                // Always go to Deleted when deletion is confirmed - linear flow
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to check environment deletion status".to_string(),
                    resource_id: Some(desired_config.id.clone()),
                }))
            }
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Only return outputs when we have all the required information
        if let (Some(environment_name), Some(resource_id), Some(resource_group_name)) = (
            &self.environment_name,
            &self.resource_id,
            &self.resource_group_name,
        ) {
            Some(ResourceOutputs::new(AzureContainerAppsEnvironmentOutputs {
                environment_name: environment_name.clone(),
                resource_id: resource_id.clone(),
                resource_group_name: resource_group_name.clone(),
                default_domain: self.default_domain.clone().unwrap_or_default(),
                static_ip: self.static_ip.clone(),
                custom_domain_verification_id: self.custom_domain_verification_id.clone(),
            }))
        } else {
            None
        }
    }
}

fn emit_azure_container_apps_environment_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    managed_env: &ManagedEnvironment,
) {
    let properties = managed_env.properties.as_ref();
    let provisioning_state = properties.and_then(|p| p.provisioning_state.as_ref());
    let (health, lifecycle) = match provisioning_state {
        Some(managed_environment::properties::ProvisioningState::Succeeded) => {
            (ObservedHealth::Healthy, ProviderLifecycleState::Running)
        }
        Some(_) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Failed),
        None => (ObservedHealth::Unknown, ProviderLifecycleState::Unknown),
    };
    let name = managed_env
        .tracked_resource
        .resource
        .name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());
    let workload_profiles = properties
        .map(|p| {
            p.workload_profiles
                .iter()
                .map(|profile| AzureContainerAppsEnvironmentWorkloadProfile {
                    name: profile.name.to_string(),
                    workload_profile_type: profile.workload_profile_type.to_string(),
                    minimum_count: profile.minimum_count,
                    maximum_count: profile.maximum_count,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: AzureContainerAppsEnvironment::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::AzureContainerAppsEnvironment(
            AzureContainerAppsEnvironmentHeartbeatData {
                status: AzureContainerAppsEnvironmentHeartbeatStatus {
                    health,
                    lifecycle,
                    message: Some(format!(
                        "Azure Container Apps environment '{}' provisioning state is {}",
                        name,
                        provisioning_state
                            .map(|state| format!("{state:?}"))
                            .as_deref()
                            .unwrap_or("unknown")
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name,
                resource_id: managed_env.tracked_resource.resource.id.clone(),
                resource_group: Some(resource_group_name.to_string()),
                location: Some(managed_env.tracked_resource.location.clone()),
                kind: managed_env.kind.clone(),
                provisioning_state: provisioning_state.map(|state| format!("{state:?}")),
                default_domain: properties.and_then(|p| p.default_domain.clone()),
                static_ip: properties.and_then(|p| p.static_ip.clone()),
                custom_domain_verification_id: properties
                    .and_then(|p| p.custom_domain_configuration.as_ref())
                    .and_then(|c| c.custom_domain_verification_id.clone()),
                infrastructure_resource_group: properties
                    .and_then(|p| p.infrastructure_resource_group.clone()),
                event_stream_endpoint: properties.and_then(|p| p.event_stream_endpoint.clone()),
                zone_redundant: properties.and_then(|p| p.zone_redundant),
                workload_profile_count: workload_profiles.len() as u32,
                workload_profiles,
            },
        ),
        raw: vec![],
    });
}

// Separate impl block for helper methods
impl AzureContainerAppsEnvironmentController {
    // ─────────────── HELPER METHODS ────────────────────────────
    fn handle_creation_completed(
        &mut self,
        managed_env: &ManagedEnvironment,
        azure_config: &AzureClientConfig,
    ) {
        self.resource_id = managed_env
            .tracked_resource
            .resource
            .id
            .clone()
            .or_else(|| {
                let resource_group_name = self.resource_group_name.as_ref()?;
                let environment_name = self.environment_name.as_ref()?;
                Some(azure_container_apps_environment_resource_id(
                    &azure_config.subscription_id,
                    resource_group_name,
                    environment_name,
                ))
            });
        self.default_domain = managed_env
            .properties
            .as_ref()
            .and_then(|p| p.default_domain.clone());
        self.static_ip = managed_env
            .properties
            .as_ref()
            .and_then(|p| p.static_ip.clone());
        self.custom_domain_verification_id = managed_env
            .properties
            .as_ref()
            .and_then(|p| p.custom_domain_configuration.as_ref())
            .and_then(|c| c.custom_domain_verification_id.clone());
    }

    fn clear_state(&mut self) {
        self.environment_name = None;
        self.resource_id = None;
        self.resource_group_name = None;
        self.default_domain = None;
        self.static_ip = None;
        self.custom_domain_verification_id = None;
        self.long_running_operation = None;
    }

    fn build_managed_environment(
        &self,
        azure_config: &AzureClientConfig,
        ctx: &ResourceControllerContext,
    ) -> ManagedEnvironment {
        let location = azure_config.region.as_deref().unwrap_or("East US");

        let mut tags = HashMap::new();
        tags.insert(
            "resource-type".to_string(),
            "container-apps-environment".to_string(),
        );
        tags.insert("resource".to_string(), ctx.desired_config.id().to_string());
        tags.insert("deployment".to_string(), ctx.resource_prefix.to_string());

        // Get VNet configuration if a Network resource exists
        let vnet_configuration = self.get_vnet_configuration(ctx);
        if vnet_configuration.is_some() {
            info!("Configuring Container Apps Environment with VNet integration");
        }

        let mut tracked_resource = TrackedResource::new(location.to_string());
        tracked_resource.tags = Some(serde_json::json!(tags));

        ManagedEnvironment {
            tracked_resource,
            identity: None,
            kind: None,
            properties: Some(managed_environment::Properties {
                vnet_configuration,
                workload_profiles: Vec::new(),
                zone_redundant: Some(false),
                ..Default::default()
            }),
        }
    }

    /// Gets VNet configuration from the Network resource if one exists in the stack.
    ///
    /// If a Network resource exists (ID: "default-network"), this method retrieves
    /// the private subnet ID from the Network controller to configure the Container
    /// Apps Environment with VNet integration.
    ///
    /// Returns `None` if no Network resource exists in the stack.
    fn get_vnet_configuration(&self, ctx: &ResourceControllerContext) -> Option<VnetConfiguration> {
        // Check if the stack has a Network resource
        let network_id = "default-network";
        if !ctx.desired_stack.resources.contains_key(network_id) {
            return None;
        }

        // Get the Network controller state via require_dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, network_id.to_string());
        let network_state =
            match ctx.require_dependency::<crate::network::AzureNetworkController>(&network_ref) {
                Ok(state) => state,
                Err(_) => return None,
            };

        // Get the private subnet resource ID
        // For Azure Container Apps, we need the full resource ID of the subnet
        let vnet_resource_id = match &network_state.vnet_resource_id {
            Some(id) => id,
            None => return None,
        };

        let private_subnet_name = match &network_state.private_subnet_name {
            Some(name) => name,
            None => return None,
        };

        // Construct the full subnet resource ID
        let infrastructure_subnet_id =
            format!("{}/subnets/{}", vnet_resource_id, private_subnet_name);

        Some(VnetConfiguration {
            infrastructure_subnet_id: Some(infrastructure_subnet_id),
            internal: Some(false), // Allow external access by default
            docker_bridge_cidr: None,
            platform_reserved_cidr: None,
            platform_reserved_dns_ip: None,
        })
    }

    /// Applies resource-scoped permissions to the container apps environment from stack permission profiles.
    ///
    /// Creates Azure role definitions and role assignments so that:
    /// - The function's managed identity gets appropriate access to the environment
    /// - The management UAMI gets management access for deployment operations
    async fn apply_resource_scoped_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        environment_name: &str,
    ) -> Result<()> {
        use crate::core::ResourcePermissionsHelper;
        use crate::core::Scope;

        let config = ctx.desired_resource_config::<AzureContainerAppsEnvironment>()?;

        let resource_scope = Scope::Resource {
            resource_group_name: get_resource_group_name(ctx.state)?,
            resource_provider: "Microsoft.App".to_string(),
            parent_resource_path: None,
            resource_type: "managedEnvironments".to_string(),
            resource_name: environment_name.to_string(),
        };

        ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
            ctx,
            &config.id,
            environment_name,
            resource_scope,
            "ContainerAppsEnvironment",
            "azure-container-apps-environment",
        )
        .await
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(environment_name: &str) -> Self {
        Self {
            state: AzureContainerAppsEnvironmentState::Ready,
            environment_name: Some(environment_name.to_string()),
            resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/mock-rg/providers/Microsoft.App/managedEnvironments/{}", environment_name)),
            resource_group_name: Some("mock-rg".to_string()),
            default_domain: Some(format!("{}.eastus.azurecontainerapps.io", environment_name)),
            static_ip: Some("20.1.2.3".to_string()),
            custom_domain_verification_id: Some("mock-verification-id".to_string()),
            long_running_operation: None,
            _internal_stay_count: None,
        }
    }
}

/// Generates the full, prefixed Azure Container Apps Environment name (pure function).
fn generate_container_apps_environment_name(resource_prefix: &str, id: &str) -> String {
    // Azure Container Apps Environment names must be valid DNS names
    // Format: {prefix}-{id} with length constraints and character restrictions
    let clean_prefix = resource_prefix
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    let clean_id = id
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();

    let combined = format!("{}-{}", clean_prefix, clean_id);

    // Truncate to reasonable length if necessary (Azure limit is usually around 60 chars)
    if combined.len() > 60 {
        combined[..60].to_string()
    } else {
        combined
    }
}

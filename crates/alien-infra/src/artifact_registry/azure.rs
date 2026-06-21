use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use std::fmt::Debug;
use tracing::{debug, info, warn};

use crate::core::{
    AzureContainerRegistryResource, OperationResult, Registry, ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_core::{
    ArtifactRegistry, ArtifactRegistryHeartbeatData, ArtifactRegistryHeartbeatStatus,
    ArtifactRegistryOutputs, AzureContainerRegistryHeartbeatData, HeartbeatBackend, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use azure_mgmt_containerregistry::package_2023_11_preview::models::{
    registry_properties, sku, RegistryProperties, Sku,
};
use chrono::Utc;

fn serialized_field<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
}

/// Azure Artifact Registry controller.
///
/// Azure requires creating an actual Azure Container Registry resource,
/// so this controller manages the full lifecycle of the registry.
#[controller]
pub struct AzureArtifactRegistryController {
    /// The name of the Azure Container Registry
    pub(crate) registry_name: Option<String>,
    /// The resource group name where the registry is created
    pub(crate) resource_group_name: Option<String>,
    /// The login server URL for the registry
    pub(crate) login_server: Option<String>,
    /// The Azure subscription ID (stored for output generation)
    pub(crate) subscription_id: Option<String>,
}

#[controller]
impl AzureArtifactRegistryController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Generate a unique registry name (ACR names must be globally unique)
        let registry_name = format!(
            "{}{}",
            ctx.resource_prefix.replace("-", ""),
            config.id.replace("-", "")
        );

        // Look up resource group from infra requirements
        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;

        info!(
            registry_id = %config.id,
            registry_name = %registry_name,
            resource_group = %resource_group_name,
            "Creating Azure Container Registry"
        );

        // Create the registry
        let acr_client = ctx
            .service_provider
            .get_azure_container_registry_client(azure_cfg)?;

        let location = azure_cfg.region.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ClientConfigInvalid {
                platform: alien_core::Platform::Azure,
                message: "Azure region is required but not specified in configuration".to_string(),
            })
        })?;
        let mut resource = AzureContainerRegistryResource::new(location);
        resource.name = Some(registry_name.clone());

        let mut registry = Registry::new(resource, Sku::new(sku::Name::Basic));
        registry.properties = Some(RegistryProperties {
            admin_user_enabled: Some(false),
            anonymous_pull_enabled: Some(false),
            public_network_access: Some(registry_properties::PublicNetworkAccess::Enabled),
            zone_redundancy: Some(registry_properties::ZoneRedundancy::Disabled),
            network_rule_bypass_options: Some(
                registry_properties::NetworkRuleBypassOptions::AzureServices,
            ),
            ..Default::default()
        });

        let operation_result = acr_client
            .create_registry(&resource_group_name, &registry_name, &registry)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Container Registry '{}'",
                    registry_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        match operation_result {
            OperationResult::Completed(created_registry) => {
                info!(
                    registry_name = %registry_name,
                    "Azure Container Registry created successfully"
                );

                self.registry_name = Some(registry_name.clone());
                self.resource_group_name = Some(resource_group_name.clone());
                self.subscription_id = Some(azure_cfg.subscription_id.clone());
                self.login_server = created_registry.properties.and_then(|p| p.login_server);

                Ok(HandlerAction::Continue {
                    state: ApplyingPermissions,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_operation) => {
                info!(
                    registry_name = %registry_name,
                    "Azure Container Registry creation is in progress"
                );

                self.registry_name = Some(registry_name);
                self.resource_group_name = Some(resource_group_name);
                self.subscription_id = Some(azure_cfg.subscription_id.clone());

                Ok(HandlerAction::Continue {
                    state: WaitingForCreation,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
        }
    }

    #[handler(
        state = WaitingForCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        let registry_name = self.registry_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Registry name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let resource_group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            registry_name = %registry_name,
            "Checking Azure Container Registry creation status"
        );

        let acr_client = ctx
            .service_provider
            .get_azure_container_registry_client(azure_cfg)?;

        match acr_client
            .get_registry(resource_group_name, registry_name)
            .await
        {
            Ok(registry) => {
                let login_server = registry.properties.and_then(|p| p.login_server);

                // Verify the data plane is reachable before transitioning.
                // ARM may report the registry as provisioned while the login
                // server (*.azurecr.io) is still warming up. GET /v2/ returns
                // 200 or 401 when the data plane is ready; transport errors
                // mean it's not yet serving.
                if let Some(ref server) = login_server {
                    let probe_url = format!("https://{}/v2/", server);
                    let probe_client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(5))
                        .build()
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to create HTTP client for ACR data plane probe"
                                .to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;

                    match probe_client.get(&probe_url).send().await {
                        Ok(_) => {
                            // Any HTTP response (200, 401, etc.) means data plane is up
                        }
                        Err(e) => {
                            info!(
                                registry_name = %registry_name,
                                login_server = %server,
                                error = %e,
                                "ACR data plane not yet reachable, retrying"
                            );
                            return Ok(HandlerAction::Continue {
                                state: WaitingForCreation,
                                suggested_delay: Some(std::time::Duration::from_secs(5)),
                            });
                        }
                    }
                }

                info!(
                    registry_name = %registry_name,
                    "Azure Container Registry is ready"
                );

                self.login_server = login_server;

                Ok(HandlerAction::Continue {
                    state: ApplyingPermissions,
                    suggested_delay: None,
                })
            }
            Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                // Registry not yet provisioned — retry with backoff.
                warn!(
                    registry_name = %registry_name,
                    error = %e,
                    "Azure Container Registry not yet ready, retrying"
                );

                Ok(HandlerAction::Continue {
                    state: WaitingForCreation,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(e) => {
                // Real error (auth failure, service unavailable, etc.) — propagate.
                Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to verify Azure Container Registry '{}'",
                        registry_name
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
        }
    }

    #[handler(
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(registry_id = %config.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(registry_name) = &self.registry_name {
            self.apply_resource_scoped_permissions(ctx, registry_name)
                .await?;
        }

        info!(registry_id = %config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Azure Container Registry update (no-op - nothing to update)"
        );

        // Azure Container Registry updates are not needed - just transition back to ready
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
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
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        let registry_name = match &self.registry_name {
            Some(name) => name,
            None => {
                info!(
                    registry_id = %config.id,
                    "No Azure Container Registry to delete"
                );
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        let resource_group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            registry_id = %config.id,
            registry_name = %registry_name,
            "Deleting Azure Container Registry"
        );

        let acr_client = ctx
            .service_provider
            .get_azure_container_registry_client(azure_cfg)?;

        // Delete registry - treat NotFound as success for idempotent deletion
        match acr_client
            .delete_registry(resource_group_name, registry_name)
            .await
        {
            Ok(_) => {
                info!(
                    registry_name = %registry_name,
                    "Azure Container Registry deleted successfully"
                );
            }
            Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(
                    registry_name = %registry_name,
                    "Azure Container Registry already deleted"
                );
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete Azure Container Registry '{}'",
                        registry_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Heartbeat check: verify registry still exists and hasn't drifted
        if let (Some(registry_name), Some(resource_group_name)) =
            (&self.registry_name, &self.resource_group_name)
        {
            let acr_client = ctx
                .service_provider
                .get_azure_container_registry_client(azure_cfg)?;

            // Verify the registry still exists and is accessible
            match acr_client
                .get_registry(resource_group_name, registry_name)
                .await
            {
                Ok(registry) => {
                    // Check if the login server has changed (indicates drift)
                    if let Some(current_login_server) = registry
                        .properties
                        .as_ref()
                        .and_then(|properties| properties.login_server.as_ref())
                    {
                        if let Some(stored_login_server) = &self.login_server {
                            if current_login_server != stored_login_server {
                                return Err(AlienError::new(ErrorData::ResourceDrift {
                                    resource_id: config.id.clone(),
                                    message: format!("Azure Container Registry login server changed from {} to {}", stored_login_server, current_login_server),
                                }));
                            }
                        }
                        // Update stored login server if it wasn't set
                        if self.login_server.is_none() {
                            self.login_server = Some(current_login_server.clone());
                        }
                    }

                    emit_azure_artifact_registry_heartbeat(
                        ctx,
                        &config.id,
                        resource_group_name,
                        registry_name,
                        &registry,
                    );

                    debug!(registry_name=%registry_name, resource_group=%resource_group_name, "Azure Container Registry heartbeat check passed");
                }
                Err(e) => {
                    return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to verify Azure Container Registry '{}' during heartbeat check",
                            registry_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(30)), // Check again in 30 seconds
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(registry_name), Some(resource_group_name)) =
            (&self.registry_name, &self.resource_group_name)
        {
            let subscription_id = self.subscription_id.as_deref().unwrap_or_else(|| {
                tracing::error!(
                    "Azure subscription_id missing when building artifact registry outputs"
                );
                "missing-subscription-id"
            });
            let registry_id = format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                subscription_id,
                resource_group_name,
                registry_name
            );
            let registry_endpoint = self
                .login_server
                .clone()
                .unwrap_or_else(|| format!("{}.azurecr.io", registry_name));

            Some(ResourceOutputs::new(ArtifactRegistryOutputs {
                registry_id,
                registry_endpoint,
                pull_role: None, // Azure uses built-in token mechanism
                push_role: None, // Azure uses built-in token mechanism
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::ArtifactRegistryBinding;

        if let (Some(registry_name), Some(resource_group_name)) =
            (&self.registry_name, &self.resource_group_name)
        {
            let binding =
                ArtifactRegistryBinding::acr(registry_name.clone(), resource_group_name.clone());

            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

impl AzureArtifactRegistryController {
    /// Applies resource-scoped permissions to the artifact registry from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        registry_name: &str,
    ) -> Result<()> {
        use crate::core::AzurePermissionsHelper;
        use crate::core::Scope;
        use alien_permissions::PermissionContext;

        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let azure_config = ctx.get_azure_config()?;

        // Build permission context for this specific artifact registry resource
        let mut permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(self.resource_group_name.as_ref().unwrap().clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(registry_name.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }

        // Build Azure resource scope for the container registry
        let resource_scope = Scope::Resource {
            resource_group_name: self.resource_group_name.as_ref().unwrap().clone(),
            resource_provider: "Microsoft.ContainerRegistry".to_string(),
            parent_resource_path: None,
            resource_type: "registries".to_string(),
            resource_name: registry_name.to_string(),
        };

        AzurePermissionsHelper::apply_resource_scoped_permissions(
            ctx,
            &config.id,
            "artifact-registry",
            resource_scope,
            &permission_context,
        )
        .await
    }

    /// Create a mock controller for testing
    #[cfg(test)]
    pub fn mock_ready(registry_name: &str, resource_group_name: &str, login_server: &str) -> Self {
        Self {
            state: AzureArtifactRegistryState::Ready,
            registry_name: Some(registry_name.to_string()),
            resource_group_name: Some(resource_group_name.to_string()),
            login_server: Some(login_server.to_string()),
            subscription_id: Some("mock-subscription-id".to_string()),
            _internal_stay_count: None,
        }
    }
}

fn emit_azure_artifact_registry_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    registry_name: &str,
    registry: &Registry,
) {
    let properties = registry.properties.as_ref();
    let network_rule_set = properties.and_then(|properties| properties.network_rule_set.as_ref());
    let encryption = properties.and_then(|properties| properties.encryption.as_ref());
    let key_vault_properties =
        encryption.and_then(|encryption| encryption.key_vault_properties.as_ref());
    let policies = properties.and_then(|properties| properties.policies.as_ref());
    let policy_count = policies
        .map(|policies| {
            [
                policies.azure_ad_authentication_as_arm_policy.is_some(),
                policies.export_policy.is_some(),
                policies.quarantine_policy.is_some(),
                policies.retention_policy.is_some(),
                policies.trust_policy.is_some(),
            ]
            .into_iter()
            .filter(|present| *present)
            .count() as u32
        })
        .unwrap_or(0);
    let managed_tag_count = registry
        .resource
        .tags
        .as_ref()
        .and_then(|tags| tags.as_object())
        .map(|tags| tags.keys().filter(|key| key.starts_with("alien")).count() as u32)
        .unwrap_or(0);

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ArtifactRegistry::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ArtifactRegistry(
            ArtifactRegistryHeartbeatData::AzureContainerRegistry(
                AzureContainerRegistryHeartbeatData {
                    status: ArtifactRegistryHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: Some(format!(
                            "Azure Container Registry '{}' is reachable",
                            registry_name
                        )),
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    name: registry
                        .resource
                        .name
                        .clone()
                        .unwrap_or_else(|| registry_name.to_string()),
                    resource_id: registry.resource.id.clone(),
                    resource_group: resource_group_name.to_string(),
                    location: registry.resource.location.clone(),
                    type_: registry.resource.type_.clone(),
                    login_server: properties.and_then(|properties| properties.login_server.clone()),
                    sku_name: serialized_field(&registry.sku.name)
                        .unwrap_or_else(|| "Unknown".to_string()),
                    sku_tier: registry.sku.tier.as_ref().and_then(serialized_field),
                    provisioning_state: properties
                        .and_then(|properties| properties.provisioning_state.as_ref())
                        .and_then(serialized_field),
                    admin_user_enabled: properties
                        .and_then(|properties| properties.admin_user_enabled)
                        .unwrap_or(false),
                    anonymous_pull_enabled: properties
                        .and_then(|properties| properties.anonymous_pull_enabled)
                        .unwrap_or(false),
                    public_network_access: properties
                        .and_then(|properties| properties.public_network_access.as_ref())
                        .and_then(serialized_field)
                        .unwrap_or_else(|| "Unknown".to_string()),
                    network_rule_bypass_options: properties
                        .and_then(|properties| properties.network_rule_bypass_options.as_ref())
                        .and_then(serialized_field)
                        .unwrap_or_else(|| "Unknown".to_string()),
                    network_rule_default_action: network_rule_set
                        .and_then(|rules| serialized_field(&rules.default_action)),
                    ip_rule_count: network_rule_set
                        .map(|rules| rules.ip_rules.len() as u32)
                        .unwrap_or(0),
                    encryption_status: encryption
                        .and_then(|encryption| encryption.status.as_ref())
                        .and_then(serialized_field),
                    encryption_key_vault_uri_present: key_vault_properties
                        .and_then(|properties| properties.key_identifier.as_ref())
                        .is_some(),
                    encryption_key_identifier_present: key_vault_properties
                        .and_then(|properties| properties.versioned_key_identifier.as_ref())
                        .is_some(),
                    policies_present: policies.is_some(),
                    policy_count,
                    private_endpoint_connection_count: properties
                        .map(|properties| properties.private_endpoint_connections.len() as u32)
                        .unwrap_or(0),
                    data_endpoint_enabled: properties
                        .and_then(|properties| properties.data_endpoint_enabled),
                    data_endpoint_host_names: properties
                        .map(|properties| properties.data_endpoint_host_names.clone())
                        .unwrap_or_default(),
                    zone_redundancy: properties
                        .and_then(|properties| properties.zone_redundancy.as_ref())
                        .and_then(serialized_field)
                        .unwrap_or_else(|| "Unknown".to_string()),
                    creation_date: properties
                        .and_then(|properties| properties.creation_date.as_ref())
                        .map(ToString::to_string),
                    managed_tag_count,
                },
            ),
        ),
        raw: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockContainerRegistryApi;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use std::sync::Arc;

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn setup_mock_service_provider_for_deletion() -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        // Mock the container registry client for delete operations
        mock_provider
            .expect_get_azure_container_registry_client()
            .returning(|_| {
                let mut mock_acr = MockContainerRegistryApi::new();

                // Mock successful deletion
                mock_acr.expect_delete_registry().returning(|_, _| Ok(()));

                Ok(Arc::new(mock_acr))
            });

        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let registry = basic_artifact_registry();
        let registry_name = "testregistry";
        let resource_group = "test-rg";
        let login_server = "testregistry.azurecr.io";

        let mock_provider = setup_mock_service_provider_for_deletion();

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(AzureArtifactRegistryController::mock_ready(
                registry_name,
                resource_group,
                login_server,
            ))
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .build()
            .await
            .unwrap();

        // Test that ready controller has correct outputs
        let outputs = executor.outputs().unwrap();
        let registry_outputs = outputs.downcast_ref::<ArtifactRegistryOutputs>().unwrap();

        assert!(registry_outputs.registry_id.contains(registry_name));
        assert_eq!(registry_outputs.registry_endpoint, login_server);

        // Test delete flow
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}

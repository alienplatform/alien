use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tracing::{debug, error, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::azure_resource_group_resource_id;
use alien_azure_clients::models::resources::ResourceGroup;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureResourceGroup, AzureResourceGroupHeartbeatData, AzureResourceGroupHeartbeatStatus,
    AzureResourceGroupOutputs, HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AzureResourceGroupController {
    /// The name of the created Azure Resource Group.
    pub(crate) resource_group_name: Option<String>,
    /// The Azure resource ID of the created Resource Group.
    pub(crate) resource_id: Option<String>,
    /// The location/region where the Resource Group was created.
    pub(crate) location: Option<String>,
}

#[controller]
impl AzureResourceGroupController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        info!(id=%config.id, "Initiating Azure Resource Group creation");

        let group_name = generate_azure_resource_group_name(&ctx.resource_prefix, &config.id);
        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        let resource_group = ResourceGroup {
            id: None,
            location: azure_config
                .region
                .clone()
                .unwrap_or_else(|| "East US".to_string()),
            managed_by: None,
            name: Some(group_name.clone()),
            properties: None,
            tags: HashMap::new(),
            type_: None,
        };

        // A group that already exists under this name is never taken over: destroy would delete
        // it with everything in it. Only an absent group, or one this deployment tagged (an
        // earlier attempt of its own), is used. A state that already records the name is a
        // retry or an install an older build made, which tagged nothing.
        let marker = owner_marker(
            &ctx.resource_prefix,
            &config.id,
            ctx.setup_owner_deployment_id()?,
        );
        // True when nothing existed under the name, so the create below must not take over a
        // group somebody else makes between this read and the write.
        let mut absent = false;
        if self.resource_group_name.is_none() {
            match client.get_resource_group(&group_name).await {
                Ok(found) => {
                    if found.tags.get(RESOURCE_GROUP_OWNER_TAG).map(String::as_str)
                        != Some(marker.as_str())
                    {
                        return Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Azure resource group '{group_name}' already exists and this \
                                 deployment did not create it. Setup will not take it over, \
                                 because destroying the deployment would delete it with \
                                 everything in it. Pick another deployment name or delete the \
                                 group."
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    absent = true;
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to read resource group '{group_name}'."),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        let mut resource_group = resource_group;
        resource_group
            .tags
            .insert(RESOURCE_GROUP_OWNER_TAG.to_string(), marker);

        let created = if absent {
            client
                .create_resource_group_if_absent(&group_name, &resource_group)
                .await
        } else {
            client
                .create_or_update_resource_group(&group_name, &resource_group)
                .await
        };
        let rg = match created {
            Ok(rg) => rg,
            Err(e)
                if absent
                    && matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceConflict { .. })
                    ) =>
            {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure resource group '{group_name}' was created by someone else while \
                         this deployment was setting up, and setup will not take it over. Pick \
                         another deployment name or delete the group."
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create or update resource group '{}'.",
                        group_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        self.resource_group_name = Some(group_name.clone());
        self.location = Some(rg.location.clone());
        self.resource_id = rg.id.clone().or_else(|| {
            Some(azure_resource_group_resource_id(
                &azure_config.subscription_id,
                &group_name,
            ))
        });

        info!(group_name=%group_name, "Resource group creation initiated");

        Ok(HandlerAction::Continue {
            state: CreatingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = CreatingResourceGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_resource_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        let group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Resource group name not set in state".to_string(),
            })
        })?;

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        match client.get_resource_group(group_name).await {
            Ok(rg) => {
                if let Some(props) = &rg.properties {
                    if let Some(state) = &props.provisioning_state {
                        if state == "Succeeded" {
                            info!(group_name=%group_name, "Resource group creation completed");

                            self.resource_id = rg.id.clone().or_else(|| {
                                Some(azure_resource_group_resource_id(
                                    &azure_config.subscription_id,
                                    group_name,
                                ))
                            });
                            self.location = Some(rg.location.clone());

                            return Ok(HandlerAction::Continue {
                                state: Ready,
                                suggested_delay: None,
                            });
                        } else if state == "Creating" {
                            debug!(group_name=%group_name, "Resource group still being created");

                            return Ok(HandlerAction::Continue {
                                state: CreatingResourceGroup,
                                suggested_delay: Some(Duration::from_secs(5)),
                            });
                        } else {
                            error!(group_name=%group_name, state=?state, "Resource group creation failed");
                            return Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Resource group creation failed with state: {}",
                                    state
                                ),
                                resource_id: Some(config.id.clone()),
                            }));
                        }
                    }
                }
                debug!(group_name=%group_name, "Provisioning state not available, continuing to wait");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(group_name=%group_name, "Resource group not yet available, continuing to wait");
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Error checking resource group status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: CreatingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
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
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        // Heartbeat check: verify resource group provisioning state
        if let Some(group_name) = &self.resource_group_name {
            let client = ctx
                .service_provider
                .get_azure_resources_client(azure_config)?;

            let rg = client.get_resource_group(group_name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'", group_name),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            emit_azure_resource_group_heartbeat(ctx, &config.id, &rg);

            if let Some(properties) = &rg.properties {
                if properties.provisioning_state != Some("Succeeded".to_string()) {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Provisioning state changed from Succeeded to {:?}",
                            properties.provisioning_state
                        ),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(60)),
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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        // Nothing recorded: creation never ran, was refused, or succeeded with its response lost
        // before the name was saved. Only a direct setup creates the group itself, so only there
        // is the derived name looked up, and the group is deleted only when it carries this
        // deployment's tag.
        let name_was_recorded = self.resource_group_name.is_some();
        let derived;
        let group_name = match self.resource_group_name.as_ref() {
            Some(name) => name,
            None if ctx.initial_setup_authority
                == alien_core::InitialSetupAuthority::DirectSetup =>
            {
                derived = generate_azure_resource_group_name(&ctx.resource_prefix, &config.id);
                &derived
            }
            None => {
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(group_name=%group_name, "Initiating Azure Resource Group deletion");

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        // A group tagged for another deployment is never ours to delete. An untagged one is an
        // install an older build made (it adopted nothing it did not create only from now on).
        let marker = owner_marker(
            &ctx.resource_prefix,
            &config.id,
            ctx.setup_owner_deployment_id()?,
        );
        match client.get_resource_group(group_name).await {
            Ok(found) => {
                let owner = found.tags.get(RESOURCE_GROUP_OWNER_TAG);
                if !name_was_recorded && owner != Some(&marker) {
                    info!(group_name=%group_name, "Resource group is not tagged for this deployment, leaving it");
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                if let Some(owner) = owner {
                    if owner != &marker {
                        info!(group_name=%group_name, "Resource group belongs to another deployment, leaving it");
                        self.resource_group_name = None;
                        self.resource_id = None;
                        self.location = None;
                        return Ok(HandlerAction::Continue {
                            state: Deleted,
                            suggested_delay: None,
                        });
                    }
                }
            }
            // Gone already: the delete below reports it (nothing to delete if never recorded).
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                if !name_was_recorded {
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to read resource group '{group_name}' to check ownership."
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        match client.delete_resource_group(group_name).await {
            Ok(_) => {
                info!(group_name=%group_name, "Resource group deletion initiated");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(group_name=%group_name, "Resource group already deleted");

                self.resource_group_name = None;
                self.resource_id = None;
                self.location = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Failed to initiate resource group deletion");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = DeletingResourceGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_resource_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<AzureResourceGroup>()?;

        let group_name = self.resource_group_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Resource group name not set in state".to_string(),
            })
        })?;

        let client = ctx
            .service_provider
            .get_azure_resources_client(azure_config)?;

        match client.get_resource_group(group_name).await {
            Ok(_) => {
                debug!(group_name=%group_name, "Resource group still exists, continuing to wait");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(group_name=%group_name, "Resource group successfully deleted");

                self.resource_group_name = None;
                self.resource_id = None;
                self.location = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            Err(e) => {
                error!(group_name=%group_name, error=%e, "Error checking resource group deletion status");
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get resource group '{}'.", group_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingResourceGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
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
        if let (Some(resource_group_name), Some(resource_id), Some(location)) =
            (&self.resource_group_name, &self.resource_id, &self.location)
        {
            Some(ResourceOutputs::new(AzureResourceGroupOutputs {
                name: resource_group_name.clone(),
                resource_id: resource_id.clone(),
                location: location.clone(),
            }))
        } else {
            None
        }
    }
}

fn emit_azure_resource_group_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    rg: &ResourceGroup,
) {
    let provisioning_state = rg
        .properties
        .as_ref()
        .and_then(|properties| properties.provisioning_state.clone());
    let (health, lifecycle) = match provisioning_state.as_deref() {
        Some("Succeeded") => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
        Some(_) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Failed),
        None => (ObservedHealth::Unknown, ProviderLifecycleState::Unknown),
    };
    let name = rg.name.clone().unwrap_or_else(|| resource_id.to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: AzureResourceGroup::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::AzureResourceGroup(AzureResourceGroupHeartbeatData {
            status: AzureResourceGroupHeartbeatStatus {
                health,
                lifecycle,
                message: Some(format!(
                    "Azure resource group '{}' provisioning state is {}",
                    name,
                    provisioning_state.as_deref().unwrap_or("unknown")
                )),
                stale: false,
                partial: false,
                collection_issues: vec![],
            },
            name,
            resource_id: rg.id.clone(),
            location: Some(rg.location.clone()),
            provisioning_state,
            managed_tags: managed_tags(&rg.tags),
        }),
        raw: vec![],
    });
}

fn managed_tags(tags: &HashMap<String, String>) -> BTreeMap<String, String> {
    tags.iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                ALIEN_STACK_TAG_KEY | ALIEN_RESOURCE_TAG_KEY | ALIEN_MANAGED_BY_TAG_KEY
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Tag on the resource group this deployment creates; it proves ownership when a setup is
/// retried and on teardown.
const RESOURCE_GROUP_OWNER_TAG: &str = "alien-setup-owner";

fn owner_marker(resource_prefix: &str, id: &str, deployment_id: &str) -> String {
    crate::setup_owner::owner_value(resource_prefix, id, deployment_id)
}

impl AzureResourceGroupController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(resource_group_name: &str) -> Self {
        Self {
            state: AzureResourceGroupState::Ready,
            resource_group_name: Some(resource_group_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/{}",
                resource_group_name
            )),
            location: Some("East US".to_string()),
            _internal_stay_count: None,
        }
    }
}

/// Generates the full, prefixed Azure resource group name (pure function).
fn generate_azure_resource_group_name(resource_prefix: &str, id: &str) -> String {
    // Azure resource group names must be 1-90 characters, case insensitive
    // Format: {prefix}-{id} with length constraints and character restrictions
    let clean_prefix = resource_prefix
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == '(' || *c == ')'
        })
        .collect::<String>();
    let clean_id = id
        .chars()
        .filter(|c| {
            c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == '(' || *c == ')'
        })
        .collect::<String>();

    let combined = format!("{}-{}", clean_prefix, clean_id);

    // Truncate to 90 characters if necessary (Azure limit)
    if combined.len() > 90 {
        combined[..90].to_string()
    } else {
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::{test_azure_resource_group, SingleControllerExecutor};
    use crate::core::MockPlatformServiceProvider;
    use alien_azure_clients::azure::resources::MockResourcesApi;
    use std::sync::Arc;

    fn group(tag: Option<&str>) -> ResourceGroup {
        let mut tags = HashMap::new();
        if let Some(tag) = tag {
            tags.insert(RESOURCE_GROUP_OWNER_TAG.to_string(), tag.to_string());
        }
        serde_json::from_value::<ResourceGroup>(serde_json::json!({
            "name": "g", "location": "eastus",
            "properties": {"provisioningState": "Succeeded"},
        }))
        .map(|mut g| {
            g.tags = tags;
            g
        })
        .unwrap()
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "ResourceGroup".to_string(),
            resource_name: "g".to_string(),
        })
    }

    async fn executor(
        api: MockResourcesApi,
        controller: AzureResourceGroupController,
    ) -> SingleControllerExecutor {
        let api = Arc::new(api);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_resources_client()
            .returning(move |_| Ok(api.clone()));
        SingleControllerExecutor::builder()
            .resource(test_azure_resource_group())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor builds")
    }

    fn marker_for(executor_prefix: &str) -> String {
        owner_marker(executor_prefix, "default-resource-group", "dep_test")
    }

    /// A pre-existing group with the deployment's name and no tag is refused, and nothing is
    /// written to it.
    #[tokio::test]
    async fn an_existing_untagged_group_is_refused() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group()
            .returning(|_| Ok(group(None)));
        api.expect_create_or_update_resource_group().times(0);
        let mut executor = executor(api, AzureResourceGroupController::default()).await;
        let result = executor.step().await;
        let message = format!("{:?}", result.err());
        assert!(
            message.contains("did not create it") || executor.status() != ResourceStatus::Running
        );
        assert_ne!(executor.status(), ResourceStatus::Running);
    }

    /// An absent group is created create-only, with the owner tag naming this deployment.
    #[tokio::test]
    async fn an_absent_group_is_created_with_the_owner_tag() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group()
            .returning(|_| Err(not_found()));
        api.expect_create_or_update_resource_group().times(0);
        api.expect_create_resource_group_if_absent()
            .times(1)
            .withf(|_, body| {
                body.tags.get(RESOURCE_GROUP_OWNER_TAG).map(String::as_str)
                    == Some(marker_for("test").as_str())
            })
            .returning(|_, body| Ok(body.clone()));
        let mut executor = executor(api, AzureResourceGroupController::default()).await;
        executor.step().await.expect("create starts");
    }

    /// A group somebody else creates between the read and the write is refused, not taken over.
    #[tokio::test]
    async fn a_group_created_by_someone_else_in_between_is_refused() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group()
            .returning(|_| Err(not_found()));
        api.expect_create_resource_group_if_absent()
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(CloudClientErrorData::RemoteResourceConflict {
                    resource_type: "Azure resource group".to_string(),
                    resource_name: "g".to_string(),
                    message: "the resource group already exists".to_string(),
                }))
            });
        api.expect_create_or_update_resource_group().times(0);
        let mut executor = executor(api, AzureResourceGroupController::default()).await;
        let result = executor.step().await;
        let message = format!("{:?}", result.err());
        assert!(
            message.contains("created by someone else"),
            "{message}"
        );
        assert_ne!(executor.status(), ResourceStatus::Running);
    }

    /// Two deployments that reuse one prefix: the group the first made (tagged with its
    /// deployment id) is refused to the second, and destroying the second deletes nothing.
    #[tokio::test]
    async fn a_group_tagged_by_another_deployment_with_the_same_prefix_is_refused_and_kept() {
        let other = owner_marker("test", "default-resource-group", "dep_first");
        let mut api = MockResourcesApi::new();
        let create_tag = other.clone();
        api.expect_get_resource_group()
            .returning(move |_| Ok(group(Some(&create_tag))));
        api.expect_create_or_update_resource_group().times(0);
        api.expect_create_resource_group_if_absent().times(0);
        api.expect_delete_resource_group().times(0);
        let mut executor = executor(api, AzureResourceGroupController::default()).await;
        let result = executor.step().await;
        assert!(format!("{:?}", result.err()).contains("did not create it"));
        assert_ne!(executor.status(), ResourceStatus::Running);
        executor.delete().expect("delete starts");
        executor.step().await.expect("delete step");
        executor
            .run_until_status(ResourceStatus::Deleted)
            .await
            .expect("destroying the refused deployment deletes nothing");
    }

    /// A group this deployment tagged is an earlier attempt's own: setup converges on it.
    #[tokio::test]
    async fn a_group_tagged_for_this_deployment_is_adopted() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group().returning(|_| {
            // The prefix the harness uses is whatever the tag the controller computes carries.
            Ok(group(Some(&marker_for("test"))))
        });
        api.expect_create_or_update_resource_group()
            .times(1)
            .returning(|_, body| Ok(body.clone()));
        let mut executor = executor(api, AzureResourceGroupController::default()).await;
        executor.step().await.expect("adopts its own group");
    }

    /// State that already records the name (a retry, or an older build's install) is not
    /// refused for lacking a tag.
    #[tokio::test]
    async fn a_state_that_records_the_name_is_not_refused() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group().times(0);
        api.expect_create_or_update_resource_group()
            .times(1)
            .returning(|_, body| Ok(body.clone()));
        let mut controller = AzureResourceGroupController::default();
        controller.resource_group_name = Some("test-default-resource-group".to_string());
        let mut executor = executor(api, controller).await;
        executor.step().await.expect("retry goes on");
    }

    /// A refused create recorded nothing, and the group under the derived name is somebody
    /// else's (untagged, or tagged for another deployment): destroy deletes nothing.
    #[tokio::test]
    async fn delete_after_a_refused_create_deletes_nothing() {
        for tag in [None, Some("other/default-resource-group".to_string())] {
            let mut api = MockResourcesApi::new();
            api.expect_get_resource_group()
                .returning(move |_| Ok(group(tag.as_deref())));
            api.expect_delete_resource_group().times(0);
            let mut executor = executor(api, AzureResourceGroupController::default()).await;
            executor.delete().expect("delete starts");
            executor.step().await.expect("delete step");
            executor
                .run_until_status(ResourceStatus::Deleted)
                .await
                .expect("deleted");
        }
    }

    /// The create succeeded but its response was lost before the name was saved: the group under
    /// the derived name carries this deployment's tag, so destroy deletes it; one that was never
    /// created (not found) is nothing to delete.
    #[tokio::test]
    async fn delete_without_a_recorded_name_removes_a_group_tagged_for_this_deployment() {
        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group()
            .returning(|_| Ok(group(Some(&marker_for("test")))));
        api.expect_delete_resource_group().times(1).returning(|_| {
            Ok(alien_azure_clients::long_running_operation::OperationResult::Completed(()))
        });
        let mut run = executor(api, AzureResourceGroupController::default()).await;
        run.delete().expect("delete starts");
        run.step().await.expect("delete step");

        let mut api = MockResourcesApi::new();
        api.expect_get_resource_group()
            .returning(|_| Err(not_found()));
        api.expect_delete_resource_group().times(0);
        let mut run = executor(api, AzureResourceGroupController::default()).await;
        run.delete().expect("delete starts");
        run.step().await.expect("delete step");
        run.run_until_status(ResourceStatus::Deleted)
            .await
            .expect("deleted");
    }

    /// Destroy leaves a group tagged for another deployment, deletes an untagged one an older
    /// build created (state records it) and one this deployment tagged.
    #[tokio::test]
    async fn delete_removes_only_untagged_or_own_groups() {
        for (tag, deleted) in [
            (Some("other/default-resource-group".to_string()), false),
            (None, true),
            (Some(marker_for("test")), true),
        ] {
            let mut api = MockResourcesApi::new();
            api.expect_get_resource_group()
                .returning(move |_| Ok(group(tag.as_deref())));
            api.expect_delete_resource_group()
                .times(usize::from(deleted))
                .returning(|_| {
                    Ok(alien_azure_clients::long_running_operation::OperationResult::Completed(()))
                });
            let controller =
                AzureResourceGroupController::mock_ready("test-default-resource-group");
            let mut executor = executor(api, controller).await;
            executor.delete().expect("delete starts");
            executor.step().await.expect("delete step");
        }
    }
}

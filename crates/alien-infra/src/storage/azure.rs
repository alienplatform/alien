//! Controller for managing Azure Storage Containers.

use crate::{
    core::{AzurePermissionsHelper, ResourceControllerContext, ResourcePermissionsHelper},
    error::{ErrorData, Result},
    infra_requirements::azure_utils,
};
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::azure::models::blob::{
    BlobContainer, BlobServiceProperties, ContainerProperties, ContainerPropertiesPublicAccess,
};
use alien_azure_clients::azure::models::storage::{ManagementPolicyRule, StorageAccount};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureBlobStorageHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use std::{fmt::Debug, time::Duration};
use tracing::{debug, info, warn};

pub(crate) fn get_azure_container_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
        .to_lowercase()
        .replace('_', "-")
}

fn azure_permission_scope_and_context(
    ctx: &ResourceControllerContext<'_>,
    container_name: &str,
) -> Result<(Scope, alien_permissions::PermissionContext)> {
    let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;
    let resource_scope = Scope::Resource {
        resource_group_name: azure_utils::get_resource_group_name(ctx.state)?,
        resource_provider: "Microsoft.Storage".to_string(),
        parent_resource_path: Some(format!(
            "storageAccounts/{storage_account_name}/blobServices/default"
        )),
        resource_type: "containers".to_string(),
        resource_name: container_name.to_string(),
    };
    let permission_context =
        ResourcePermissionsHelper::build_azure_permission_context(ctx, container_name)?;
    Ok((resource_scope, permission_context))
}

async fn verify_shared_settings(
    ctx: &ResourceControllerContext<'_>,
    config: &Storage,
    container_name: &str,
) -> Result<()> {
    let azure_config = ctx.get_azure_config()?;
    let group = azure_utils::get_resource_group_name(ctx.state)?;
    let account = azure_utils::get_storage_account_name(ctx.state)?;
    let client = ctx
        .service_provider
        .get_azure_blob_container_client(azure_config)?;
    let setup_required = |setting: &str| {
        AlienError::new(ErrorData::SetupRerunRequired {
            resource_id: config.id.clone(),
            message: format!(
                "Azure account '{account}' does not confirm {setting}. \
                 Rerun setup with the complete container declarations; runtime cannot change \
                 shared account settings. Lifecycle expiry requires explicit setup activation."
            ),
        })
    };
    if config.public_read {
        let account_client = ctx
            .service_provider
            .get_azure_storage_accounts_client(azure_config)?;
        let actual = account_client
            .get_storage_account_properties(&group, &account)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to verify shared Azure public-access capability".into(),
                resource_id: Some(config.id.clone()),
            })?;
        if actual.properties.and_then(|p| p.allow_blob_public_access) != Some(true) {
            return Err(setup_required("public blob access"));
        }
    }
    if config.versioning || !config.cors_allowed_origins.is_empty() {
        let actual = client
            .get_blob_service_properties(&group, &account)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to verify shared Azure blob-service settings".into(),
                resource_id: Some(config.id.clone()),
            })?;
        let properties = actual
            .properties
            .as_ref()
            .ok_or_else(|| setup_required("blob-service properties"))?;
        if config.versioning && properties.is_versioning_enabled != Some(true) {
            return Err(setup_required("versioning"));
        }
        for origin in &config.cors_allowed_origins {
            let confirmed = properties.cors.as_ref().is_some_and(|cors| {
                cors.cors_rules.iter().any(|rule| {
                    rule.allowed_origins
                        .iter()
                        .any(|allowed| allowed == origin || allowed == "*")
                        && ["GET", "HEAD"].iter().all(|method| {
                            rule.allowed_methods
                                .iter()
                                .any(|allowed| allowed.to_string() == *method)
                        })
                        && rule.allowed_headers.iter().any(|header| header == "*")
                        && ["ETag", "Content-Range", "Accept-Ranges"]
                            .iter()
                            .all(|header| {
                                rule.exposed_headers.iter().any(|allowed| {
                                    allowed == "*" || allowed.eq_ignore_ascii_case(header)
                                })
                            })
                })
            });
            if !confirmed {
                return Err(setup_required(
                    "the declared CORS origins, methods and response headers",
                ));
            }
        }
    }
    // The account is setup-owned. Read its policy even after the last declaration
    // is removed: otherwise old rules keep deleting objects while runtime reports
    // the update as complete. A missing policy is valid only without declarations.
    let actual = match client.get_management_policy(&group, &account).await {
        Ok(policy) => policy,
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return if config.lifecycle_rules.is_empty() {
                Ok(())
            } else {
                Err(setup_required("the container lifecycle policy"))
            };
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: "Failed to verify shared Azure lifecycle policy".into(),
                resource_id: Some(config.id.clone()),
            }))
        }
    };
    let properties = actual
        .properties
        .as_ref()
        .ok_or_else(|| setup_required("the container lifecycle policy"))?;
    for desired in &config.lifecycle_rules {
        if !properties
            .policy
            .rules
            .iter()
            .any(|rule| lifecycle_rule_matches(rule, desired, container_name))
        {
            return Err(setup_required(
                "a container-scoped lifecycle rule with the declared expiry",
            ));
        }
    }
    for rule in &properties.policy.rules {
        // Azure defaults omitted `enabled` to true. Only an explicit false is an
        // installer's disabled choice, including the generated legacy Live gate.
        if rule.enabled == Some(false) || !lifecycle_rule_targets(rule, container_name) {
            continue;
        }
        let actions = &rule.definition.actions;
        let deletes_other_versions = actions
            .snapshot
            .as_ref()
            .is_some_and(|action| action.delete.is_some())
            || actions
                .version
                .as_ref()
                .is_some_and(|action| action.delete.is_some());
        if deletes_other_versions
            || (actions
                .base_blob
                .as_ref()
                .is_some_and(|action| action.delete.is_some())
                && !config
                    .lifecycle_rules
                    .iter()
                    .any(|desired| lifecycle_rule_matches(rule, desired, container_name)))
        {
            return Err(setup_required(
                "removal or disabling of obsolete container expiry rules",
            ));
        }
    }
    Ok(())
}

fn lifecycle_rule_matches(
    rule: &ManagementPolicyRule,
    desired: &alien_core::LifecycleRule,
    container: &str,
) -> bool {
    let prefix = format!(
        "{container}/{}",
        desired.prefix.as_deref().unwrap_or_default()
    );
    rule.definition.filters.as_ref().is_some_and(|filter| {
        filter.blob_index_match.is_empty()
            && filter.blob_types == ["blockBlob"]
            && filter.prefix_match == [prefix]
    }) && rule
        .definition
        .actions
        .base_blob
        .as_ref()
        .and_then(|base| base.delete.as_ref())
        .is_some_and(|delete| {
            delete.days_after_modification_greater_than == Some(f64::from(desired.days))
                && delete.days_after_creation_greater_than.is_none()
                && delete.days_after_last_access_time_greater_than.is_none()
                && delete.days_after_last_tier_change_greater_than.is_none()
        })
}

fn lifecycle_rule_targets(rule: &ManagementPolicyRule, container: &str) -> bool {
    let Some(filter) = &rule.definition.filters else {
        return true;
    };
    // Index filters may match some existing objects; they cannot prove safety.
    // Prefixes are lexical, including partial container names and account-wide "".
    let root = format!("{container}/");
    filter.prefix_match.is_empty()
        || filter
            .prefix_match
            .iter()
            .any(|prefix| root.starts_with(prefix) || prefix.starts_with(&root))
}

#[controller]
pub struct AzureStorageController {
    /// The name of the created Azure Storage Container
    pub(crate) container_name: Option<String>,
    /// The name of the Azure Storage Account (from infrastructure dependencies)
    pub(crate) storage_account_name: Option<String>,
    /// Full deterministic Azure resource IDs of runtime-owned role assignments.
    /// IDs are recorded before creation so partial failures remain cleanable.
    #[serde(default)]
    pub(crate) role_assignment_ids: Vec<String>,
    /// Whether the role assignment IDs were durably planned in an earlier controller step.
    #[serde(default)]
    pub(crate) role_assignments_planned: bool,
}

#[controller]
impl AzureStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        // Determine container name (idempotent)
        let container_name = if let Some(name) = &self.container_name {
            name.clone()
        } else {
            get_azure_container_name(ctx.resource_prefix, config.id())
        };

        info!(
            resource_id = %config.id(),
            container_name = %container_name,
            "Creating Azure Storage Container"
        );

        verify_shared_settings(ctx, config, &container_name).await?;

        // Get infrastructure dependencies
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

        // Store the storage account name in controller state for environment variables
        self.storage_account_name = Some(storage_account_name.clone());

        // Build container configuration
        let mut properties = ContainerProperties::default();
        properties.public_access = if config.public_read {
            Some(ContainerPropertiesPublicAccess::Blob)
        } else {
            Some(ContainerPropertiesPublicAccess::None)
        };

        let container_body = BlobContainer {
            properties: Some(properties),
            ..Default::default()
        };

        // Execute container creation
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_blob_container_client(azure_config)?;

        // Fail fast on any error - executor handles retries
        client
            .create_blob_container(
                &resource_group_name,
                &storage_account_name,
                &container_name,
                &container_body,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Storage Container '{}'",
                    container_name
                ),
                resource_id: Some(config.id().to_string()),
            })?;

        info!(
            "Successfully created Azure Storage Container '{}'",
            container_name
        );

        self.container_name = Some(container_name.clone());

        Ok(HandlerAction::Continue {
            state: PlanningPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = PlanningPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn planning_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        self.role_assignment_ids = if let Some(container_name) = &self.container_name {
            let (resource_scope, permission_context) =
                azure_permission_scope_and_context(ctx, container_name)?;
            AzurePermissionsHelper::plan_resource_scoped_role_assignment_ids(
                ctx,
                &config.id,
                "storage",
                resource_scope,
                &permission_context,
            )
            .await?
        } else {
            Vec::new()
        };
        self.role_assignments_planned = true;

        info!(
            resource_id = %config.id(),
            role_assignments = self.role_assignment_ids.len(),
            "Planned Azure Storage role assignments"
        );
        Ok(HandlerAction::Continue {
            state: ApplyingPermissions,
            suggested_delay: None,
        })
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
        let config = ctx.desired_resource_config::<Storage>()?;

        if !self.role_assignments_planned {
            return Ok(HandlerAction::Continue {
                state: PlanningPermissions,
                suggested_delay: None,
            });
        }

        info!(resource_id = %config.id(), "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_name) = &self.container_name {
            let (resource_scope, permission_context) =
                azure_permission_scope_and_context(ctx, container_name)?;
            AzurePermissionsHelper::apply_resource_scoped_permissions_from_checkpoint(
                ctx,
                &config.id,
                "storage",
                resource_scope,
                &permission_context,
                &self.role_assignment_ids,
            )
            .await?;
        }

        info!(resource_id = %config.id(), "Successfully applied resource-scoped permissions");

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
        let config = ctx.desired_resource_config::<Storage>()?;

        if let Some(container_name) = &self.container_name {
            verify_shared_settings(ctx, config, container_name).await?;
            let azure_config = ctx.get_azure_config()?;
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;
            let storage_accounts_client = ctx
                .service_provider
                .get_azure_storage_accounts_client(azure_config)?;

            // Check if container still exists
            let container = client
                .get_blob_container(&resource_group_name, &storage_account_name, container_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check Azure Storage Container during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let storage_account = storage_accounts_client
                .get_storage_account_properties(&resource_group_name, &storage_account_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get Azure Storage account during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            let blob_service = client
                .get_blob_service_properties(&resource_group_name, &storage_account_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get Azure Blob service during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_azure_storage_heartbeat(
                ctx,
                &config.id,
                &resource_group_name,
                &storage_account_name,
                container,
                storage_account,
                blob_service,
            );

            debug!(name = %config.id, container = %container_name, "Azure Storage Container exists and is accessible");
        }

        debug!(name = %config.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
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
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev = ctx.previous_resource_config::<Storage>()?;

        info!(resource_id = %config.id(), "Starting update of Azure Storage Container");

        let container_name = self.container_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container name not set during update".to_string(),
                resource_id: Some(config.id().to_string()),
            })
        })?;

        info!(
            resource_id = %config.id(),
            container_name,
            "Updating Azure Storage Container"
        );

        verify_shared_settings(ctx, config, container_name).await?;

        // Only perform update if public_read setting changed
        if config.public_read != prev.public_read {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

            let mut properties = ContainerProperties::default();
            properties.public_access = if config.public_read {
                Some(ContainerPropertiesPublicAccess::Blob)
            } else {
                Some(ContainerPropertiesPublicAccess::None)
            };
            let container_body = BlobContainer {
                properties: Some(properties),
                ..Default::default()
            };

            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;

            client
                .update_blob_container(
                    &resource_group_name,
                    &storage_account_name,
                    container_name,
                    &container_body,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update Azure Storage Container '{}': public_read change",
                        container_name
                    ),
                    resource_id: Some(config.id().to_string()),
                })?;
            info!(
                resource_id = %config.id(),
                container_name,
                "Updated public access to {}",
                config.public_read
            );
        }

        // Permission ownership is part of setup state even when the container configuration did
        // not change. Reconcile it on every setup update so enabling or disabling Remote Bindings
        // cannot leave missing or stale role assignments.
        let azure_config = ctx.get_azure_config()?;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;
        while let Some(assignment_id) = self.role_assignment_ids.last().cloned() {
            match authorization_client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        &error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to replace Azure Storage role assignment '{assignment_id}'"
                        ),
                        resource_id: Some(config.id().to_string()),
                    }));
                }
            }
            self.role_assignment_ids.pop();
        }
        let (resource_scope, permission_context) =
            azure_permission_scope_and_context(ctx, container_name)?;
        self.role_assignment_ids =
            AzurePermissionsHelper::plan_resource_scoped_role_assignment_ids(
                ctx,
                &config.id,
                "storage",
                resource_scope.clone(),
                &permission_context,
            )
            .await?;
        // A grant may succeed before its response is lost. Keep every planned ID
        // in the failed checkpoint so retry or delete can revoke partial grants.
        self.role_assignments_planned = true;
        AzurePermissionsHelper::apply_resource_scoped_permissions_from_checkpoint(
            ctx,
            &config.id,
            "storage",
            resource_scope,
            &permission_context,
            &self.role_assignment_ids,
        )
        .await?;

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
        let config = ctx.desired_resource_config::<Storage>()?;

        if !self.role_assignment_ids.is_empty() {
            let azure_config = ctx.get_azure_config()?;
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;

            for assignment_id in &self.role_assignment_ids {
                match authorization_client
                    .delete_role_assignment_by_id(assignment_id.clone())
                    .await
                {
                    Ok(_) => {
                        info!(
                            resource_id = %config.id(),
                            assignment_id,
                            "Deleted Azure Storage role assignment"
                        );
                    }
                    Err(error)
                        if matches!(
                            &error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(
                            resource_id = %config.id(),
                            assignment_id,
                            "Azure Storage role assignment was already deleted"
                        );
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete Azure role assignment '{}' for Storage",
                                assignment_id
                            ),
                            resource_id: Some(config.id().to_string()),
                        }));
                    }
                }
            }
            self.role_assignment_ids.clear();
        }

        Ok(HandlerAction::Continue {
            state: DeletingContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingContainer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        if let Some(container_name) = &self.container_name {
            info!(
                resource_id = %config.id(),
                container_name,
                "Deleting Azure Storage Container"
            );
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let storage_account_name = azure_utils::get_storage_account_name(ctx.state)?;

            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_blob_container_client(azure_config)?;

            match client
                .delete_blob_container(&resource_group_name, &storage_account_name, container_name)
                .await
            {
                Ok(_) => {
                    info!(
                        "Successfully deleted Azure Storage Container '{}'",
                        container_name
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        "Azure Storage Container '{}' was already deleted.",
                        container_name
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete Azure Storage Container '{}'",
                            container_name
                        ),
                        resource_id: Some(config.id().to_string()),
                    }));
                }
            }
        } else {
            warn!(resource_id = %config.id(), "Delete called on storage resource with no container name, assuming it was never created.");
        }

        self.container_name = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
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
        self.container_name.as_ref().map(|container_name| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: container_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::StorageBinding;

        if let (Some(storage_account_name), Some(container_name)) =
            (&self.storage_account_name, &self.container_name)
        {
            let binding =
                StorageBinding::blob(storage_account_name.clone(), container_name.clone());
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

fn emit_azure_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    storage_account_name: &str,
    container: BlobContainer,
    storage_account: StorageAccount,
    blob_service: BlobServiceProperties,
) {
    let container_name = container
        .name
        .clone()
        .unwrap_or_else(|| resource_id.to_string());
    let container_public_access = container
        .properties
        .as_ref()
        .and_then(|properties| properties.public_access.as_ref())
        .map(ToString::to_string);
    let account_properties = storage_account.properties.as_ref();
    let blob_service_properties = blob_service.properties.as_ref();
    let location = storage_account.location.clone();
    let sku_name = storage_account.sku.as_ref().map(|sku| sku.name.to_string());
    let sku_tier = storage_account
        .sku
        .as_ref()
        .and_then(|sku| sku.tier.as_ref())
        .map(ToString::to_string);
    let access_tier = account_properties
        .and_then(|properties| properties.access_tier.as_ref())
        .map(ToString::to_string);
    let encryption = account_properties.and_then(|properties| properties.encryption.as_ref());
    let encryption_services = encryption.and_then(|encryption| encryption.services.as_ref());
    let blob_delete_retention =
        blob_service_properties.and_then(|properties| properties.delete_retention_policy.as_ref());
    let container_delete_retention = blob_service_properties
        .and_then(|properties| properties.container_delete_retention_policy.as_ref());
    let change_feed =
        blob_service_properties.and_then(|properties| properties.change_feed.as_ref());
    let public_network_access = account_properties
        .and_then(|properties| properties.public_network_access.as_ref())
        .map(ToString::to_string);
    let allow_blob_public_access =
        account_properties.and_then(|properties| properties.allow_blob_public_access);
    let provisioning_state = account_properties
        .and_then(|properties| properties.provisioning_state.as_ref())
        .map(ToString::to_string);
    let health = match provisioning_state.as_deref() {
        Some("Succeeded") => ObservedHealth::Healthy,
        Some("Failed") => ObservedHealth::Unhealthy,
        _ => ObservedHealth::Degraded,
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Storage::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Storage(StorageHeartbeatData::AzureBlob(
            AzureBlobStorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "Azure Storage container '{}' metadata is reachable",
                        container_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: container_name,
                storage_account_name: Some(storage_account_name.to_string()),
                resource_group: Some(resource_group_name.to_string()),
                location: Some(location),
                account_kind: storage_account.kind.as_ref().map(ToString::to_string),
                sku_name,
                sku_tier,
                access_tier,
                provisioning_state,
                primary_location: account_properties
                    .and_then(|properties| properties.primary_location.clone()),
                secondary_location: account_properties
                    .and_then(|properties| properties.secondary_location.clone()),
                status_of_primary: account_properties
                    .and_then(|properties| properties.status_of_primary.as_ref())
                    .map(ToString::to_string),
                status_of_secondary: account_properties
                    .and_then(|properties| properties.status_of_secondary.as_ref())
                    .map(ToString::to_string),
                public_network_access,
                allow_blob_public_access,
                encryption_key_source: encryption
                    .map(|encryption| encryption.key_source.to_string()),
                blob_encryption_enabled: encryption_services
                    .and_then(|services| services.blob.as_ref())
                    .and_then(|service| service.enabled),
                file_encryption_enabled: encryption_services
                    .and_then(|services| services.file.as_ref())
                    .and_then(|service| service.enabled),
                queue_encryption_enabled: encryption_services
                    .and_then(|services| services.queue.as_ref())
                    .and_then(|service| service.enabled),
                table_encryption_enabled: encryption_services
                    .and_then(|services| services.table.as_ref())
                    .and_then(|service| service.enabled),
                blob_versioning_enabled: blob_service_properties
                    .and_then(|properties| properties.is_versioning_enabled),
                blob_delete_retention_enabled: blob_delete_retention
                    .and_then(|policy| policy.enabled),
                blob_delete_retention_days: blob_delete_retention
                    .and_then(|policy| policy.days)
                    .map(|days| days.get()),
                container_delete_retention_enabled: container_delete_retention
                    .and_then(|policy| policy.enabled),
                container_delete_retention_days: container_delete_retention
                    .and_then(|policy| policy.days)
                    .map(|days| days.get()),
                change_feed_enabled: change_feed.and_then(|feed| feed.enabled),
                change_feed_retention_days: change_feed
                    .and_then(|feed| feed.retention_in_days)
                    .map(|days| u64::from(days.get())),
                container_public_access,
            },
        )),
        raw: vec![],
    });
}

impl AzureStorageController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AzureStorageState::Ready,
            container_name: Some(get_azure_container_name("test-stack", storage_name)),
            storage_account_name: Some("test-storage-account".to_string()),
            role_assignment_ids: Vec::new(),
            role_assignments_planned: true,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Storage Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    use alien_azure_clients::models::blob::{
        BlobContainer, ContainerProperties, ContainerPropertiesPublicAccess,
    };
    use alien_azure_clients::{
        authorization::{AuthorizationApi, MockAuthorizationApi},
        blob_containers::{BlobContainerApi, MockBlobContainerApi},
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, LifecycleRule, Platform,
        ResourceLifecycle, ResourceStatus, Stack, StackSettings, StackState, Storage,
        StorageOutputs,
    };
    use alien_error::AlienError;
    use mockall::{predicate::eq, Sequence};
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider, StackExecutor, StackResourceStateExt,
    };
    use crate::storage::{fixtures::*, AzureStorageController};

    // ─────────────── MOCK SETUP HELPERS ────────────────────────

    fn create_successful_container_response(container_name: &str) -> BlobContainer {
        BlobContainer {
            name: Some(container_name.to_string()),
            properties: Some(ContainerProperties {
                public_access: Some(ContainerPropertiesPublicAccess::None),
                last_modified_time: Some("2023-01-01T00:00:00Z".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn install_shared_settings(
        mock: &mut MockBlobContainerApi,
        storage: &Storage,
        container: &str,
    ) {
        let properties = serde_json::json!({"properties":{
            "isVersioningEnabled": storage.versioning,
            "cors":{"corsRules":[{"allowedOrigins":storage.cors_allowed_origins,
                "allowedMethods":["GET","HEAD"],
                "allowedHeaders":["*"],"exposedHeaders":["ETag","Content-Range","Accept-Ranges"],
                "maxAgeInSeconds":3600}]}
        }});
        mock.expect_get_blob_service_properties()
            .returning(move |_, _| Ok(serde_json::from_value(properties.clone()).unwrap()));
        let rules: Vec<_> = storage.lifecycle_rules.iter().enumerate().map(|(index, rule)| {
            serde_json::json!({"name":format!("rule-{index}"),"enabled":true,"type":"Lifecycle",
                "definition":{"filters":{"blobTypes":["blockBlob"],
                    "prefixMatch":[format!("{container}/{}",rule.prefix.as_deref().unwrap_or_default())]},
                    "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":rule.days}}}}})
        }).collect();
        mock.expect_get_management_policy().returning(move |_, _| {
            Ok(
                serde_json::from_value(
                    serde_json::json!({"properties":{"policy":{"rules":rules}}}),
                )
                .unwrap(),
            )
        });
    }

    fn setup_mock_client_for_creation_and_deletion(
        container_name: &str,
        storage: &Storage,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        install_shared_settings(&mut mock_blob, storage, container_name);

        // Mock successful container creation
        let container_name = container_name.to_string();
        let container_name_clone1 = container_name.clone();

        mock_blob
            .expect_create_blob_container()
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));

        // Mock successful container deletion
        mock_blob
            .expect_delete_blob_container()
            .returning(|_, _, _| Ok(()));

        Arc::new(mock_blob)
    }

    fn setup_mock_client_for_creation_and_update(
        container_name: &str,
        storage: &Storage,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        install_shared_settings(
            &mut mock_blob,
            storage,
            &format!("test-stack-{}", storage.id),
        );

        // Mock container updates for public read changes
        let container_name = container_name.to_string();

        mock_blob
            .expect_update_blob_container()
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));

        Arc::new(mock_blob)
    }

    fn setup_mock_client_for_best_effort_deletion(
        _container_name: &str,
    ) -> Arc<MockBlobContainerApi> {
        let mut mock_blob = MockBlobContainerApi::new();

        // Mock container deletion failure (container doesn't exist)
        mock_blob
            .expect_delete_blob_container()
            .returning(|_, _, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Azure Blob Container".to_string(),
                        resource_name: "test-container".to_string(),
                    },
                ))
            });

        Arc::new(mock_blob)
    }

    fn setup_mock_service_provider(
        mock_blob: Arc<MockBlobContainerApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        setup_mock_service_provider_with_authorization(
            mock_blob,
            Arc::new(MockAuthorizationApi::new()),
        )
    }

    fn setup_mock_service_provider_with_authorization(
        mock_blob: Arc<MockBlobContainerApi>,
        authorization: Arc<MockAuthorizationApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(mock_blob.clone()));

        let mut accounts = alien_azure_clients::storage_accounts::MockStorageAccountsApi::new();
        accounts
            .expect_get_storage_account_properties()
            .returning(|_, _| {
                Ok(serde_json::from_value(
                    serde_json::json!({"location":"eastus","properties":{"allowBlobPublicAccess":true}}),
                )
                .unwrap())
            });
        let accounts = Arc::new(accounts);
        mock_provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));

        let authorization: Arc<dyn AuthorizationApi> = authorization;
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));

        Arc::new(mock_provider)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())]
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())]
    #[case::complete_config(storage_complete_config())] // Some features will be ignored with warnings
    #[case::custom_lifecycle(storage_custom_lifecycle())]
    #[case::versioning_only(storage_versioning_only())]
    #[case::public_only(storage_public_only())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] storage: Storage) {
        let container_name = format!("test-{}", storage.id);
        let mock_blob = setup_mock_client_for_creation_and_deletion(&container_name, &storage);
        let mock_provider = setup_mock_service_provider(mock_blob);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let storage_outputs = outputs.downcast_ref::<StorageOutputs>().unwrap();
        assert!(storage_outputs.bucket_name.starts_with("test-"));

        // Delete the storage
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn test_role_assignment_plan_is_serialized_before_apply_step() {
        let storage = basic_storage();
        let container_name = format!("test-{}", storage.id);
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_create_blob_container()
            .times(1)
            .returning(move |_, _, _, _| Ok(create_successful_container_response(&container_name)));
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(0);
        authorization
            .expect_create_or_update_role_definition()
            .times(0);
        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let resource_id = storage.id.clone();
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("Azure Storage executor should build");

        executor
            .step()
            .await
            .expect("container creation should reach permission planning");
        executor
            .step()
            .await
            .expect("permission planning should complete without Azure mutations");

        let controller = executor
            .internal_state::<AzureStorageController>()
            .expect("Azure Storage controller state");
        assert!(matches!(
            controller.state,
            crate::storage::azure::AzureStorageState::ApplyingPermissions
        ));
        assert!(controller.role_assignments_planned);

        let persisted = executor
            .stack_state()
            .resources
            .get(&resource_id)
            .and_then(|resource| resource.internal_state.as_ref())
            .expect("planned controller state should be serialized");
        assert_eq!(
            persisted
                .get("roleAssignmentsPlanned")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(persisted.get("roleAssignmentIds").is_some());
    }

    #[tokio::test]
    async fn interrupted_update_keeps_grants_for_serialized_retry_or_delete() {
        for lose_response in [false, true] {
            for delete_after_failure in [false, true] {
                let storage = basic_storage();
                let ids: Vec<String> = ["keep", "fail", "first"]
                    .map(|id| format!("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Authorization/roleAssignments/{id}"))
                    .into();
                let grants = Arc::new(Mutex::new(ids.iter().cloned().collect::<HashSet<_>>()));
                let blocked = Arc::new(AtomicBool::new(true));
                let observed = grants.clone();
                let failure = blocked.clone();
                let fail_id = ids[1].clone();
                let mut authorization = MockAuthorizationApi::new();
                authorization
                    .expect_delete_role_assignment_by_id()
                    .returning(move |id| {
                        let mut grants = observed.lock().unwrap();
                        if id == fail_id && failure.load(Ordering::SeqCst) {
                            if lose_response {
                                grants.remove(&id);
                            }
                            return Err(AlienError::new(if lose_response {
                                CloudClientErrorData::HttpRequestFailed {
                                    message: "response lost".into(),
                                }
                            } else {
                                CloudClientErrorData::RemoteAccessDenied {
                                    resource_type: "role assignment".into(),
                                    resource_name: id,
                                }
                            }));
                        }
                        if grants.remove(&id) {
                            Ok(None)
                        } else {
                            Err(AlienError::new(
                                CloudClientErrorData::RemoteResourceNotFound {
                                    resource_type: "role assignment".into(),
                                    resource_name: id,
                                },
                            ))
                        }
                    });
                let mut blob = MockBlobContainerApi::new();
                blob.expect_get_management_policy().returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "policy".into(),
                            resource_name: "default".into(),
                        },
                    ))
                });
                let observed = grants.clone();
                blob.expect_delete_blob_container()
                    .times(usize::from(delete_after_failure))
                    .returning(move |_, _, _| {
                        assert!(
                            observed.lock().unwrap().is_empty(),
                            "grants must be revoked before deleting the container"
                        );
                        Ok(())
                    });
                let provider = setup_mock_service_provider_with_authorization(
                    Arc::new(blob),
                    Arc::new(authorization),
                );
                let mut controller = AzureStorageController::mock_ready(&storage.id);
                controller.role_assignment_ids = ids.clone();
                let mut seed = SingleControllerExecutor::builder()
                    .resource(storage.clone())
                    .resource_lifecycle(ResourceLifecycle::Live)
                    .controller(controller)
                    .platform(Platform::Azure)
                    .service_provider(provider.clone())
                    .with_test_dependencies()
                    .build()
                    .await
                    .unwrap();
                seed.update(storage.clone()).unwrap();
                let client = seed.with_context(|ctx| ctx.client_config.clone());
                let mut state = seed.stack_state().clone();
                for (id, resource) in &mut state.resources {
                    if id != &storage.id {
                        resource.lifecycle = Some(ResourceLifecycle::Frozen);
                    }
                }
                let desired = Stack::new("grant-retry".into())
                    .add(storage.clone(), ResourceLifecycle::Live)
                    .build();
                let config = DeploymentConfig::builder()
                    .stack_settings(StackSettings::default())
                    .environment_variables(EnvironmentVariablesSnapshot {
                        variables: vec![],
                        hash: String::new(),
                        created_at: String::new(),
                    })
                    .external_bindings(ExternalBindings::default())
                    .allow_frozen_changes(false)
                    .build();
                let executor = StackExecutor::builder(&desired, client.clone())
                    .deployment_config(&config)
                    .lifecycle_filter(vec![ResourceLifecycle::Live])
                    .step_out_of_scope_resources(false)
                    .step_running_resources(false)
                    .service_provider(provider.clone())
                    .build()
                    .unwrap();
                for _ in 0..10 {
                    state = executor.step(state).await.unwrap().next_state;
                    state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
                    if state.resources[&storage.id].status == ResourceStatus::UpdateFailed {
                        break;
                    }
                }
                assert_eq!(
                    state.resources[&storage.id].status,
                    ResourceStatus::UpdateFailed
                );
                let failed = state.resources[&storage.id]
                    .get_last_failed_controller()
                    .unwrap()
                    .unwrap();
                let checkpoint = failed
                    .as_any()
                    .downcast_ref::<AzureStorageController>()
                    .unwrap();
                assert_eq!(checkpoint.role_assignment_ids, ids[..2]);
                assert!(checkpoint.container_name.is_some());
                assert!(grants.lock().unwrap().contains(&ids[0]));
                blocked.store(false, Ordering::SeqCst);
                let next = if delete_after_failure {
                    Stack::new("grant-retry".into()).build()
                } else {
                    assert!(state
                        .resources
                        .get_mut(&storage.id)
                        .unwrap()
                        .retry_failed()
                        .unwrap());
                    desired.clone()
                };
                let executor = StackExecutor::builder(&next, client)
                    .deployment_config(&config)
                    .lifecycle_filter(vec![ResourceLifecycle::Live])
                    .step_out_of_scope_resources(false)
                    .step_running_resources(false)
                    .service_provider(provider)
                    .build()
                    .unwrap();
                let expected = if delete_after_failure {
                    ResourceStatus::Deleted
                } else {
                    ResourceStatus::Running
                };
                for _ in 0..10 {
                    state = executor.step(state).await.unwrap().next_state;
                    state =
                        serde_json::from_value::<StackState>(serde_json::to_value(state).unwrap())
                            .unwrap();
                    if state.resources[&storage.id].status == expected {
                        break;
                    }
                }
                assert_eq!(state.resources[&storage.id].status, expected);
                assert!(grants.lock().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn lost_update_grant_response_remains_cleanable_after_reload() {
        let storage = basic_storage();
        let grants = Arc::new(Mutex::new(HashSet::new()));
        let observed = grants.clone();
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(1)
            .returning(move |id, _| {
                observed.lock().unwrap().insert(id);
                Err(AlienError::new(CloudClientErrorData::HttpRequestFailed {
                    message: "grant response lost".into(),
                }))
            });
        let observed = grants.clone();
        authorization
            .expect_delete_role_assignment_by_id()
            .times(1)
            .returning(move |id| {
                assert!(observed.lock().unwrap().remove(&id));
                Ok(None)
            });
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        let observed = grants.clone();
        blob.expect_delete_blob_container()
            .times(1)
            .returning(move |_, _, _| {
                assert!(observed.lock().unwrap().is_empty());
                Ok(())
            });
        let provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let mut run = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .resource_lifecycle(ResourceLifecycle::Live)
            .permission_profile(
                "reader",
                alien_core::PermissionProfile::new().resource(&storage.id, ["storage/data-read"]),
            )
            .controller(AzureStorageController::mock_ready(&storage.id))
            .platform(Platform::Azure)
            .service_provider(provider.clone())
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        run.update(storage.clone()).unwrap();
        run.step()
            .await
            .expect_err("grant succeeded but response was lost");
        let value =
            serde_json::to_value(run.internal_state::<AzureStorageController>().unwrap()).unwrap();
        let controller: AzureStorageController = serde_json::from_value(value).unwrap();
        assert_eq!(controller.role_assignment_ids.len(), 1);
        assert_eq!(
            controller
                .role_assignment_ids
                .iter()
                .cloned()
                .collect::<HashSet<_>>(),
            *grants.lock().unwrap()
        );
        let mut cleanup = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        cleanup.delete().unwrap();
        cleanup.run_until_terminal().await.unwrap();
        assert_eq!(cleanup.status(), ResourceStatus::Deleted);
        assert!(grants.lock().unwrap().is_empty());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_public_read(basic_storage(), storage_with_public_read())]
    #[case::public_read_to_basic(storage_with_public_read(), basic_storage())]
    #[case::versioning_to_public_read(storage_with_versioning(), storage_with_public_read())]
    #[case::lifecycle_to_public_read(storage_with_lifecycle_rules(), storage_with_public_read())]
    #[case::for_update_test_to_complete(storage_for_update_test(), storage_complete_config())] // Mixed feature changes
    #[case::public_only_to_versioning_only(storage_public_only(), storage_versioning_only())]
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_storage: Storage, #[case] to_storage: Storage) {
        // Ensure both storages have the same ID for valid updates
        let storage_id = "test-update-storage".to_string();
        let mut from_storage = from_storage;
        from_storage.id = storage_id.clone();

        let mut to_storage = to_storage;
        to_storage.id = storage_id.clone();

        let container_name = format!("test-{}", storage_id);
        let mock_blob = setup_mock_client_for_creation_and_update(&container_name, &to_storage);
        let mock_provider = setup_mock_service_provider(mock_blob);

        // Start with the "from" storage in Ready state
        let ready_controller = AzureStorageController::mock_ready(&storage_id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new storage
        executor.update(to_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())]
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())]
    #[case::complete_config(storage_complete_config())]
    #[case::versioning_only(storage_versioning_only())]
    #[case::public_only(storage_public_only())]
    #[tokio::test]
    async fn test_best_effort_deletion_when_container_missing(#[case] storage: Storage) {
        let container_name = format!("test-{}", storage.id);
        let mock_blob = setup_mock_client_for_best_effort_deletion(&container_name);
        let mock_provider = setup_mock_service_provider(mock_blob);

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though container deletion fails
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn test_role_assignments_are_deleted_before_container() {
        let storage = basic_storage();
        let assignment_id = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/assignment-123";
        let mut sequence = Sequence::new();

        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_delete_role_assignment_by_id()
            .with(eq(assignment_id.to_string()))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(None));

        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_delete_blob_container()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));

        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let mut controller = AzureStorageController::mock_ready(&storage.id);
        controller.role_assignment_ids = vec![assignment_id.to_string()];
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_missing_role_assignment_does_not_block_container_deletion() {
        let storage = basic_storage();
        let assignment_id = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/missing-assignment";
        let mut sequence = Sequence::new();

        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_delete_role_assignment_by_id()
            .with(eq(assignment_id.to_string()))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Azure role assignment".to_string(),
                        resource_name: "missing-assignment".to_string(),
                    },
                ))
            });

        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_delete_blob_container()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| Ok(()));

        let mock_provider =
            setup_mock_service_provider_with_authorization(Arc::new(blob), Arc::new(authorization));
        let mut controller = AzureStorageController::mock_ready(&storage.id);
        controller.role_assignment_ids = vec![assignment_id.to_string()];
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_no_container_name() {
        let storage = basic_storage();

        // Create a controller with no container name set (simulates a failed creation)
        let controller = AzureStorageController {
            state: crate::storage::azure::AzureStorageState::Ready,
            container_name: None, // No container name set
            storage_account_name: None,
            role_assignment_ids: Vec::new(),
            role_assignments_planned: true,
            _internal_stay_count: None,
        };

        let mock_blob = MockBlobContainerApi::new();
        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though no container was created
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies correct container naming convention
    #[tokio::test]
    async fn test_container_naming_validation() {
        let mut storage = basic_storage();
        storage.id = "My_Awesome_Storage".to_string(); // Use special naming case for this test

        let mut mock_blob = MockBlobContainerApi::new();
        mock_blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });

        // Validate that container names are converted to lowercase and underscores replaced with dashes
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, container_name, _| container_name == "test-my-awesome-storage")
            .returning(|_, _, container_name, _| {
                Ok(create_successful_container_response(container_name))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public read configuration is applied correctly
    #[tokio::test]
    async fn test_public_read_configuration() {
        let storage = storage_with_public_read();

        let mut mock_blob = MockBlobContainerApi::new();
        mock_blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });

        // Validate that public read configuration is set correctly during creation
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::Blob
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                let mut response = create_successful_container_response(container_name);
                if let Some(properties) = &mut response.properties {
                    properties.public_access = Some(ContainerPropertiesPublicAccess::Blob);
                }
                Ok(response)
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies private access configuration is applied correctly
    #[tokio::test]
    async fn test_private_access_configuration() {
        let storage = basic_storage(); // By default, public_read is false

        let mut mock_blob = MockBlobContainerApi::new();
        mock_blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });

        // Validate that private access configuration is set correctly during creation
        mock_blob
            .expect_create_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::None
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                Ok(create_successful_container_response(container_name))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public access update configuration is applied correctly
    #[tokio::test]
    async fn test_public_access_update_configuration() {
        let initial_storage = storage_for_update_test(); // Has public_read = false
        let mut updated_storage = storage_with_public_read(); // Has public_read = true
        updated_storage.id = initial_storage.id.clone(); // Ensure same ID for valid update

        let mut mock_blob = MockBlobContainerApi::new();
        mock_blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });

        // Validate that the update call has the correct public access configuration
        mock_blob
            .expect_update_blob_container()
            .withf(|_, _, _, blob_container| {
                if let Some(properties) = &blob_container.properties {
                    if let Some(public_access) = &properties.public_access {
                        *public_access == ContainerPropertiesPublicAccess::Blob
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .returning(|_, _, container_name, _| {
                let mut response = create_successful_container_response(container_name);
                if let Some(properties) = &mut response.properties {
                    properties.public_access = Some(ContainerPropertiesPublicAccess::Blob);
                }
                Ok(response)
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_blob));

        // Start with a ready controller
        let ready_controller = AzureStorageController::mock_ready(&initial_storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(initial_storage)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to enable public read
        executor.update(updated_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    #[tokio::test]
    async fn lifecycle_setup_choices_and_removed_rules_are_checked_at_every_entry() {
        for phase in 0..3 {
            for case in [
                "disabled",
                "enabled",
                "default-enabled",
                "removed",
                "removed-disabled",
                "sibling",
                "partial-prefix",
                "broad",
                "version",
                "snapshot",
                "tier-only",
                "absent",
                "denied",
                "missing-declared",
                "disabled-wrong-expiry",
            ] {
                let mut storage = basic_storage();
                let declared = matches!(
                    case,
                    "disabled"
                        | "enabled"
                        | "default-enabled"
                        | "missing-declared"
                        | "disabled-wrong-expiry"
                );
                if declared {
                    storage.lifecycle_rules = vec![LifecycleRule {
                        days: 30,
                        prefix: None,
                    }];
                }
                let container = if phase == 0 {
                    "test-basic-storage"
                } else {
                    "test-stack-basic-storage"
                };
                let mut rule = serde_json::json!({"name":"rule", "enabled":true,"type":"Lifecycle",
                    "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":[format!("{container}/")]},
                    "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":30}}}}});
                match case {
                    "disabled" | "removed-disabled" => rule["enabled"] = false.into(),
                    "default-enabled" => {
                        rule.as_object_mut().unwrap().remove("enabled");
                    }
                    "disabled-wrong-expiry" => {
                        rule["enabled"] = false.into();
                        rule["definition"]["actions"]["baseBlob"]["delete"]
                            ["daysAfterModificationGreaterThan"] = 1.into();
                    }
                    "sibling" => {
                        rule["definition"]["filters"]["prefixMatch"] =
                            serde_json::json!([format!("{container}-sibling/")])
                    }
                    "partial-prefix" => {
                        rule["definition"]["filters"]["prefixMatch"] = serde_json::json!(["test"])
                    }
                    "broad" => rule["definition"]["filters"]["prefixMatch"] = serde_json::json!([]),
                    "version" | "snapshot" => {
                        rule["definition"]["actions"] =
                            serde_json::json!({case:{"delete":{"daysAfterCreationGreaterThan":30}}})
                    }
                    "tier-only" => {
                        rule["definition"]["actions"] = serde_json::json!({"baseBlob":{"tierToCool":{"daysAfterModificationGreaterThan":30}}})
                    }
                    _ => {}
                }
                let succeeds = matches!(
                    case,
                    "disabled"
                        | "enabled"
                        | "default-enabled"
                        | "removed-disabled"
                        | "sibling"
                        | "tier-only"
                        | "absent"
                );
                let mut blob = MockBlobContainerApi::new();
                blob.expect_get_management_policy()
                    .times(1)
                    .returning(move |_, _| {
                        if matches!(case, "absent" | "missing-declared") {
                            return Err(AlienError::new(
                                CloudClientErrorData::RemoteResourceNotFound {
                                    resource_type: "policy".into(),
                                    resource_name: "default".into(),
                                },
                            ));
                        }
                        if case == "denied" {
                            return Err(AlienError::new(
                                CloudClientErrorData::RemoteAccessDenied {
                                    resource_type: "policy".into(),
                                    resource_name: "default".into(),
                                },
                            ));
                        }
                        Ok(serde_json::from_value(
                            serde_json::json!({"properties":{"policy":{"rules":[rule]}}}),
                        )
                        .unwrap())
                    });
                blob.expect_create_blob_container()
                    .times(usize::from(succeeds && phase == 0))
                    .returning(|_, _, name, _| Ok(create_successful_container_response(name)));
                blob.expect_get_blob_container()
                    .times(usize::from(succeeds && phase == 2))
                    .returning(|_, _, name| Ok(create_successful_container_response(name)));
                blob.expect_get_blob_service_properties().returning(|_, _| {
                    Ok(serde_json::from_value(serde_json::json!({"properties":{}})).unwrap())
                });
                blob.expect_update_blob_container().times(0);
                blob.expect_set_management_policy().times(0);
                let mut controller = if phase == 0 {
                    AzureStorageController::default()
                } else {
                    AzureStorageController::mock_ready(&storage.id)
                };
                if phase == 1 {
                    controller.state = super::AzureStorageState::UpdateStart;
                }
                let mut run = SingleControllerExecutor::builder()
                    .resource(storage.clone())
                    .previous_resource(storage)
                    .controller(
                        serde_json::from_value::<AzureStorageController>(
                            serde_json::to_value(controller).unwrap(),
                        )
                        .unwrap(),
                    )
                    .platform(Platform::Azure)
                    .service_provider(setup_mock_service_provider(Arc::new(blob)))
                    .with_test_dependencies()
                    .build()
                    .await
                    .unwrap();
                let result = run.step().await;
                if succeeds {
                    result.unwrap_or_else(|error| panic!("{phase}/{case}: {error}"));
                } else {
                    assert!(result.is_err(), "{phase}/{case} must refuse before writes");
                }
            }
        }
    }

    #[tokio::test]
    async fn shared_settings_refusal_survives_create_update_and_legacy_ready_checkpoints() {
        for phase in 0..3 {
            for kind in [
                "versioning",
                "cors",
                "lifecycle",
                "wrong-prefix",
                "wrong-expiry",
                "missing-head",
                "missing-etag",
            ] {
                let mut storage = basic_storage();
                let mut service = serde_json::json!({"properties":{
                    "isVersioningEnabled":false,
                    "cors":{"corsRules":[{"allowedOrigins":["https://example.com"],
                        "allowedMethods":["GET","HEAD"],
                        "allowedHeaders":["*"],"exposedHeaders":["ETag","Content-Range","Accept-Ranges"],
                        "maxAgeInSeconds":3600}]}
                }});
                let prefix = if phase == 0 {
                    "test-basic-storage/"
                } else {
                    "test-stack-basic-storage/"
                };
                let mut policy = serde_json::json!({"properties":{"policy":{"rules":[{
                    "name":"rule", "enabled":true,"type":"Lifecycle",
                    "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":[prefix]},
                        "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":30}}}}
                }]}}});
                match kind {
                    "versioning" => storage.versioning = true,
                    "cors" | "missing-head" | "missing-etag" => {
                        storage.cors_allowed_origins = vec!["https://example.com".into()];
                        match kind {
                            "missing-head" => {
                                service["properties"]["cors"]["corsRules"][0]["allowedMethods"] =
                                    serde_json::json!(["GET", "PUT", "POST", "DELETE", "OPTIONS"])
                            }
                            "missing-etag" => {
                                service["properties"]["cors"]["corsRules"][0]["exposedHeaders"] =
                                    serde_json::json!(["Content-Range", "Accept-Ranges"])
                            }
                            _ => service["properties"]["cors"]["corsRules"] = serde_json::json!([]),
                        }
                    }
                    _ => {
                        storage.lifecycle_rules = vec![LifecycleRule {
                            days: 30,
                            prefix: None,
                        }];
                        match kind {
                            "wrong-prefix" => {
                                policy["properties"]["policy"]["rules"][0]["definition"]
                                    ["filters"]["prefixMatch"] = serde_json::json!(["sibling/"])
                            }
                            "wrong-expiry" => {
                                policy["properties"]["policy"]["rules"][0]["definition"]
                                    ["actions"]["baseBlob"]["delete"]
                                    ["daysAfterModificationGreaterThan"] = 1.into()
                            }
                            _ => policy = serde_json::json!({}),
                        }
                    }
                }
                let mut blob = MockBlobContainerApi::new();
                blob.expect_get_blob_service_properties()
                    .returning(move |_, _| Ok(serde_json::from_value(service.clone()).unwrap()));
                blob.expect_get_management_policy()
                    .returning(move |_, _| Ok(serde_json::from_value(policy.clone()).unwrap()));
                blob.expect_create_blob_container().times(0);
                blob.expect_update_blob_container().times(0);
                blob.expect_set_blob_service_properties().times(0);
                blob.expect_set_management_policy().times(0);
                let provider = setup_mock_service_provider(Arc::new(blob));
                let mut controller = if phase > 0 {
                    AzureStorageController::mock_ready(&storage.id)
                } else {
                    AzureStorageController::default()
                };
                if phase == 1 {
                    controller.state = super::AzureStorageState::UpdateStart;
                }
                let checkpoint = serde_json::to_value(controller).unwrap();
                let mut executor = SingleControllerExecutor::builder()
                    .resource(storage.clone())
                    .previous_resource(storage.clone())
                    .controller(
                        serde_json::from_value::<AzureStorageController>(checkpoint).unwrap(),
                    )
                    .platform(Platform::Azure)
                    .service_provider(provider)
                    .with_test_dependencies()
                    .build()
                    .await
                    .unwrap();
                let error = executor.step().await.unwrap_err();
                assert_eq!(error.code, "SETUP_RERUN_REQUIRED", "{phase}/{kind}");
                assert_eq!(
                    executor.status(),
                    match phase {
                        0 => ResourceStatus::Provisioning,
                        1 => ResourceStatus::Updating,
                        _ => ResourceStatus::Running,
                    }
                );
            }
        }
    }
    #[tokio::test]
    async fn unchanged_imported_ready_checks_setup_and_recovers_after_serialized_retry() {
        for (lifecycle, removed_expiry) in [
            (ResourceLifecycle::Live, false),
            (ResourceLifecycle::Frozen, false),
            (ResourceLifecycle::Live, true),
            (ResourceLifecycle::Frozen, true),
        ] {
            let storage = if removed_expiry {
                basic_storage()
            } else {
                storage_with_versioning()
            };
            let configured = Arc::new(AtomicBool::new(false));
            let observed = configured.clone();
            let mut blob = MockBlobContainerApi::new();
            blob.expect_get_blob_service_properties()
                .returning(move |_, _| {
                    Ok(serde_json::from_value(serde_json::json!({"properties":{
                        "isVersioningEnabled":observed.load(Ordering::SeqCst)
                    }}))
                    .unwrap())
                });
            let observed = configured.clone();
            blob.expect_get_management_policy().returning(move |_, _| {
                let rules = if removed_expiry { vec![serde_json::json!({
                    "name":"obsolete", "enabled":!observed.load(Ordering::SeqCst), "type":"Lifecycle",
                    "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":["test-stack-basic-storage/"]},
                    "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":30}}}}
                })] } else { vec![] };
                Ok(serde_json::from_value(serde_json::json!({"properties":{"policy":{"rules":rules}}})).unwrap())
            });
            let observed = configured.clone();
            blob.expect_get_blob_container()
                .returning(move |_, _, name| {
                    assert!(observed.load(Ordering::SeqCst));
                    Ok(create_successful_container_response(name))
                });
            blob.expect_create_blob_container().times(0);
            blob.expect_update_blob_container().times(0);
            blob.expect_set_blob_service_properties().times(0);
            blob.expect_set_management_policy().times(0);
            let provider = setup_mock_service_provider(Arc::new(blob));
            let seed = SingleControllerExecutor::builder()
                .resource(storage.clone())
                .resource_lifecycle(lifecycle)
                .controller(AzureStorageController::mock_ready(&storage.id))
                .platform(Platform::Azure)
                .service_provider(provider.clone())
                .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            let client = seed.with_context(|ctx| ctx.client_config.clone());
            let mut state = seed.stack_state().clone();
            for (id, resource) in &mut state.resources {
                if id != &storage.id {
                    resource.lifecycle = Some(if lifecycle == ResourceLifecycle::Live {
                        ResourceLifecycle::Frozen
                    } else {
                        ResourceLifecycle::Live
                    });
                }
            }
            let mut desired = seed.with_context(|ctx| ctx.desired_stack.clone());
            for (id, entry) in &mut desired.resources {
                if id != &storage.id {
                    entry.lifecycle = if lifecycle == ResourceLifecycle::Live {
                        ResourceLifecycle::Frozen
                    } else {
                        ResourceLifecycle::Live
                    };
                }
            }
            // The fixture runs preflight mutations after seeding state. Model a
            // completed installation with those same recorded dependency edges.
            let entry = &desired.resources[&storage.id];
            let installed = state.resources.get_mut(&storage.id).unwrap();
            installed.config = entry.config.clone();
            installed.previous_config = Some(entry.config.clone());
            installed.dependencies = entry.combined_dependencies();
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(ExternalBindings::default())
                .allow_frozen_changes(false)
                .build();
            let executor = StackExecutor::builder(&desired, client)
                .deployment_config(&config)
                .lifecycle_filter(vec![lifecycle])
                .step_out_of_scope_resources(false)
                .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
                .service_provider(provider)
                .build()
                .unwrap();
            assert!(executor.plan(&state).unwrap().updates.is_empty());
            state = executor.step(state).await.unwrap().next_state;
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            assert_eq!(
                state.resources[&storage.id].status,
                ResourceStatus::RefreshFailed
            );
            assert!(state.resources[&storage.id]
                .get_last_failed_controller()
                .unwrap()
                .is_some());
            configured.store(true, Ordering::SeqCst);
            assert!(state
                .resources
                .get_mut(&storage.id)
                .unwrap()
                .retry_failed()
                .unwrap());
            state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(state.resources[&storage.id].status, ResourceStatus::Running);
            assert!(state.resources[&storage.id].outputs.is_some());
        }
    }
    #[tokio::test]
    async fn public_container_refuses_disabled_parent_before_writes() {
        let mut accounts = alien_azure_clients::storage_accounts::MockStorageAccountsApi::new();
        accounts
            .expect_get_storage_account_properties()
            .times(1)
            .returning(|_, _| {
                Ok(serde_json::from_value(
                    serde_json::json!({"location":"eastus","properties":{"allowBlobPublicAccess":false}}),
                )
                .unwrap())
            });
        let accounts = Arc::new(accounts);
        let mut blob = MockBlobContainerApi::new();
        blob.expect_get_management_policy().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "policy".into(),
                    resource_name: "default".into(),
                },
            ))
        });
        blob.expect_create_blob_container().times(0);
        let blob = Arc::new(blob);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_storage_accounts_client()
            .returning(move |_| Ok(accounts.clone()));
        provider
            .expect_get_azure_blob_container_client()
            .returning(move |_| Ok(blob.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage_with_public_read())
            .controller(AzureStorageController::default())
            .platform(Platform::Azure)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        assert_eq!(
            executor.step().await.unwrap_err().code,
            "SETUP_RERUN_REQUIRED"
        );
    }
}

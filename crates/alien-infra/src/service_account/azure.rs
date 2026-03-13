use std::time::Duration;
use tracing::info;
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_azure_clients::models::managed_identity::Identity;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::{AzureRoleDefinition, AzureRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use std::collections::HashMap;

/// Generates the Azure managed identity name.
fn get_azure_managed_identity_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

/// Generates the Azure custom role name.
fn get_azure_custom_role_name(prefix: &str, name: &str) -> String {
    format!("{}-{}-role", prefix, name)
}

#[controller]
pub struct AzureServiceAccountController {
    /// The resource ID of the created user-assigned managed identity.
    pub(crate) identity_resource_id: Option<String>,
    /// The client ID of the created user-assigned managed identity.
    pub(crate) identity_client_id: Option<String>,
    /// The principal ID of the created user-assigned managed identity.
    pub(crate) identity_principal_id: Option<String>,
    /// Resource IDs of created custom role definitions.
    pub(crate) custom_role_definition_ids: Vec<String>,
    /// Resource IDs of created role assignments.
    pub(crate) role_assignment_ids: Vec<String>,
    /// Whether stack-level permissions have been applied
    pub(crate) stack_permissions_applied: bool,
}

#[controller]
impl AzureServiceAccountController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingManagedIdentity,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

        let azure_cfg = ctx.get_azure_config()?;
        let location = azure_cfg.region.as_deref().unwrap_or("eastus");

        let identity = Identity {
            id: None,
            location: location.to_string(),
            name: None,
            properties: None,
            system_data: None,
            tags: HashMap::new(),
            type_: None,
        };

        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;
        let created_identity = client
            .create_or_update_user_assigned_identity(
                &resource_group_name,
                &identity_name,
                &identity,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create managed identity '{}'", identity_name),
                resource_id: Some(config.id.clone()),
            })?;

        let identity_id = created_identity.id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let properties = created_identity.properties.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing properties".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let principal_id = properties.principal_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing principal ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let client_id = properties.client_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing client ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            identity_name = %identity_name,
            identity_id = %identity_id,
            principal_id = %principal_id,
            client_id = %client_id,
            "Managed identity created successfully"
        );

        self.identity_resource_id = Some(identity_id);
        self.identity_client_id = Some(client_id.to_string());
        self.identity_principal_id = Some(principal_id.to_string());

        info!("Waiting 30 seconds for managed identity to propagate across Azure tenants");

        Ok(HandlerAction::Continue {
            state: CreatingRoleDefinitions,
            suggested_delay: Some(std::time::Duration::from_secs(30)),
        })
    }

    #[handler(
        state = CreatingRoleDefinitions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role_definitions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            stack_permission_sets_count = config.stack_permission_sets.len(),
            "Creating Azure role definitions for stack-level permission sets"
        );

        // Generate role definitions for all stack-level permission sets
        let role_definitions = self.generate_stack_role_definitions(config, ctx)?;

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        for (index, role_def) in role_definitions.iter().enumerate() {
            let role_name = format!(
                "{}-{}",
                get_azure_custom_role_name(ctx.resource_prefix, &config.id),
                index
            );
            // Deterministic UUID so re-running the same deployment updates
            // the existing role definition instead of creating a duplicate.
            let role_definition_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("alien:azure:stack-role-def:{}", role_name).as_bytes(),
            )
            .to_string();

            let role_definition_props = RoleDefinitionProperties {
                role_name: Some(role_name.clone()),
                description: Some(role_def.description.clone()),
                type_: Some("CustomRole".to_string()),
                permissions: vec![Permission {
                    actions: role_def.actions.clone(),
                    not_actions: Vec::new(),
                    data_actions: role_def.data_actions.clone(),
                    not_data_actions: Vec::new(),
                }],
                assignable_scopes: role_def.assignable_scopes.clone(),
                ..Default::default()
            };

            let role_definition = RoleDefinition {
                properties: Some(role_definition_props),
                ..Default::default()
            };

            let created_role = client
                .create_or_update_role_definition(
                    &scope,
                    role_definition_id.clone(),
                    &role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create custom role definition '{}'", role_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let role_id = created_role.id.ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "Created role definition missing ID".to_string(),
                    operation: Some("create_role_definition".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(
                role_name = %role_name,
                role_id = %role_id,
                actions_count = role_def.actions.len(),
                data_actions_count = role_def.data_actions.len(),
                "Role definition created successfully"
            );

            self.custom_role_definition_ids.push(role_id);
        }

        Ok(HandlerAction::Continue {
            state: CreatingRoleAssignments,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRoleAssignments,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        let principal_id = self.identity_principal_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Managed identity principal ID not available for role assignment"
                    .to_string(),
                operation: Some("assign_role_to_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            config_id = %config.id,
            principal_id = %principal_id,
            role_definitions_count = self.custom_role_definition_ids.len(),
            "Creating role assignments for managed identity"
        );

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        // Create role assignments for each custom role definition
        for role_definition_id in &self.custom_role_definition_ids {
            // Deterministic UUID so re-running produces the same assignment
            // rather than accumulating duplicates.
            let assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "alien:azure:stack-role-assign:{}:{}",
                    role_definition_id, principal_id
                )
                .as_bytes(),
            )
            .to_string();

            let role_assignment = RoleAssignment {
                id: None,
                name: None,
                properties: Some(RoleAssignmentProperties {
                    condition: None,
                    condition_version: None,
                    created_by: None,
                    created_on: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some(format!(
                        "Role assignment for Alien ServiceAccount {}",
                        config.id
                    )),
                    principal_id: principal_id.clone(),
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    role_definition_id: role_definition_id.clone(),
                    scope: Some(format!(
                        "/subscriptions/{}/resourceGroups/{}",
                        azure_cfg.subscription_id, resource_group_name
                    )),
                    updated_by: None,
                    updated_on: None,
                }),
                type_: None,
            };

            let full_assignment_id = client.build_role_assignment_id(&scope, assignment_id.clone());

            client
                .create_or_update_role_assignment_by_id(
                    full_assignment_id.clone(),
                    &role_assignment,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to assign role to managed identity '{}'",
                        principal_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                assignment_id = %full_assignment_id,
                role_definition_id = %role_definition_id,
                "Role assignment created successfully"
            );

            self.role_assignment_ids.push(full_assignment_id);
        }

        self.stack_permissions_applied = true;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Heartbeat check: verify managed identity still exists
        if let Some(identity_id) = &self.identity_resource_id {
            let resource_group_name =
                crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
            let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

            let managed_identity_client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;
            let identity = managed_identity_client
                .get_user_assigned_identity(&resource_group_name, &identity_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get managed identity during heartbeat check".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if identity ID matches what we expect
            if let Some(fetched_id) = &identity.id {
                if fetched_id != identity_id {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Managed identity ID changed from {} to {}",
                            identity_id, fetched_id
                        ),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            "Updating Azure managed identity permissions"
        );

        // For updates, we regenerate role definitions with new permissions
        // This is a simple approach - in production we might want to be more granular
        let new_role_definitions = self.generate_stack_role_definitions(config, ctx)?;

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        // Update existing role definitions (Azure allows updates via create_or_update)
        for (index, (role_def, role_definition_id)) in new_role_definitions
            .iter()
            .zip(&self.custom_role_definition_ids)
            .enumerate()
        {
            let role_name = format!(
                "{}-{}",
                get_azure_custom_role_name(ctx.resource_prefix, &config.id),
                index
            );

            let role_definition_props = RoleDefinitionProperties {
                role_name: Some(role_name.clone()),
                description: Some(role_def.description.clone()),
                type_: Some("CustomRole".to_string()),
                permissions: vec![Permission {
                    actions: role_def.actions.clone(),
                    not_actions: Vec::new(),
                    data_actions: role_def.data_actions.clone(),
                    not_data_actions: Vec::new(),
                }],
                assignable_scopes: role_def.assignable_scopes.clone(),
                ..Default::default()
            };

            let role_definition = RoleDefinition {
                properties: Some(role_definition_props),
                ..Default::default()
            };

            // Extract the role definition UUID from the full ID
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);

            client
                .create_or_update_role_definition(
                    &scope,
                    role_def_uuid.to_string(),
                    &role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update custom role definition '{}'", role_name),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                role_definition_id = %role_definition_id,
                "Role definition updated successfully"
            );
        }

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            role_assignments_count = self.role_assignment_ids.len(),
            "Starting deletion of role assignments"
        );

        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        // Delete all role assignments
        for assignment_id in &self.role_assignment_ids {
            match client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Role assignment deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(assignment_id = %assignment_id, "Role assignment already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete role assignment '{}'", assignment_id),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        }
        self.role_assignment_ids.clear();

        Ok(HandlerAction::Continue {
            state: DeletingRoleDefinitions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRoleDefinitions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role_definitions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            role_definitions_count = self.custom_role_definition_ids.len(),
            "Deleting custom role definitions"
        );

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        // Delete all role definitions
        for role_definition_id in &self.custom_role_definition_ids {
            // Extract the role definition UUID from the full ID
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);

            match client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_definition_id, "Role definition deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_definition_id, "Role definition already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete role definition '{}'",
                            role_definition_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        }
        self.custom_role_definition_ids.clear();

        Ok(HandlerAction::Continue {
            state: DeletingManagedIdentity,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagedIdentity,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

        if self.identity_resource_id.is_none() {
            info!(config_id = %config.id, "No managed identity was created, nothing to delete");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        match client
            .delete_user_assigned_identity(&resource_group_name, &identity_name)
            .await
        {
            Ok(_) => {
                info!(config_id = %config.id, identity_name = %identity_name, "Managed identity deleted successfully");
            }
            Err(e)
                if matches!(
                    &e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(config_id = %config.id, identity_name = %identity_name, "Managed identity already deleted");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete managed identity '{}'", identity_name),
                    resource_id: Some(config.id.clone()),
                }))
            }
        }

        self.identity_resource_id = None;
        self.identity_client_id = None;
        self.identity_principal_id = None;
        self.stack_permissions_applied = false;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(client_id), Some(resource_id)) =
            (&self.identity_client_id, &self.identity_resource_id)
        {
            Some(ResourceOutputs::new(ServiceAccountOutputs {
                identity: client_id.clone(),
                resource_id: resource_id.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        if let (Some(client_id), Some(resource_id), Some(principal_id)) = (
            &self.identity_client_id,
            &self.identity_resource_id,
            &self.identity_principal_id,
        ) {
            let binding = ServiceAccountBinding::azure_managed_identity(
                BindingValue::Value(client_id.clone()),
                BindingValue::Value(resource_id.clone()),
                BindingValue::Value(principal_id.clone()),
            );
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureServiceAccountController {
    /// Generate role definitions for all stack-level permission sets
    fn generate_stack_role_definitions(
        &self,
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<AzureRoleDefinition>> {
        if service_account.stack_permission_sets.is_empty() {
            return Ok(Vec::new());
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_subscription_id(ctx.get_azure_config()?.subscription_id.clone())
            .with_resource_group(
                crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?,
            );

        let mut all_role_definitions = Vec::new();

        for permission_set in &service_account.stack_permission_sets {
            let role_definition = generator
                .generate_role_definition(permission_set, BindingTarget::Stack, &permission_context)
                .map_err(|e| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate role definition for permission set '{}': {}",
                            permission_set.id, e
                        ),
                        operation: Some("generate_stack_role_definitions".to_string()),
                        resource_id: Some(service_account.id.clone()),
                    })
                })?;

            all_role_definitions.push(role_definition);
        }

        Ok(all_role_definitions)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(identity_name: &str) -> Self {
        Self {
            state: AzureServiceAccountState::Ready,
            identity_resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}", identity_name)),
            identity_client_id: Some("12345678-1234-1234-1234-123456789012".to_string()),
            identity_principal_id: Some("87654321-4321-4321-4321-210987654321".to_string()),
            custom_role_definition_ids: vec!["/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.Authorization/roleDefinitions/test-role".to_string()],
            role_assignment_ids: vec!["/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.Authorization/roleAssignments/test-assignment".to_string()],
            stack_permissions_applied: true,
            _internal_stay_count: None,
        }
    }
}

use std::time::Duration;
use tracing::info;
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_azure_clients::models::managedservices::{
    Authorization, RegistrationAssignment, RegistrationAssignmentProperties,
    RegistrationDefinition, RegistrationDefinitionProperties,
};
use alien_core::{
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};

/// Generates the Azure Lighthouse registration definition name.
fn get_azure_registration_definition_name(prefix: &str) -> String {
    format!("{}-lighthouse-definition", prefix)
}

/// Generates the Azure Lighthouse registration assignment name.
fn get_azure_registration_assignment_name(prefix: &str) -> String {
    format!("{}-lighthouse-assignment", prefix)
}

#[controller]
pub struct AzureRemoteStackManagementController {
    /// The ID of the created custom role definition for management permissions.
    pub(crate) management_role_definition_id: Option<String>,
    /// The ID of the created registration definition.
    pub(crate) registration_definition_id: Option<String>,
    /// The ID of the created registration assignment.
    pub(crate) registration_assignment_id: Option<String>,
    /// Whether the management role definition has been created.
    pub(crate) management_role_created: bool,
    /// Whether the registration definition has been created.
    pub(crate) definition_created: bool,
    /// Whether the registration assignment has been created.
    pub(crate) assignment_created: bool,
}

#[controller]
impl AzureRemoteStackManagementController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingManagementRoleDefinition,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_management_role_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;

        info!(
            config_id = %config.id,
            "Creating Azure custom role definition for management permissions"
        );

        // For role definitions, we need to create at resource group scope
        // Get the default resource group name from stack state
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };
        // Deterministic UUID so re-running the same deployment updates
        // the existing role definition instead of creating a duplicate.
        let role_definition_id = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("alien:azure:mgmt-role-def:{}", ctx.resource_prefix).as_bytes(),
        )
        .to_string();

        // Generate role definition from management permission sets
        let role_definition_properties = self.generate_management_role_definition(ctx)?;

        let role_definition = RoleDefinition {
            properties: Some(role_definition_properties),
            ..Default::default()
        };

        let created_role = client
            .create_or_update_role_definition(&scope, role_definition_id.clone(), &role_definition)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure management role definition '{}'",
                    role_definition_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let role_id = created_role.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created management role definition missing ID".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            role_definition_id = %role_definition_id,
            role_id = %role_id,
            "Azure management role definition created successfully"
        );

        self.management_role_definition_id = Some(role_id);
        self.management_role_created = true;

        Ok(HandlerAction::Continue {
            state: CreatingRegistrationDefinition,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRegistrationDefinition,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_registration_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_services_client(azure_config)?;

        // Get management tenant ID and identity from stack settings (required for cross-account management)
        let azure_management = ctx.get_azure_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration is required for RemoteStackManagement. Please configure management settings in your stack.".to_string(),
                operation: Some("create_lighthouse_registration".to_string()),
                resource_id: Some(config.id.clone()),
            }))?;
        let managing_tenant_id = &azure_management.managing_tenant_id;

        let subscription_id = &azure_config.subscription_id;
        let scope = client.build_subscription_scope(subscription_id);
        // Deterministic UUID so re-running the same deployment updates
        // the existing registration definition instead of creating a duplicate.
        let registration_definition_id = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("alien:azure:reg-def:{}", ctx.resource_prefix).as_bytes(),
        )
        .to_string();

        info!(
            registration_definition_id = %registration_definition_id,
            managing_tenant_id = %managing_tenant_id,
            "Creating Azure Lighthouse registration definition"
        );

        // Generate management authorizations from the stack's management permission profile
        let authorizations = self.generate_management_authorizations(ctx, managing_tenant_id)?;

        let registration_definition_name =
            get_azure_registration_definition_name(ctx.resource_prefix);

        let properties = RegistrationDefinitionProperties {
            authorizations,
            description: Some(format!(
                "Lighthouse registration for Alien stack {}",
                ctx.resource_prefix
            )),
            eligible_authorizations: vec![], // Empty for now
            managed_by_tenant_id: managing_tenant_id.clone(),
            managed_by_tenant_name: None,
            managee_tenant_id: None,
            managee_tenant_name: None,
            provisioning_state: None,
            registration_definition_name: Some(registration_definition_name),
        };

        let registration_definition = RegistrationDefinition {
            id: None,
            name: None,
            plan: None,
            properties: Some(properties),
            system_data: None,
            type_: None,
        };

        let created_definition = client
            .create_or_update_registration_definition(
                &scope,
                &registration_definition_id,
                &registration_definition,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Lighthouse registration definition '{}'",
                    registration_definition_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let definition_id = created_definition.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created registration definition missing ID".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            registration_definition_id = %registration_definition_id,
            definition_id = %definition_id,
            "Azure Lighthouse registration definition created successfully"
        );

        self.registration_definition_id = Some(definition_id);
        self.definition_created = true;

        Ok(HandlerAction::Continue {
            state: CreatingRegistrationAssignment,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRegistrationAssignment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_registration_assignment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_services_client(azure_config)?;

        let subscription_id = &azure_config.subscription_id;
        let scope = client.build_subscription_scope(subscription_id);
        // Deterministic UUID so re-running the same deployment updates
        // the existing registration assignment instead of creating a duplicate.
        let registration_assignment_id = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("alien:azure:reg-assign:{}", ctx.resource_prefix).as_bytes(),
        )
        .to_string();

        let registration_definition_id =
            self.registration_definition_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "Registration definition ID not available for assignment creation"
                        .to_string(),
                    operation: Some("creating_registration_assignment".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(
            registration_assignment_id = %registration_assignment_id,
            registration_definition_id = %registration_definition_id,
            "Creating Azure Lighthouse registration assignment"
        );

        // Create registration assignment
        let assignment_properties = RegistrationAssignmentProperties {
            provisioning_state: None,
            registration_definition: None,
            registration_definition_id: registration_definition_id.clone(),
        };

        let registration_assignment = RegistrationAssignment {
            id: None,
            name: None,
            properties: Some(assignment_properties),
            system_data: None,
            type_: None,
        };

        let created_assignment = client
            .create_or_update_registration_assignment(
                &scope,
                &registration_assignment_id,
                &registration_assignment,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure Lighthouse registration assignment '{}'",
                    registration_assignment_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let assignment_id = created_assignment.id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created registration assignment missing ID".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            registration_assignment_id = %registration_assignment_id,
            assignment_id = %assignment_id,
            "Azure Lighthouse registration assignment created successfully"
        );

        self.registration_assignment_id = Some(assignment_id);
        self.assignment_created = true;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_services_client(azure_config)?;
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        let subscription_id = &azure_config.subscription_id;
        let scope = client.build_subscription_scope(subscription_id);

        // Heartbeat check: verify registration definition still exists
        if let Some(definition_id) = &self.registration_definition_id {
            // Extract the UUID from the full resource ID
            let definition_uuid = definition_id.split('/').last().unwrap_or(definition_id);

            let definition = client
                .get_registration_definition(&scope, definition_uuid)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get registration definition during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if definition ID matches what we expect
            if let Some(fetched_id) = &definition.id {
                if fetched_id != definition_id {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Registration definition ID changed from {} to {}",
                            definition_id, fetched_id
                        ),
                    }));
                }
            }
        }

        // Heartbeat check: verify registration assignment still exists
        if let Some(assignment_id) = &self.registration_assignment_id {
            // Extract the UUID from the full resource ID
            let assignment_uuid = assignment_id.split('/').last().unwrap_or(assignment_id);

            let assignment = client
                .get_registration_assignment(&scope, assignment_uuid)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get registration assignment during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if assignment ID matches what we expect
            if let Some(fetched_id) = &assignment.id {
                if fetched_id != assignment_id {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Registration assignment ID changed from {} to {}",
                            assignment_id, fetched_id
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
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_config = ctx.get_azure_config()?;

        info!(
            config_id = %config.id,
            "Updating Azure management role definition"
        );

        // First, update the management role definition
        if let Some(role_definition_id) = &self.management_role_definition_id {
            let authorization_client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;

            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let scope = Scope::ResourceGroup {
                resource_group_name: resource_group_name.clone(),
            };

            // Re-generate role definition properties based on current stack permissions
            let role_definition_properties = self.generate_management_role_definition(ctx)?;

            let role_definition = RoleDefinition {
                properties: Some(role_definition_properties),
                ..Default::default()
            };

            // Extract the UUID from the full resource ID
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);

            authorization_client
                .create_or_update_role_definition(
                    &scope,
                    role_def_uuid.to_string(),
                    &role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update Azure management role definition '{}'",
                        role_definition_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                config_id = %config.id,
                role_definition_id = %role_definition_id,
                "Azure management role definition updated successfully"
            );
        }

        // Then update the registration definition
        let managed_services_client = ctx
            .service_provider
            .get_azure_managed_services_client(azure_config)?;
        let subscription_id = &azure_config.subscription_id;
        let scope = managed_services_client.build_subscription_scope(subscription_id);

        // Get management tenant ID from stack settings (required for cross-account management)
        let azure_management = ctx.get_azure_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration is required for RemoteStackManagement updates. Please configure management settings in your stack.".to_string(),
                operation: Some("update_lighthouse_registration".to_string()),
                resource_id: Some(config.id.clone()),
            }))?;
        let managing_tenant_id = &azure_management.managing_tenant_id;

        if let Some(definition_id) = &self.registration_definition_id {
            // Re-generate authorizations based on current stack permissions
            let authorizations =
                self.generate_management_authorizations(ctx, managing_tenant_id)?;

            let registration_definition_name =
                get_azure_registration_definition_name(ctx.resource_prefix);

            let properties = RegistrationDefinitionProperties {
                authorizations,
                description: Some(format!(
                    "Lighthouse registration for Alien stack {} (updated)",
                    ctx.resource_prefix
                )),
                eligible_authorizations: vec![], // Empty for now
                managed_by_tenant_id: managing_tenant_id.clone(),
                managed_by_tenant_name: None,
                managee_tenant_id: None,
                managee_tenant_name: None,
                provisioning_state: None,
                registration_definition_name: Some(registration_definition_name),
            };

            let registration_definition = RegistrationDefinition {
                id: None,
                name: None,
                plan: None,
                properties: Some(properties),
                system_data: None,
                type_: None,
            };

            // Extract the UUID from the full resource ID
            let definition_uuid = definition_id.split('/').last().unwrap_or(definition_id);

            managed_services_client
                .create_or_update_registration_definition(
                    &scope,
                    definition_uuid,
                    &registration_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update Azure Lighthouse registration definition '{}'",
                        definition_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                config_id = %config.id,
                definition_id = %definition_id,
                "Azure Lighthouse registration definition updated successfully"
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
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        info!(
            config_id = %config.id,
            "Starting deletion of Azure Lighthouse registration assignment"
        );

        if let Some(assignment_id) = &self.registration_assignment_id {
            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_managed_services_client(azure_config)?;

            let subscription_id = &azure_config.subscription_id;
            let scope = client.build_subscription_scope(subscription_id);

            // Extract the UUID from the full resource ID
            let assignment_uuid = assignment_id.split('/').last().unwrap_or(assignment_id);

            match client
                .delete_registration_assignment(&scope, assignment_uuid)
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Registration assignment deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(assignment_id = %assignment_id, "Registration assignment already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete registration assignment '{}'",
                            assignment_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.registration_assignment_id = None;
            self.assignment_created = false;
        } else {
            info!(config_id = %config.id, "No registration assignment was created, skipping assignment deletion");
        }

        Ok(HandlerAction::Continue {
            state: DeletingRegistrationDefinition,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRegistrationDefinition,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_registration_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        info!(
            config_id = %config.id,
            "Deleting Azure Lighthouse registration definition"
        );

        if let Some(definition_id) = &self.registration_definition_id {
            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_managed_services_client(azure_config)?;

            let subscription_id = &azure_config.subscription_id;
            let scope = client.build_subscription_scope(subscription_id);

            // Extract the UUID from the full resource ID
            let definition_uuid = definition_id.split('/').last().unwrap_or(definition_id);

            match client
                .delete_registration_definition(&scope, definition_uuid)
                .await
            {
                Ok(_) => {
                    info!(definition_id = %definition_id, "Registration definition deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(definition_id = %definition_id, "Registration definition already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete registration definition '{}'",
                            definition_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.registration_definition_id = None;
            self.definition_created = false;
        } else {
            info!(config_id = %config.id, "No registration definition was created, skipping definition deletion");
        }

        Ok(HandlerAction::Continue {
            state: DeletingManagementRoleDefinition,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagementRoleDefinition,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_management_role_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        info!(
            config_id = %config.id,
            "Deleting Azure management role definition"
        );

        if let Some(role_definition_id) = &self.management_role_definition_id {
            let azure_config = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_authorization_client(azure_config)?;

            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let scope = Scope::ResourceGroup {
                resource_group_name: resource_group_name.clone(),
            };

            // Extract the UUID from the full resource ID
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);

            match client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_definition_id, "Management role definition deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_definition_id, "Management role definition already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete management role definition '{}'",
                            role_definition_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.management_role_definition_id = None;
            self.management_role_created = false;
        } else {
            info!(config_id = %config.id, "No management role definition was created, skipping role definition deletion");
        }

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
        if let (Some(definition_id), Some(assignment_id)) = (
            &self.registration_definition_id,
            &self.registration_assignment_id,
        ) {
            Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: definition_id.clone(),
                access_configuration: assignment_id.clone(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AzureRemoteStackManagementController {
    /// Generate management authorizations from the stack's management permission profile
    fn generate_management_authorizations(
        &self,
        ctx: &ResourceControllerContext<'_>,
        managing_tenant_id: &str,
    ) -> Result<Vec<Authorization>> {
        let azure_config = ctx.get_azure_config()?;

        // Get the management principal ID from stack settings (required for cross-account management)
        let azure_management = ctx.get_azure_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration is required for RemoteStackManagement authorization generation. Please configure management settings in your stack.".to_string(),
                operation: Some("generate_management_authorizations".to_string()),
                resource_id: None,
            }))?;
        let management_principal_id = &azure_management.management_principal_id;

        // Get the custom role definition ID that we created
        let role_definition_id = self.management_role_definition_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Management role definition ID not available".to_string(),
                operation: Some("generate_management_authorizations".to_string()),
                resource_id: Some("management".to_string()),
            })
        })?;

        let authorization = Authorization {
            principal_id: management_principal_id.clone(),
            role_definition_id: role_definition_id.clone(),
            delegated_role_definition_ids: vec![], // Empty for now
            principal_id_display_name: None,
        };

        Ok(vec![authorization])
    }

    /// Generate management role definition properties from the stack's management permission profile
    fn generate_management_role_definition(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<RoleDefinitionProperties> {
        // Get the management permission profile from the stack
        // The stack processor should have processed the management permissions
        let management_permissions = ctx.desired_stack.management();
        let management_profile = management_permissions.profile()
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Management permissions not configured or set to Auto. Management permissions must be explicitly configured for remote stack management.".to_string(),
                operation: Some("generate_management_role_definition".to_string()),
                resource_id: Some("management".to_string()),
            }))?;

        // Get the global permissions for management (should be under "*")
        let global_permission_set_ids = management_profile.0.get("*").ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Management permission profile missing global permissions (*)".to_string(),
                operation: Some("generate_management_role_definition".to_string()),
                resource_id: Some("management".to_string()),
            })
        })?;

        // Only provision permission sets (ID ends with "/provision") need RG-scoped role
        // assignments via the management role definition. Non-provision sets (management,
        // heartbeat, etc.) are applied by resource controllers via resource-level IAM.
        let mut combined_actions = Vec::new();
        let mut combined_data_actions = Vec::new();
        let azure_config = ctx.get_azure_config()?;

        for permission_set_ref in global_permission_set_ids {
            let permission_set =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned());
            if let Some(permission_set) = permission_set {
                // Skip non-provision permission sets — they are handled by resource controllers
                // via resource-level IAM role assignments.
                if !permission_set.id.ends_with("/provision") {
                    continue;
                }

                // Create permission context for Azure generation
                let permission_context = PermissionContext::new()
                    .with_subscription_id(azure_config.subscription_id.clone())
                    .with_stack_prefix(ctx.resource_prefix.to_string());

                // Generate Azure role definition from the permission set
                let generator = AzureRuntimePermissionsGenerator::new();
                let azure_role_def = generator
                    .generate_role_definition(
                        &permission_set,
                        BindingTarget::Stack,
                        &permission_context,
                    )
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate Azure role definition for permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("generate_management_role_definition".to_string()),
                        resource_id: Some("management".to_string()),
                    })?;

                // Combine actions and data actions from this permission set
                combined_actions.extend(azure_role_def.actions);
                combined_data_actions.extend(azure_role_def.data_actions);
            } else {
                // If a permission set doesn't exist, log a warning but continue
                tracing::warn!(
                    permission_set_id = %permission_set_ref.id(),
                    "Management permission set not found in registry, skipping"
                );
            }
        }

        // Deduplicate actions
        combined_actions.sort();
        combined_actions.dedup();
        combined_data_actions.sort();
        combined_data_actions.dedup();

        let role_name = format!("{}-management-role", ctx.resource_prefix);
        let description = format!(
            "Custom role for managing Alien stack '{}'",
            ctx.resource_prefix
        );

        // Set assignable scopes to the subscription level for Lighthouse
        let assignable_scopes = vec![format!("/subscriptions/{}", azure_config.subscription_id)];

        let permission = Permission {
            actions: combined_actions,
            not_actions: vec![],
            data_actions: combined_data_actions,
            not_data_actions: vec![],
        };

        let role_definition_properties = RoleDefinitionProperties {
            role_name: Some(role_name),
            description: Some(description),
            type_: Some("CustomRole".to_string()),
            permissions: vec![permission],
            assignable_scopes,
            ..Default::default()
        };

        Ok(role_definition_properties)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(definition_name: &str) -> Self {
        Self {
            state: AzureRemoteStackManagementState::Ready,
            management_role_definition_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.Authorization/roleDefinitions/{}-management", definition_name)),
            registration_definition_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.ManagedServices/registrationDefinitions/{}", definition_name)),
            registration_assignment_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.ManagedServices/registrationAssignments/{}-assignment", definition_name)),
            management_role_created: true,
            definition_created: true,
            assignment_created: true,
            _internal_stay_count: None,
        }
    }
}

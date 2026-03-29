//! Azure permissions helper for applying resource-scoped permissions
//!
//! This module provides shared functionality for Azure resource controllers to apply
//! resource-scoped permissions using the Azure Authorization API.

use std::sync::Arc;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::{AuthorizationApi, Scope};
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, BindingTarget, PermissionContext,
};

use tracing::{error, info, warn};
use uuid::Uuid;

/// Helper for applying Azure resource-scoped permissions
pub struct AzurePermissionsHelper;

impl AzurePermissionsHelper {
    /// Apply resource-scoped permissions to an Azure resource
    ///
    /// This method:
    /// 1. Finds permission profiles that apply to the resource
    /// 2. Generates role definitions and assignments for each permission set
    /// 3. Creates/updates role definitions in Azure
    /// 4. Creates role assignments for the managed identities
    ///
    /// # Arguments
    /// * `ctx` - Resource controller context
    /// * `resource_id` - The resource ID (for logging and error messages)
    /// * `resource_scope` - Azure Authorization API scope for the resource
    /// * `permission_context` - Context for variable interpolation in permission sets
    pub async fn apply_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: Scope,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let azure_config = ctx.get_azure_config()?;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;

        let generator = AzureRuntimePermissionsGenerator::new();
        let type_prefix = format!("{}/", resource_type);

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions
            let mut combined_refs: Vec<alien_core::permissions::PermissionSetReference> =
                Vec::new();

            if let Some(permission_set_refs) = profile.0.get(resource_id) {
                combined_refs.extend(permission_set_refs.iter().cloned());
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                combined_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(&type_prefix))
                        .cloned(),
                );
            }

            if !combined_refs.is_empty() {
                info!(
                    resource_id = %resource_id,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing resource-scoped permissions"
                );

                // Try to process permissions for this profile, continue on errors
                if let Err(e) = Self::process_profile_permissions(
                    ctx,
                    &authorization_client,
                    resource_id,
                    profile_name,
                    &combined_refs,
                    &generator,
                    permission_context,
                    &resource_scope,
                )
                .await
                {
                    warn!(
                        resource_id = %resource_id,
                        profile = %profile_name,
                        error = %e,
                        "Failed to process permissions for profile, continuing with other profiles"
                    );
                }
            }
        }

        // Note: Azure management permissions are handled via Lighthouse (cross-tenant delegation)
        // in the AzureRemoteStackManagementController, not via resource-level IAM here.

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        ctx: &ResourceControllerContext<'_>,
        authorization_client: &Arc<dyn AuthorizationApi>,
        resource_id: &str,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &AzureRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        resource_scope: &Scope,
    ) -> Result<()> {
        // Get the managed identity ID for this profile
        let managed_identity_id = Self::get_managed_identity_id_for_profile(ctx, profile_name)?;
        let managed_identity_principal_id =
            Self::get_managed_identity_principal_id_for_profile(ctx, profile_name)?;

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            // Generate role definition for resource-scoped permissions
            let azure_role_definition = generator
                .generate_role_definition(
                    &permission_set,
                    BindingTarget::Resource,
                    permission_context,
                )
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate role definition for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            // Generate role assignment for the resource
            let azure_role_assignment = generator
                .generate_role_assignment(
                    &permission_set,
                    BindingTarget::Resource,
                    permission_context,
                )
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate role assignment for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            info!(
                profile = %profile_name,
                managed_identity = %managed_identity_id,
                permission_set = %permission_set.id,
                role_name = %azure_role_definition.name,
                assignment_scope = %azure_role_assignment.properties.scope,
                "Applying Azure role definition and assignment"
            );

            // Deterministic UUID so re-running the same deployment updates
            // the existing role definition instead of creating a duplicate.
            let role_definition_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "alien:azure:res-role-def:{}:{}:{}:{}",
                    ctx.resource_prefix, resource_id, profile_name, permission_set.id
                )
                .as_bytes(),
            )
            .to_string();
            match Self::create_role_definition(
                authorization_client,
                resource_scope,
                &role_definition_id,
                &azure_role_definition,
            )
            .await
            {
                Ok(_) => {
                    info!(
                        role_definition_id = %role_definition_id,
                        role_name = %azure_role_definition.name,
                        "Successfully created Azure role definition"
                    );
                }
                Err(e) => {
                    error!(
                        role_definition_id = %role_definition_id,
                        role_name = %azure_role_definition.name,
                        error = %e,
                        "Failed to create Azure role definition"
                    );
                    continue; // Skip assignment if role definition creation failed
                }
            }

            // Deterministic UUID matching the role definition above so
            // re-running produces the same assignment rather than a duplicate.
            let role_assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "alien:azure:res-role-assign:{}:{}:{}:{}",
                    ctx.resource_prefix, resource_id, profile_name, permission_set.id
                )
                .as_bytes(),
            )
            .to_string();
            let azure_config = ctx.get_azure_config()?;
            let full_role_definition_id = format!(
                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                resource_scope.to_scope_string(azure_config),
                role_definition_id
            );

            match Self::create_role_assignment(
                authorization_client,
                azure_config,
                resource_scope,
                &role_assignment_id,
                &managed_identity_principal_id,
                &full_role_definition_id,
            )
            .await
            {
                Ok(_) => {
                    info!(
                        role_assignment_id = %role_assignment_id,
                        principal_id = %managed_identity_principal_id,
                        role_definition_id = %full_role_definition_id,
                        "Successfully created Azure role assignment"
                    );
                }
                Err(e) => {
                    error!(
                        role_assignment_id = %role_assignment_id,
                        principal_id = %managed_identity_principal_id,
                        role_definition_id = %full_role_definition_id,
                        error = %e,
                        "Failed to create Azure role assignment"
                    );
                }
            }
        }

        Ok(())
    }

    /// Create an Azure role definition
    async fn create_role_definition(
        authorization_client: &Arc<dyn AuthorizationApi>,
        scope: &Scope,
        role_definition_id: &str,
        azure_role_definition: &alien_permissions::generators::AzureRoleDefinition,
    ) -> Result<()> {
        let role_definition = RoleDefinition {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleDefinitionProperties {
                role_name: Some(azure_role_definition.name.clone()),
                type_: Some("CustomRole".to_string()),
                description: Some(azure_role_definition.description.clone()),
                assignable_scopes: azure_role_definition.assignable_scopes.clone(),
                permissions: vec![Permission {
                    actions: azure_role_definition.actions.clone(),
                    not_actions: azure_role_definition.not_actions.clone(),
                    data_actions: azure_role_definition.data_actions.clone(),
                    not_data_actions: azure_role_definition.not_data_actions.clone(),
                }],
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        authorization_client
            .create_or_update_role_definition(
                scope,
                role_definition_id.to_string(),
                &role_definition,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure role definition".to_string(),
                resource_id: Some(azure_role_definition.name.clone()),
            })?;

        Ok(())
    }

    /// Create an Azure role assignment
    async fn create_role_assignment(
        authorization_client: &Arc<dyn AuthorizationApi>,
        azure_config: &alien_azure_clients::AzureClientConfig,
        scope: &Scope,
        role_assignment_id: &str,
        principal_id: &str,
        role_definition_id: &str,
    ) -> Result<()> {
        let full_assignment_id =
            authorization_client.build_role_assignment_id(scope, role_assignment_id.to_string());

        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: role_definition_id.to_string(),
                scope: Some(scope.to_scope_string(azure_config)),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                condition: None,
                condition_version: None,
                delegated_managed_identity_resource_id: None,
                description: Some(
                    "Alien-managed role assignment for resource-scoped permissions".to_string(),
                ),
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        authorization_client
            .create_or_update_role_assignment_by_id(full_assignment_id, &role_assignment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure role assignment".to_string(),
                resource_id: Some(role_assignment_id.to_string()),
            })?;

        Ok(())
    }

    /// Get the managed identity resource ID for a permission profile
    fn get_managed_identity_id_for_profile(
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
            &(&service_account_resource.config).into(),
        )?;

        service_account_controller
            .identity_resource_id
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "permissions_helper".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }

    /// Get the managed identity principal ID (object ID) for a permission profile
    fn get_managed_identity_principal_id_for_profile(
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
            &(&service_account_resource.config).into(),
        )?;

        service_account_controller
            .identity_principal_id
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "permissions_helper".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }
}

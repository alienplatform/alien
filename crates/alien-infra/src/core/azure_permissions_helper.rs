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

use tracing::{info, warn};
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

                Self::process_profile_permissions(
                    ctx,
                    &authorization_client,
                    resource_id,
                    profile_name,
                    &combined_refs,
                    &generator,
                    permission_context,
                    &resource_scope,
                )
                .await?;
            }
        }

        // Process management SA permissions at resource-group scope (not resource scope).
        // Management operations often involve linked resources (e.g., Container Apps
        // need `managedEnvironments/join/action` on the environment, not the app itself).
        // Using RG scope ensures the management SA can act on all related resources.
        let rg_scope = match &resource_scope {
            Scope::Resource {
                resource_group_name,
                ..
            }
            | Scope::ResourceGroup {
                resource_group_name,
            } => Scope::ResourceGroup {
                resource_group_name: resource_group_name.clone(),
            },
            // For subscription scope, keep as-is
            other => other.clone(),
        };
        Self::apply_management_permissions(
            ctx,
            resource_id,
            resource_type,
            &rg_scope,
            permission_context,
        )
        .await?;

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

        // Prepare all permission set futures, then execute in parallel.
        // Each permission set creates a role definition then a role assignment.
        // Sets are independent (different UUIDs), so they can run concurrently.
        // All operations are idempotent (deterministic UUIDs + create_or_update).
        let azure_config = ctx.get_azure_config()?;

        let futures = permission_set_refs.iter().map(|permission_set_ref| {
            let authorization_client = authorization_client.clone();
            let managed_identity_id = managed_identity_id.clone();
            let managed_identity_principal_id = managed_identity_principal_id.clone();
            let azure_config = azure_config.clone();

            async move {
                let permission_set = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Permission set '{}' not found",
                                permission_set_ref.id()
                            ),
                            resource_id: Some(profile_name.to_string()),
                        })
                    })?;

                let mut azure_role_definition = generator
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

                // Use subscription-level assignableScopes as recommended by Azure.
                // A subscription scope allows assignment at any child scope
                // (resource groups, individual resources) within that subscription,
                // and avoids propagation delays when adding new resource scopes.
                let subscription_scope =
                    format!("/{}", Scope::Subscription.to_scope_string(&azure_config));
                azure_role_definition.assignable_scopes = vec![subscription_scope];

                info!(
                    profile = %profile_name,
                    managed_identity = %managed_identity_id,
                    permission_set = %permission_set.id,
                    role_name = %azure_role_definition.name,
                    "Applying Azure role definition and assignment"
                );

                // Deterministic UUID keyed on (prefix, profile, permission_set) — NOT on
                // resource_id — so that every resource using the same permission set
                // shares a single role definition.
                let role_definition_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!(
                        "alien:azure:res-role-def:{}:{}:{}",
                        ctx.resource_prefix, profile_name, permission_set.id
                    )
                    .as_bytes(),
                )
                .to_string();

                Self::create_or_update_role_definition(
                    &authorization_client,
                    &role_definition_id,
                    &azure_role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create role definition for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

                info!(
                    role_definition_id = %role_definition_id,
                    role_name = %azure_role_definition.name,
                    "Successfully created/updated Azure role definition"
                );

                // Deterministic UUID for the role assignment.
                let role_assignment_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!(
                        "alien:azure:res-role-assign:{}:{}:{}:{}",
                        ctx.resource_prefix, resource_id, profile_name, permission_set.id
                    )
                    .as_bytes(),
                )
                .to_string();

                let full_role_definition_id = format!(
                    "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                    Scope::Subscription.to_scope_string(&azure_config),
                    role_definition_id
                );

                Self::create_role_assignment(
                    &authorization_client,
                    &azure_config,
                    resource_scope,
                    &role_assignment_id,
                    &managed_identity_principal_id,
                    &full_role_definition_id,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create role assignment for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

                info!(
                    role_assignment_id = %role_assignment_id,
                    principal_id = %managed_identity_principal_id,
                    role_definition_id = %full_role_definition_id,
                    "Successfully created Azure role assignment"
                );

                Ok::<_, AlienError<ErrorData>>(())
            }
        });

        futures::future::try_join_all(futures).await?;

        Ok(())
    }

    /// Create or update an Azure role definition at subscription scope.
    ///
    /// All callers use subscription-level `assignableScopes` (Azure's
    /// recommended approach), so multiple resources sharing the same
    /// permission set produce identical role definitions. The PUT is
    /// idempotent — no GET-then-merge needed.
    async fn create_or_update_role_definition(
        authorization_client: &Arc<dyn AuthorizationApi>,
        role_definition_id: &str,
        azure_role_definition: &alien_permissions::generators::AzureRoleDefinition,
    ) -> Result<()> {
        let scope = Scope::Subscription;

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
                &scope,
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
    pub async fn create_role_assignment(
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

    /// Apply management resource-scoped permissions (non-provision sets) for the
    /// management UAMI. This is the Azure equivalent of AWS's
    /// `apply_aws_management_resource_permissions` and GCP's
    /// `collect_gcp_management_bindings`.
    pub async fn apply_management_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: &Scope,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let management_profile = match ctx.desired_stack.management().profile() {
            Some(profile) => profile,
            None => return Ok(()),
        };

        let type_prefix = format!("{}/", resource_type);

        let mut combined_refs = Vec::new();
        if let Some(refs) = management_profile.0.get(resource_id) {
            combined_refs.extend(refs.iter().cloned());
        }
        if let Some(wildcard_refs) = management_profile.0.get("*") {
            combined_refs.extend(
                wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(&type_prefix))
                    .cloned(),
            );
        }

        if combined_refs.is_empty() {
            return Ok(());
        }

        // Skip /provision sets — handled by RSM at RG scope
        let non_provision_refs: Vec<_> = combined_refs
            .into_iter()
            .filter(|r| !r.id().ends_with("/provision"))
            .collect();

        if non_provision_refs.is_empty() {
            return Ok(());
        }

        let management_principal_id = match Self::get_management_uami_principal_id(ctx)? {
            Some(id) => id,
            None => {
                warn!(
                    resource_id = %resource_id,
                    "Management UAMI not found, skipping management permissions"
                );
                return Ok(());
            }
        };

        let azure_config = ctx.get_azure_config()?;
        let authorization_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_config)?;

        // Parallelize management permission sets — each set creates a role
        // definition then a role assignment. Sets are independent (different
        // UUIDs), so they can run concurrently. All operations are idempotent.
        let futures = non_provision_refs.iter().map(|permission_set_ref| {
            let authorization_client = authorization_client.clone();
            let management_principal_id = management_principal_id.clone();
            let azure_config = azure_config.clone();

            async move {
                let generator = AzureRuntimePermissionsGenerator::new();
                let permission_set = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!(
                                "Management permission set '{}' not found",
                                permission_set_ref.id()
                            ),
                            resource_id: Some(resource_id.to_string()),
                        })
                    })?;

                let mut azure_role_definition = generator
                    .generate_role_definition(
                        &permission_set,
                        BindingTarget::Stack,
                        permission_context,
                    )
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to generate management role definition for '{}'",
                            permission_set.id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })?;

                // Distinguish management role names from execution role names to
                // avoid Azure 409 RoleDefinitionWithSameNameExists when both
                // profiles reference the same permission set.
                azure_role_definition.name = format!("{} [mgmt]", azure_role_definition.name);

                let subscription_scope =
                    format!("/{}", Scope::Subscription.to_scope_string(&azure_config));
                azure_role_definition.assignable_scopes = vec![subscription_scope];

                // Deterministic UUID keyed on (prefix, permission_set) — NOT on
                // resource_id — so every resource shares the same management role
                // definition for a given permission set.
                let role_definition_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!(
                        "alien:azure:mgmt-res-role-def:{}:{}",
                        ctx.resource_prefix, permission_set.id
                    )
                    .as_bytes(),
                )
                .to_string();

                info!(
                    profile = "management",
                    permission_set = %permission_set.id,
                    role_definition_id = %role_definition_id,
                    "Applying management role definition and assignment"
                );

                Self::create_or_update_role_definition(
                    &authorization_client,
                    &role_definition_id,
                    &azure_role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create management role definition for '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

                info!(
                    role_definition_id = %role_definition_id,
                    permission_set = %permission_set.id,
                    "Management role definition created/updated for resource"
                );

                let full_role_definition_id = format!(
                    "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                    Scope::Subscription.to_scope_string(&azure_config),
                    role_definition_id
                );

                // UUID excludes resource_id because management assignments are at RG scope:
                // multiple resources sharing the same (role_definition, principal, scope)
                // must use the SAME assignment UUID to avoid Azure 409 RoleAssignmentExists.
                let role_assignment_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!(
                        "alien:azure:mgmt-rg-role-assign:{}:{}",
                        ctx.resource_prefix, permission_set.id
                    )
                    .as_bytes(),
                )
                .to_string();

                Self::create_role_assignment(
                    &authorization_client,
                    &azure_config,
                    resource_scope,
                    &role_assignment_id,
                    &management_principal_id,
                    &full_role_definition_id,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create management role assignment for '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

                info!(
                    principal_id = %management_principal_id,
                    permission_set = %permission_set.id,
                    "Management role assignment created for resource"
                );

                Ok::<_, AlienError<ErrorData>>(())
            }
        });

        futures::future::try_join_all(futures).await?;

        Ok(())
    }

    /// Get the management UAMI principal ID from the RSM controller
    pub fn get_management_uami_principal_id(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<String>> {
        use alien_core::RemoteStackManagement;

        for (_resource_id, resource_entry) in &ctx.desired_stack.resources {
            if resource_entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
                let controller = ctx
                    .require_dependency::<crate::remote_stack_management::AzureRemoteStackManagementController>(
                        &(&resource_entry.config).into(),
                    )?;

                return Ok(controller.uami_principal_id.clone());
            }
        }

        Ok(None)
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

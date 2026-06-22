//! Azure permissions helper for applying resource-scoped permissions
//!
//! This module provides shared functionality for Azure resource controllers to apply
//! resource-scoped permissions using the Azure Authorization API.

use crate::azure_authorization;
use crate::core::{ResourceControllerContext, Scope};
use crate::error::{ErrorData, Result};
use alien_core::AzureClientConfig;
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{
        dedupe_azure_role_bindings, AzureCustomRole, AzureRoleBinding, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use azure_mgmt_authorization::package_2022_04_01 as azure_authorization_2022_04;
use azure_mgmt_authorization::package_2022_04_01::models::{
    role_assignment_properties::PrincipalType as RoleAssignmentPropertiesPrincipalType, Permission,
    RoleAssignmentCreateParameters, RoleAssignmentProperties, RoleDefinition,
    RoleDefinitionProperties,
};

use tracing::{info, warn};
use uuid::Uuid;

/// Helper for applying Azure resource-scoped permissions
pub struct AzurePermissionsHelper;

impl AzurePermissionsHelper {
    fn role_definition_scope_for_assignment_scope(scope: &Scope) -> Scope {
        match scope {
            Scope::Resource {
                resource_group_name,
                ..
            }
            | Scope::ResourceGroup {
                resource_group_name,
            } => Scope::ResourceGroup {
                resource_group_name: resource_group_name.clone(),
            },
            Scope::Subscription => Scope::Subscription,
        }
    }

    /// Apply resource-scoped permissions to an Azure resource
    ///
    /// This method:
    /// 1. Finds permission profiles that apply to the resource
    /// 2. Resolves setup-owned role definition IDs for each permission set
    /// 3. Creates role assignments for the managed identities
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
                combined_refs.extend(
                    permission_set_refs
                        .iter()
                        .filter(|r| !is_worker_command_transport_permission(resource_type, r.id()))
                        .cloned(),
                );
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                combined_refs.extend(
                    wildcard_refs
                        .iter()
                        .filter(|r| r.id().starts_with(&type_prefix))
                        .filter(|r| !is_worker_command_transport_permission(resource_type, r.id()))
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

        Self::apply_management_permissions(
            ctx,
            resource_id,
            resource_type,
            &resource_scope,
            permission_context,
        )
        .await?;

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        ctx: &ResourceControllerContext<'_>,
        authorization_client: &azure_authorization_2022_04::Client,
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

        let azure_config = ctx.get_azure_config()?;
        let role_definition_scope =
            Self::role_definition_scope_for_assignment_scope(resource_scope);

        let mut custom_roles = Vec::new();
        let mut bindings = Vec::new();
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate grant plan for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            info!(
                profile = %profile_name,
                managed_identity = %managed_identity_id,
                permission_set = %permission_set.id,
                bindings_count = grant_plan.bindings.len(),
                "Generated Azure role assignments"
            );

            custom_roles.extend(
                grant_plan
                    .custom_roles
                    .into_iter()
                    .map(|custom_role| (permission_set.id.clone(), custom_role)),
            );
            bindings.extend(grant_plan.bindings);
        }

        Self::ensure_profile_custom_role_definitions(
            ctx,
            authorization_client,
            profile_name,
            custom_roles,
            &role_definition_scope,
            azure_config,
        )
        .await?;

        let bindings = dedupe_azure_role_bindings(bindings);
        let futures = bindings
            .into_iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let authorization_client = authorization_client.clone();
                let managed_identity_id = managed_identity_id.clone();
                let managed_identity_principal_id = managed_identity_principal_id.clone();
                let azure_config = azure_config.clone();
                let role_definition_scope = role_definition_scope.clone();

                async move {
                    info!(
                        profile = %profile_name,
                        managed_identity = %managed_identity_id,
                        "Applying Azure role assignments"
                    );

                    let role_definition_id = Self::resource_role_definition_id(
                        ctx.resource_prefix,
                        profile_name,
                        &binding,
                        &role_definition_scope,
                        &azure_config,
                    );

                    let role_assignment_id = Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!(
                            "deployment:azure:res-role-assign:{}:{}:{}:{}",
                            ctx.resource_prefix, resource_id, profile_name, binding_index
                        )
                        .as_bytes(),
                    )
                    .to_string();

                    Self::create_role_assignment(
                        &authorization_client,
                        &azure_config,
                        resource_scope,
                        &role_assignment_id,
                        &managed_identity_principal_id,
                        &role_definition_id,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create role assignment for permission set '{}'",
                            binding.permission_set_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })?;

                    info!(
                        role_assignment_id = %role_assignment_id,
                        principal_id = %managed_identity_principal_id,
                        role_definition_id = %role_definition_id,
                        "Successfully created Azure role assignment"
                    );

                    Ok::<_, AlienError<ErrorData>>(())
                }
            });

        futures::future::try_join_all(futures).await?;

        Ok(())
    }

    fn resource_role_definition_id(
        resource_prefix: &str,
        profile_name: &str,
        binding: &AzureRoleBinding,
        role_definition_scope: &Scope,
        azure_config: &AzureClientConfig,
    ) -> String {
        match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => role_definition_id.clone(),
            AzureRoleDefinitionRef::Custom { key } => {
                let role_definition_id = Self::resource_custom_role_definition_uuid(
                    resource_prefix,
                    profile_name,
                    &binding.permission_set_id,
                    key,
                );
                format!(
                    "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                    role_definition_scope.to_scope_string(azure_config),
                    role_definition_id
                )
            }
        }
    }

    async fn ensure_profile_custom_role_definitions(
        ctx: &ResourceControllerContext<'_>,
        authorization_client: &azure_authorization_2022_04::Client,
        profile_name: &str,
        custom_roles: Vec<(String, AzureCustomRole)>,
        role_definition_scope: &Scope,
        azure_config: &AzureClientConfig,
    ) -> Result<()> {
        for (permission_set_id, custom_role) in custom_roles {
            let role_definition_id = Self::resource_custom_role_definition_uuid(
                ctx.resource_prefix,
                profile_name,
                &permission_set_id,
                &custom_role.key,
            );
            let role_name = format!(
                "{}-{} [{}]",
                ctx.resource_prefix, custom_role.role_definition.name, profile_name
            );

            Self::create_or_update_custom_role_definition(
                authorization_client,
                azure_config,
                role_definition_scope,
                &role_definition_id,
                role_name,
                custom_role,
                &permission_set_id,
            )
            .await?;
        }

        Ok(())
    }

    async fn ensure_management_custom_role_definitions(
        ctx: &ResourceControllerContext<'_>,
        authorization_client: &azure_authorization_2022_04::Client,
        custom_roles: Vec<(String, AzureCustomRole)>,
        role_definition_scope: &Scope,
        azure_config: &AzureClientConfig,
    ) -> Result<()> {
        for (permission_set_id, custom_role) in custom_roles {
            let role_definition_id = Self::management_resource_custom_role_definition_uuid(
                ctx.resource_prefix,
                &permission_set_id,
                &custom_role.key,
            );
            let role_name = format!(
                "{}-{} [mgmt]",
                ctx.resource_prefix, custom_role.role_definition.name
            );

            Self::create_or_update_custom_role_definition(
                authorization_client,
                azure_config,
                role_definition_scope,
                &role_definition_id,
                role_name,
                custom_role,
                &permission_set_id,
            )
            .await?;
        }

        Ok(())
    }

    async fn create_or_update_custom_role_definition(
        authorization_client: &azure_authorization_2022_04::Client,
        azure_config: &AzureClientConfig,
        role_definition_scope: &Scope,
        role_definition_id: &str,
        role_name: String,
        custom_role: AzureCustomRole,
        permission_set_id: &str,
    ) -> Result<()> {
        let assignable_scope = role_definition_scope.to_resource_id_string(azure_config);
        let role_definition = custom_role.role_definition;
        let role_definition = RoleDefinition {
            properties: Some(RoleDefinitionProperties {
                role_name: Some(role_name.clone()),
                description: Some(role_definition.description),
                type_: Some("CustomRole".to_string()),
                permissions: vec![Permission {
                    actions: role_definition.actions,
                    not_actions: role_definition.not_actions,
                    data_actions: role_definition.data_actions,
                    not_data_actions: role_definition.not_data_actions,
                }],
                assignable_scopes: vec![assignable_scope],
                ..Default::default()
            }),
            ..Default::default()
        };

        azure_authorization::create_or_update_role_definition(
            authorization_client,
            azure_config,
            role_definition_scope,
            role_definition_id,
            &role_definition,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Azure custom role definition '{role_name}'"),
            resource_id: Some(permission_set_id.to_string()),
        })?;

        info!(
            role_name = %role_name,
            role_definition_id = %role_definition_id,
            permission_set = %permission_set_id,
            "Azure custom role definition ensured"
        );

        Ok(())
    }

    fn resource_custom_role_definition_uuid(
        resource_prefix: &str,
        profile_name: &str,
        permission_set_id: &str,
        key: &str,
    ) -> String {
        let role_segment = azure_role_key_segment(key);
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "deployment:azure:res-role-def:{}:{}:{}:{}",
                resource_prefix, profile_name, permission_set_id, role_segment
            )
            .as_bytes(),
        )
        .to_string()
    }

    fn management_resource_custom_role_definition_uuid(
        resource_prefix: &str,
        permission_set_id: &str,
        key: &str,
    ) -> String {
        let role_segment = azure_role_key_segment(key);
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "deployment:azure:mgmt-res-role-def:{}:{}:{}",
                resource_prefix, permission_set_id, role_segment
            )
            .as_bytes(),
        )
        .to_string()
    }

    /// Create an Azure role assignment
    pub async fn create_role_assignment(
        authorization_client: &azure_authorization_2022_04::Client,
        azure_config: &AzureClientConfig,
        scope: &Scope,
        role_assignment_id: &str,
        principal_id: &str,
        role_definition_id: &str,
    ) -> Result<()> {
        let full_assignment_id =
            azure_authorization::role_assignment_id(azure_config, scope, role_assignment_id);

        let role_assignment = RoleAssignmentCreateParameters::new(RoleAssignmentProperties {
            principal_id: principal_id.to_string(),
            role_definition_id: role_definition_id.to_string(),
            scope: Some(scope.to_resource_id_string(azure_config)),
            principal_type: Some(RoleAssignmentPropertiesPrincipalType::ServicePrincipal),
            condition: None,
            condition_version: None,
            delegated_managed_identity_resource_id: None,
            description: Some(
                "Runtime-managed role assignment for resource-scoped permissions".to_string(),
            ),
            created_by: None,
            created_on: None,
            updated_by: None,
            updated_on: None,
        });

        azure_authorization::create_or_update_role_assignment_by_id(
            authorization_client,
            &full_assignment_id,
            &role_assignment,
        )
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
            combined_refs.extend(
                refs.iter()
                    .filter(|r| !is_worker_command_transport_permission(resource_type, r.id()))
                    .cloned(),
            );
        }
        if let Some(wildcard_refs) = management_profile.0.get("*") {
            combined_refs.extend(
                wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(&type_prefix))
                    .filter(|r| !is_worker_command_transport_permission(resource_type, r.id()))
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
        let role_definition_scope =
            Self::role_definition_scope_for_assignment_scope(resource_scope);

        let mut custom_roles = Vec::new();
        let mut bindings = Vec::new();
        let generator = AzureRuntimePermissionsGenerator::new();
        for permission_set_ref in &non_provision_refs {
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

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate management grant plan for '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            info!(
                profile = "management",
                permission_set = %permission_set.id,
                bindings_count = grant_plan.bindings.len(),
                "Generated management role assignments"
            );

            custom_roles.extend(
                grant_plan
                    .custom_roles
                    .into_iter()
                    .map(|custom_role| (permission_set.id.clone(), custom_role)),
            );
            bindings.extend(grant_plan.bindings);
        }

        Self::ensure_management_custom_role_definitions(
            ctx,
            &authorization_client,
            custom_roles,
            &role_definition_scope,
            azure_config,
        )
        .await?;

        let bindings = dedupe_azure_role_bindings(bindings);
        let futures = bindings
            .into_iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let authorization_client = authorization_client.clone();
                let management_principal_id = management_principal_id.clone();
                let azure_config = azure_config.clone();
                let role_definition_scope = role_definition_scope.clone();

                async move {
                    let role_definition_id = match &binding.role_definition {
                        AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                            role_definition_id.clone()
                        }
                        AzureRoleDefinitionRef::Custom { key } => {
                            let role_definition_id =
                                Self::management_resource_custom_role_definition_uuid(
                                    ctx.resource_prefix,
                                    &binding.permission_set_id,
                                    key,
                                );
                            format!(
                                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                                role_definition_scope.to_scope_string(&azure_config),
                                role_definition_id
                            )
                        }
                    };

                    let role_assignment_id = Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!(
                            "deployment:azure:mgmt-res-role-assign:{}:{}:{}",
                            ctx.resource_prefix, resource_id, binding_index
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
                        &role_definition_id,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create management role assignment for '{}'",
                            binding.permission_set_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })?;

                    info!(
                        principal_id = %management_principal_id,
                        permission_set = %binding.permission_set_id,
                        role_definition_id = %role_definition_id,
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

fn is_worker_command_transport_permission(resource_type: &str, permission_set_id: &str) -> bool {
    resource_type == "worker" && permission_set_id == "worker/dispatch-command"
}

fn azure_role_key_segment(key: &str) -> String {
    key.rsplit(':')
        .next()
        .map(|segment| {
            segment
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| "custom".to_string())
}

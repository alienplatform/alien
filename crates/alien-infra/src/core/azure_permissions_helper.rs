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
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use alien_permissions::{
    generators::{
        azure_runtime_role_definition_uuid, dedupe_azure_role_bindings, AzureCustomRole,
        AzureRoleBinding, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator,
        AzureRuntimeRoleHolder,
    },
    BindingTarget, PermissionContext,
};

use tracing::{info, warn};
use uuid::Uuid;

/// Helper for applying Azure resource-scoped permissions
pub struct AzurePermissionsHelper;

#[derive(Debug)]
struct PlannedRoleAssignment {
    scope: String,
    role_assignment_id: String,
    principal_id: String,
    role_definition_id: String,
    permission_set_id: String,
    failure_message: String,
}

impl AzurePermissionsHelper {
    fn role_assignment_scope(
        permission_set_id: &str,
        generated_scope: &str,
        resource_scope: &Scope,
        azure_config: &alien_azure_clients::AzureClientConfig,
    ) -> String {
        // Azure queues share one preflight-created Service Bus namespace. The
        // legacy queue permission templates derive a namespace from
        // `${resourceName}-sb`, so the controller's concrete scope is the only
        // authoritative queue or shared-namespace resource ID.
        if permission_set_id.starts_with("queue/") {
            resource_scope.to_resource_id_string(azure_config)
        } else {
            generated_scope.to_string()
        }
    }

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
        let mut role_assignment_ids = Vec::new();
        Self::reconcile_resource_scoped_permissions(
            ctx,
            resource_id,
            resource_type,
            resource_scope,
            permission_context,
            &mut role_assignment_ids,
            true,
        )
        .await
    }

    /// Compute every deterministic assignment ID without mutating Azure.
    pub(crate) async fn plan_resource_scoped_role_assignment_ids(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: Scope,
        permission_context: &PermissionContext,
    ) -> Result<Vec<String>> {
        let mut role_assignment_ids = Vec::new();
        Self::reconcile_resource_scoped_permissions(
            ctx,
            resource_id,
            resource_type,
            resource_scope,
            permission_context,
            &mut role_assignment_ids,
            false,
        )
        .await?;
        Ok(role_assignment_ids)
    }

    /// Apply a previously checkpointed assignment plan.
    pub(crate) async fn apply_resource_scoped_permissions_from_checkpoint(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: Scope,
        permission_context: &PermissionContext,
        checkpointed_role_assignment_ids: &[String],
    ) -> Result<()> {
        let expected_role_assignment_ids = Self::plan_resource_scoped_role_assignment_ids(
            ctx,
            resource_id,
            resource_type,
            resource_scope.clone(),
            permission_context,
        )
        .await?;
        if expected_role_assignment_ids != checkpointed_role_assignment_ids {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Azure role assignment plan changed before it was applied".to_string(),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        let mut applied_role_assignment_ids = checkpointed_role_assignment_ids.to_vec();
        Self::reconcile_resource_scoped_permissions(
            ctx,
            resource_id,
            resource_type,
            resource_scope,
            permission_context,
            &mut applied_role_assignment_ids,
            true,
        )
        .await
    }

    async fn reconcile_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: Scope,
        permission_context: &PermissionContext,
        role_assignment_ids: &mut Vec<String>,
        apply: bool,
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
                    role_assignment_ids,
                    apply,
                )
                .await?;
            }
        }

        Self::apply_management_permissions_tracking_assignment_ids(
            ctx,
            resource_id,
            resource_type,
            &resource_scope,
            permission_context,
            role_assignment_ids,
            apply,
        )
        .await?;

        Self::apply_remote_bindings_permissions(
            ctx,
            &authorization_client,
            resource_id,
            &resource_scope,
            permission_context,
            role_assignment_ids,
            apply,
        )
        .await?;

        Ok(())
    }

    async fn apply_remote_bindings_permissions(
        ctx: &ResourceControllerContext<'_>,
        authorization_client: &Arc<dyn AuthorizationApi>,
        resource_id: &str,
        resource_scope: &Scope,
        permission_context: &PermissionContext,
        role_assignment_ids: &mut Vec<String>,
        apply: bool,
    ) -> Result<()> {
        let Some(entry) = ctx.desired_stack.resources.get(resource_id) else {
            return Ok(());
        };
        let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry) else {
            return Ok(());
        };
        let Some((_, identity_entry)) = ctx.desired_stack.resources.iter().find(|(_, entry)| {
            entry.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE
        }) else {
            return Err(AlienError::new(ErrorData::DependencyNotReady {
                resource_id: resource_id.to_string(),
                dependency_id: "remote-bindings".to_string(),
            }));
        };
        let controller = ctx
            .require_dependency::<crate::remote_bindings::AzureRemoteBindingsController>(
                &(&identity_entry.config).into(),
            )?;
        let principal_id = controller.principal_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: resource_id.to_string(),
                dependency_id: "remote-bindings".to_string(),
            })
        })?;
        let permission_set = alien_permissions::get_permission_set(definition.permission_set)
            .cloned()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Remote Bindings permission set '{}' is not registered",
                        definition.permission_set
                    ),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
        let generator = AzureRuntimePermissionsGenerator::new();
        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to generate Remote Bindings grant '{}'",
                    definition.permission_set
                ),
                resource_id: Some(resource_id.to_string()),
            })?;
        let profile_name = "remote-bindings";
        let azure_config = ctx.get_azure_config()?;
        let role_definition_scope =
            Self::role_definition_scope_for_assignment_scope(resource_scope);
        if apply {
            Self::ensure_profile_custom_role_definitions(
                ctx,
                authorization_client,
                profile_name,
                grant_plan
                    .custom_roles
                    .into_iter()
                    .map(|role| (permission_set.id.clone(), role))
                    .collect(),
                &role_definition_scope,
                azure_config,
            )
            .await?;
        }
        let assignments = dedupe_azure_role_bindings(grant_plan.bindings)
            .into_iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let scope = Self::role_assignment_scope(
                    &binding.permission_set_id,
                    &binding.scope,
                    resource_scope,
                    azure_config,
                );
                let role_definition_id = Self::resource_role_definition_id(
                    ctx.resource_prefix,
                    profile_name,
                    &binding,
                    &role_definition_scope,
                    azure_config,
                );
                PlannedRoleAssignment {
                    scope,
                    role_assignment_id: Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!(
                            "deployment:azure:res-role-assign:{}:{}:{}:{}",
                            ctx.resource_prefix, resource_id, profile_name, binding_index,
                        )
                        .as_bytes(),
                    )
                    .to_string(),
                    principal_id: principal_id.clone(),
                    role_definition_id,
                    failure_message: format!(
                        "Failed to create Remote Bindings assignment for '{}'",
                        binding.permission_set_id
                    ),
                    permission_set_id: binding.permission_set_id,
                }
            })
            .collect();
        if apply {
            Self::apply_planned_role_assignments(
                authorization_client,
                resource_id,
                assignments,
                role_assignment_ids,
            )
            .await
        } else {
            Self::record_planned_role_assignment_ids(&assignments, role_assignment_ids);
            Ok(())
        }
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
        role_assignment_ids: &mut Vec<String>,
        apply: bool,
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

        if apply {
            Self::ensure_profile_custom_role_definitions(
                ctx,
                authorization_client,
                profile_name,
                custom_roles,
                &role_definition_scope,
                azure_config,
            )
            .await?;
        }

        let assignments = dedupe_azure_role_bindings(bindings)
            .into_iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let assignment_scope = Self::role_assignment_scope(
                    &binding.permission_set_id,
                    &binding.scope,
                    resource_scope,
                    azure_config,
                );
                let role_definition_id = Self::resource_role_definition_id(
                    ctx.resource_prefix,
                    profile_name,
                    &binding,
                    &role_definition_scope,
                    azure_config,
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

                PlannedRoleAssignment {
                    scope: assignment_scope,
                    role_assignment_id,
                    principal_id: managed_identity_principal_id.clone(),
                    role_definition_id,
                    failure_message: format!(
                        "Failed to create role assignment for permission set '{}'",
                        binding.permission_set_id
                    ),
                    permission_set_id: binding.permission_set_id,
                }
            })
            .collect();

        info!(
            profile = %profile_name,
            managed_identity = %managed_identity_id,
            "Applying Azure role assignments"
        );
        if apply {
            Self::apply_planned_role_assignments(
                authorization_client,
                resource_id,
                assignments,
                role_assignment_ids,
            )
            .await
        } else {
            Self::record_planned_role_assignment_ids(&assignments, role_assignment_ids);
            Ok(())
        }
    }

    fn resource_role_definition_id(
        resource_prefix: &str,
        profile_name: &str,
        binding: &AzureRoleBinding,
        role_definition_scope: &Scope,
        azure_config: &alien_azure_clients::AzureClientConfig,
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
        authorization_client: &Arc<dyn AuthorizationApi>,
        profile_name: &str,
        custom_roles: Vec<(String, AzureCustomRole)>,
        role_definition_scope: &Scope,
        azure_config: &alien_azure_clients::AzureClientConfig,
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
            Self::ensure_runtime_role_definition(
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
        resource_prefix: &str,
        authorization_client: &Arc<dyn AuthorizationApi>,
        custom_roles: Vec<(String, AzureCustomRole)>,
        role_definition_scope: &Scope,
        azure_config: &alien_azure_clients::AzureClientConfig,
    ) -> Result<()> {
        for (permission_set_id, custom_role) in custom_roles {
            let role_definition_id = Self::management_resource_custom_role_definition_uuid(
                resource_prefix,
                &permission_set_id,
                &custom_role.key,
            );
            let role_name = format!(
                "{}-{} [mgmt]",
                resource_prefix, custom_role.role_definition.name
            );
            Self::ensure_runtime_role_definition(
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

    /// Makes sure a role the runtime assigns grants what the permission set asks for.
    ///
    /// Setup creates these roles, and a setup that keeps management from writing role
    /// definitions leaves the runtime nothing to write: a role that already grants the set's
    /// actions is used as it is. A role that is missing or grants something else is written,
    /// which works on a setup from before setup owned these roles; on a newer one the write is
    /// refused, and the failure asks for setup to run again, which creates the role.
    async fn ensure_runtime_role_definition(
        authorization_client: &Arc<dyn AuthorizationApi>,
        azure_config: &alien_azure_clients::AzureClientConfig,
        role_definition_scope: &Scope,
        role_definition_id: &str,
        role_name: String,
        custom_role: AzureCustomRole,
        permission_set_id: &str,
    ) -> Result<()> {
        match authorization_client
            .get_role_definition(role_definition_scope, role_definition_id.to_string())
            .await
        {
            Ok(existing) if Self::grants_role(&existing, &custom_role) => return Ok(()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read Azure custom role definition '{role_name}'"),
                    resource_id: Some(permission_set_id.to_string()),
                }));
            }
        }

        Self::create_or_update_custom_role_definition(
            authorization_client,
            azure_config,
            role_definition_scope,
            role_definition_id,
            role_name,
            custom_role,
            permission_set_id,
        )
        .await
    }

    /// Whether an existing role grants exactly what `custom_role` asks for. Azure keeps action
    /// lists in its own order.
    fn grants_role(existing: &RoleDefinition, custom_role: &AzureCustomRole) -> bool {
        let set = |items: &[String]| -> std::collections::BTreeSet<String> {
            items.iter().map(|item| item.to_ascii_lowercase()).collect()
        };
        let wanted = &custom_role.role_definition;
        existing.properties.as_ref().is_some_and(|properties| {
            let permissions = properties.permissions.first();
            let actions = permissions
                .map(|p| p.actions.as_slice())
                .unwrap_or_default();
            let not_actions = permissions
                .map(|p| p.not_actions.as_slice())
                .unwrap_or_default();
            let data_actions = permissions
                .map(|p| p.data_actions.as_slice())
                .unwrap_or_default();
            let not_data_actions = permissions
                .map(|p| p.not_data_actions.as_slice())
                .unwrap_or_default();
            properties.permissions.len() <= 1
                && set(actions) == set(&wanted.actions)
                && set(not_actions) == set(&wanted.not_actions)
                && set(data_actions) == set(&wanted.data_actions)
                && set(not_data_actions) == set(&wanted.not_data_actions)
        })
    }

    async fn create_or_update_custom_role_definition(
        authorization_client: &Arc<dyn AuthorizationApi>,
        azure_config: &alien_azure_clients::AzureClientConfig,
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

        authorization_client
            .create_or_update_role_definition(
                role_definition_scope,
                role_definition_id.to_string(),
                &role_definition,
            )
            .await
            .map_err(|error| {
                role_write_error(
                    error,
                    permission_set_id,
                    &format!("write role definition '{role_name}'"),
                    "Microsoft.Authorization/roleDefinitions/write",
                    ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create Azure custom role definition '{role_name}'"
                        ),
                        resource_id: Some(permission_set_id.to_string()),
                    },
                )
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
        azure_runtime_role_definition_uuid(
            &AzureRuntimeRoleHolder::Profile(profile_name.to_string()),
            resource_prefix,
            permission_set_id,
            key,
        )
    }

    pub(crate) fn management_resource_custom_role_definition_uuid(
        resource_prefix: &str,
        permission_set_id: &str,
        key: &str,
    ) -> String {
        azure_runtime_role_definition_uuid(
            &AzureRuntimeRoleHolder::Management,
            resource_prefix,
            permission_set_id,
            key,
        )
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
        let scope = scope.to_resource_id_string(azure_config);

        Self::create_role_assignment_at_scope(
            authorization_client,
            &scope,
            role_assignment_id,
            principal_id,
            role_definition_id,
        )
        .await?;
        Ok(())
    }

    async fn create_role_assignment_at_scope(
        authorization_client: &Arc<dyn AuthorizationApi>,
        scope: &str,
        role_assignment_id: &str,
        principal_id: &str,
        role_definition_id: &str,
    ) -> Result<String> {
        let scope = format!("/{}", scope.trim_matches('/'));
        let full_assignment_id = Self::role_assignment_resource_id(&scope, role_assignment_id);
        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: role_definition_id.to_string(),
                scope: Some(scope),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
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
            }),
        };

        let result = authorization_client
            .create_or_update_role_assignment_by_id(full_assignment_id.clone(), &role_assignment)
            .await;

        if let Err(error) = result {
            if Self::is_existing_role_assignment_conflict(&error) {
                warn!(
                    role_assignment_id = %full_assignment_id,
                    principal_id = %principal_id,
                    role_definition_id = %role_definition_id,
                    "Equivalent Azure role assignment already exists; reusing the existing grant"
                );
                return Ok(full_assignment_id);
            }

            let operation = format!(
                "assign role '{}' to principal '{principal_id}'",
                role_definition_id
                    .rsplit('/')
                    .next()
                    .unwrap_or(role_definition_id)
            );
            return Err(role_write_error(
                error,
                role_assignment_id,
                &operation,
                "Microsoft.Authorization/roleAssignments/write for that role",
                ErrorData::CloudPlatformError {
                    message: "Failed to create Azure role assignment".to_string(),
                    resource_id: Some(role_assignment_id.to_string()),
                },
            ));
        }

        Ok(full_assignment_id)
    }

    fn is_existing_role_assignment_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
        matches!(
            error.error.as_ref(),
            Some(CloudClientErrorData::RemoteResourceConflict { message, .. })
                if message
                    .to_ascii_lowercase()
                    .contains("role assignment already exists")
        )
    }

    fn role_assignment_resource_id(scope: &str, role_assignment_id: &str) -> String {
        let scope = format!("/{}", scope.trim_matches('/'));
        format!("{scope}/providers/Microsoft.Authorization/roleAssignments/{role_assignment_id}")
    }

    async fn apply_planned_role_assignments(
        authorization_client: &Arc<dyn AuthorizationApi>,
        resource_id: &str,
        assignments: Vec<PlannedRoleAssignment>,
        role_assignment_ids: &mut Vec<String>,
    ) -> Result<()> {
        Self::record_planned_role_assignment_ids(&assignments, role_assignment_ids);

        let futures = assignments.into_iter().map(|assignment| {
            let authorization_client = authorization_client.clone();
            let resource_id = resource_id.to_string();

            async move {
                Self::create_role_assignment_at_scope(
                    &authorization_client,
                    &assignment.scope,
                    &assignment.role_assignment_id,
                    &assignment.principal_id,
                    &assignment.role_definition_id,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: assignment.failure_message,
                    resource_id: Some(resource_id),
                })?;

                info!(
                    role_assignment_id = %assignment.role_assignment_id,
                    principal_id = %assignment.principal_id,
                    role_definition_id = %assignment.role_definition_id,
                    permission_set = %assignment.permission_set_id,
                    "Successfully created Azure role assignment"
                );

                Ok::<_, AlienError<ErrorData>>(())
            }
        });

        futures::future::try_join_all(futures).await?;
        Ok(())
    }

    fn record_planned_role_assignment_ids(
        assignments: &[PlannedRoleAssignment],
        role_assignment_ids: &mut Vec<String>,
    ) {
        for assignment in assignments {
            let full_assignment_id = Self::role_assignment_resource_id(
                &assignment.scope,
                &assignment.role_assignment_id,
            );
            if !role_assignment_ids.contains(&full_assignment_id) {
                role_assignment_ids.push(full_assignment_id);
            }
        }
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
        let mut role_assignment_ids = Vec::new();
        Self::apply_management_permissions_tracking_assignment_ids(
            ctx,
            resource_id,
            resource_type,
            resource_scope,
            permission_context,
            &mut role_assignment_ids,
            true,
        )
        .await?;
        Ok(())
    }

    async fn apply_management_permissions_tracking_assignment_ids(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
        resource_scope: &Scope,
        permission_context: &PermissionContext,
        role_assignment_ids: &mut Vec<String>,
        apply: bool,
    ) -> Result<()> {
        let Some(ManagementSelfAssignmentRefs {
            resource_refs,
            all_wildcard_refs,
            self_refs,
        }) = management_self_assignment_refs(ctx.desired_stack, resource_id, resource_type)
        else {
            return Ok(());
        };

        if resource_refs.is_empty() && all_wildcard_refs.is_empty() {
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

        // The earlier plan: resource refs, then every stack-wide ref, numbered together.
        let legacy_refs: Vec<_> = resource_refs
            .iter()
            .chain(&all_wildcard_refs)
            .cloned()
            .collect();
        let (_, legacy) = Self::plan_management_self_assignments(
            ctx,
            resource_id,
            resource_scope,
            permission_context,
            &legacy_refs,
            &management_principal_id,
            &role_definition_scope,
            "mgmt-res-role-assign",
        )?;
        let (custom_roles, assignments) = Self::plan_management_self_assignments(
            ctx,
            resource_id,
            resource_scope,
            permission_context,
            &self_refs,
            &management_principal_id,
            &role_definition_scope,
            "mgmt-res-role-assign-v2",
        )?;

        if !apply {
            Self::record_planned_role_assignment_ids(&assignments, role_assignment_ids);
            return Ok(());
        }

        // Remove the earlier self-assignments this plan no longer makes. One that grants the
        // same role at the same scope stays: the new plan finds it already there.
        for stale in legacy.iter().filter(|stale| {
            !assignments.iter().any(|kept| {
                kept.scope == stale.scope && kept.role_definition_id == stale.role_definition_id
            })
        }) {
            let full_id =
                Self::role_assignment_resource_id(&stale.scope, &stale.role_assignment_id);
            match authorization_client
                .delete_role_assignment_by_id(full_id.clone())
                .await
            {
                Ok(Some(_)) => info!(
                    role_assignment_id = %full_id,
                    permission_set = %stale.permission_set_id,
                    "Removed a management self-assignment setup now covers"
                ),
                Ok(None) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to remove management self-assignment for '{}'",
                            stale.permission_set_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    }));
                }
            }
        }

        // Roles first: a permission set that changed must reach a role the runtime created even
        // when every assignment below already exists. A role setup protected is left alone.
        if !assignments.is_empty() {
            Self::ensure_management_custom_role_definitions(
                ctx.resource_prefix,
                &authorization_client,
                custom_roles,
                &role_definition_scope,
                azure_config,
            )
            .await?;
        }

        // A grant already assigned stays in place, under the id this version uses or one an
        // earlier version used. Asking for it again would fail once setup keeps management from
        // assigning roles to itself, even though nothing changes. An id is only trusted when the
        // assignment it names grants this role, at this scope, to the management identity: an
        // earlier version numbered its ids by position, so the same id can hold another grant.
        let mut missing = Vec::new();
        for assignment in assignments {
            let candidates = std::iter::once(&assignment).chain(legacy.iter());
            let mut found = None;
            for candidate in candidates {
                let full_id = Self::role_assignment_resource_id(
                    &candidate.scope,
                    &candidate.role_assignment_id,
                );
                match authorization_client
                    .get_role_assignment_by_id(full_id.clone())
                    .await
                {
                    Ok(existing) if Self::grants(&existing, &assignment) => {
                        found = Some(full_id);
                        break;
                    }
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to read management role assignment for '{}'",
                                assignment.permission_set_id
                            ),
                            resource_id: Some(resource_id.to_string()),
                        }));
                    }
                }
            }
            match found {
                Some(full_id) => {
                    if !role_assignment_ids.contains(&full_id) {
                        role_assignment_ids.push(full_id);
                    }
                }
                None => missing.push(assignment),
            }
        }

        if missing.is_empty() {
            return Ok(());
        }
        Self::apply_planned_role_assignments(
            &authorization_client,
            resource_id,
            missing,
            role_assignment_ids,
        )
        .await
    }

    /// Whether `existing` gives `planned`'s principal its role at its scope. Azure reports a
    /// custom role's id at the subscription even when it was created in a resource group, so
    /// roles are compared by their definition GUID.
    fn grants(existing: &RoleAssignment, planned: &PlannedRoleAssignment) -> bool {
        let guid = |id: &str| id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
        let normalize = |scope: &str| scope.trim_matches('/').to_ascii_lowercase();
        existing.properties.as_ref().is_some_and(|properties| {
            properties
                .principal_id
                .eq_ignore_ascii_case(&planned.principal_id)
                && guid(&properties.role_definition_id) == guid(&planned.role_definition_id)
                && properties
                    .scope
                    .as_deref()
                    .is_none_or(|scope| normalize(scope) == normalize(&planned.scope))
        })
    }

    /// The role assignments that give the management identity `refs` on this resource, and the
    /// custom roles they need. `id_namespace` names the assignment ids.
    #[allow(clippy::too_many_arguments)]
    fn plan_management_self_assignments(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_scope: &Scope,
        permission_context: &PermissionContext,
        refs: &[alien_core::permissions::PermissionSetReference],
        management_principal_id: &str,
        role_definition_scope: &Scope,
        id_namespace: &str,
    ) -> Result<(Vec<(String, AzureCustomRole)>, Vec<PlannedRoleAssignment>)> {
        let azure_config = ctx.get_azure_config()?;
        let mut custom_roles = Vec::new();
        let mut bindings = Vec::new();
        let generator = AzureRuntimePermissionsGenerator::new();
        for permission_set_ref in refs {
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

            custom_roles.extend(
                grant_plan
                    .custom_roles
                    .into_iter()
                    .map(|custom_role| (permission_set.id.clone(), custom_role)),
            );
            bindings.extend(grant_plan.bindings);
        }

        let assignments = dedupe_azure_role_bindings(bindings)
            .into_iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let assignment_scope = Self::role_assignment_scope(
                    &binding.permission_set_id,
                    &binding.scope,
                    resource_scope,
                    azure_config,
                );
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
                            role_definition_scope.to_scope_string(azure_config),
                            role_definition_id
                        )
                    }
                };
                let role_assignment_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!(
                        "deployment:azure:{id_namespace}:{}:{}:{}",
                        ctx.resource_prefix, resource_id, binding_index
                    )
                    .as_bytes(),
                )
                .to_string();

                PlannedRoleAssignment {
                    scope: assignment_scope,
                    role_assignment_id,
                    principal_id: management_principal_id.to_string(),
                    role_definition_id,
                    failure_message: format!(
                        "Failed to create management role assignment for '{}'",
                        binding.permission_set_id
                    ),
                    permission_set_id: binding.permission_set_id,
                }
            })
            .collect();
        Ok((custom_roles, assignments))
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

/// The management identity's grants a resource's reconcile deals with.
#[derive(Debug, PartialEq)]
struct ManagementSelfAssignmentRefs {
    /// Grants named for this resource.
    resource_refs: Vec<alien_core::permissions::PermissionSetReference>,
    /// Every stack-wide grant for this resource type, in profile order.
    all_wildcard_refs: Vec<alien_core::permissions::PermissionSetReference>,
    /// What the runtime still assigns onto the management identity at this resource.
    self_refs: Vec<alien_core::permissions::PermissionSetReference>,
}

/// Splits the management identity's grants for a resource into what setup covers and what the
/// runtime still assigns onto management itself. `None` when the stack has no management
/// profile.
fn management_self_assignment_refs(
    stack: &alien_core::Stack,
    resource_id: &str,
    resource_type: &str,
) -> Option<ManagementSelfAssignmentRefs> {
    let management_profile = stack.management().profile()?;
    let type_prefix = format!("{}/", resource_type);
    // `/provision` sets are bound at the resource group by setup.
    let keep = |reference: &&alien_core::permissions::PermissionSetReference| {
        !is_worker_command_transport_permission(resource_type, reference.id())
            && !reference.id().ends_with("/provision")
    };
    // Grants named for this resource. Setup cannot assign them before the resource exists, so
    // the runtime does, onto the management identity itself.
    let resource_refs: Vec<_> = management_profile
        .0
        .get(resource_id)
        .into_iter()
        .flatten()
        .filter(keep)
        .cloned()
        .collect();
    // Stack-wide grants. Setup assigns the ones the management identity keeps at the resource
    // group, which covers this resource. Earlier versions also assigned them here, onto the
    // management identity: those assignments are removed, because a custom role the runtime
    // created for them is one the identity could change, and a setup that keeps management from
    // assigning roles to itself would refuse them.
    let all_wildcard_refs: Vec<_> = management_profile
        .0
        .get("*")
        .into_iter()
        .flatten()
        .filter(|reference| reference.id().starts_with(&type_prefix))
        .filter(keep)
        .cloned()
        .collect();
    // A stack-wide set the remote-bindings identity claims is left out of setup's grants, so the
    // runtime still assigns it here as earlier versions did, rather than silently taking that
    // access away.
    let kept_global: Vec<&str> = alien_permissions::management_identity_global_refs(
        stack.resources.values(),
        management_profile,
    )
    .into_iter()
    .map(|reference| reference.id())
    .collect();
    let self_refs = resource_refs
        .iter()
        .chain(
            all_wildcard_refs
                .iter()
                .filter(|reference| !kept_global.contains(&reference.id())),
        )
        .cloned()
        .collect();
    Some(ManagementSelfAssignmentRefs {
        resource_refs,
        all_wildcard_refs,
        self_refs,
    })
}

fn is_worker_command_transport_permission(resource_type: &str, permission_set_id: &str) -> bool {
    resource_type == "worker" && permission_set_id == "worker/dispatch-command"
}

/// A failed role write, with `context`. A refused one also carries
/// [`ErrorData::ManagementPermissionMissing`], which waits for setup to run again instead of
/// failing: on a setup that owns the runtime's roles, a role or an assignment the runtime is
/// refused is one the installed setup did not provide for, such as a grant a newer release added.
fn role_write_error(
    error: AlienError<CloudClientErrorData>,
    resource_id: &str,
    operation: &str,
    action: &str,
    context: ErrorData,
) -> AlienError<ErrorData> {
    if matches!(
        error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    ) {
        error
            .context(ErrorData::ManagementPermissionMissing {
                resource_id: resource_id.to_string(),
                operation: operation.to_string(),
                action: action.to_string(),
            })
            .context(context)
    } else {
        error.context(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::authorization::MockAuthorizationApi;
    use alien_azure_clients::{AzureClientConfig, AzureCredentials};
    fn azure_config() -> AzureClientConfig {
        AzureClientConfig {
            subscription_id: "sub-123".to_string(),
            tenant_id: "tenant-123".to_string(),
            region: None,
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        }
    }

    #[test]
    fn queue_assignments_use_the_controller_service_bus_scope() {
        let azure_config = azure_config();
        let shared_namespace_scope = Scope::Resource {
            resource_group_name: "rg-123".to_string(),
            resource_provider: "Microsoft.ServiceBus".to_string(),
            parent_resource_path: None,
            resource_type: "namespaces".to_string(),
            resource_name: "shared-bus".to_string(),
        };
        let concrete_queue_scope = Scope::Resource {
            resource_group_name: "rg-123".to_string(),
            resource_provider: "Microsoft.ServiceBus".to_string(),
            parent_resource_path: Some("namespaces/shared-bus".to_string()),
            resource_type: "queues".to_string(),
            resource_name: "orders".to_string(),
        };
        let stale_generated_scope = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.ServiceBus/namespaces/orders-sb/queues/orders";

        for permission_set_id in [
            "queue/data-read",
            "queue/data-write",
            "queue/heartbeat",
            "queue/management",
            "queue/provision",
        ] {
            assert_eq!(
                AzurePermissionsHelper::role_assignment_scope(
                    permission_set_id,
                    stale_generated_scope,
                    &shared_namespace_scope,
                    &azure_config,
                ),
                "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.ServiceBus/namespaces/shared-bus",
                "wildcard queue access must target the real shared namespace"
            );
            assert_eq!(
                AzurePermissionsHelper::role_assignment_scope(
                    permission_set_id,
                    stale_generated_scope,
                    &concrete_queue_scope,
                    &azure_config,
                ),
                "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.ServiceBus/namespaces/shared-bus/queues/orders",
                "resource-scoped queue access must target the real queue"
            );
        }
    }

    #[test]
    fn non_queue_assignments_preserve_the_generated_binding_scope() {
        let azure_config = azure_config();
        let concrete_container_scope = Scope::Resource {
            resource_group_name: "rg-123".to_string(),
            resource_provider: "Microsoft.Storage".to_string(),
            parent_resource_path: Some(
                "storageAccounts/account-123/blobServices/default".to_string(),
            ),
            resource_type: "containers".to_string(),
            resource_name: "content".to_string(),
        };
        let generated_resource_group_scope =
            "/subscriptions/sub-123/resourceGroups/rg-123".to_string();

        assert_eq!(
            AzurePermissionsHelper::role_assignment_scope(
                "storage/trigger-management",
                &generated_resource_group_scope,
                &concrete_container_scope,
                &azure_config,
            ),
            generated_resource_group_scope,
            "permissions that intentionally bind above the concrete resource must keep their generated scope"
        );
    }

    #[test]
    fn storage_data_assignments_use_generated_container_scope() {
        let permission_context = PermissionContext::new()
            .with_subscription_id("sub-123")
            .with_resource_group("rg-123")
            .with_storage_account_name("account-123")
            .with_resource_name("content");
        let expected_scope = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Storage/storageAccounts/account-123/blobServices/default/containers/content";

        for permission_set_id in ["storage/data-read", "storage/data-write"] {
            let permission_set = alien_permissions::get_permission_set(permission_set_id)
                .expect("storage data permission set");
            let grant_plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(permission_set, BindingTarget::Resource, &permission_context)
                .expect("storage data grant plan");
            assert_eq!(
                grant_plan.bindings.len(),
                1,
                "{permission_set_id} should emit one Azure role binding"
            );
            assert_eq!(
                grant_plan.bindings[0].scope, expected_scope,
                "{permission_set_id} must be scoped to the referenced blob container"
            );
        }
    }

    /// A role that already grants the set's actions is used as it is, whoever made it: setup
    /// creates these roles and a newer setup leaves management no right to write them. A role
    /// that is missing or grants something else is written (a setup from before setup owned
    /// them, or a set a later release changed), and on a setup that refuses the write the step
    /// waits for setup to run again instead of failing.
    #[tokio::test]
    async fn runtime_roles_are_written_only_when_they_differ() {
        let permission_set = alien_permissions::get_permission_set("vault/data-write").unwrap();
        let context = PermissionContext::new()
            .with_subscription_id("sub-123")
            .with_resource_group("rg-123")
            .with_resource_name("test-app-secrets");
        let custom_roles: Vec<_> = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .unwrap()
            .custom_roles
            .into_iter()
            .map(|role| (permission_set.id.clone(), role))
            .collect();
        assert_eq!(custom_roles.len(), 1);
        let wanted = custom_roles[0].1.role_definition.clone();
        let scope = Scope::ResourceGroup {
            resource_group_name: "rg-123".to_string(),
        };
        let existing = |data_actions: Vec<String>, assignable: &[&str]| RoleDefinition {
            properties: Some(RoleDefinitionProperties {
                assignable_scopes: assignable.iter().map(|s| s.to_string()).collect(),
                permissions: vec![Permission {
                    actions: wanted.actions.clone(),
                    not_actions: Vec::new(),
                    // Azure returns its own order and case.
                    data_actions: data_actions
                        .into_iter()
                        .rev()
                        .map(|a| a.to_uppercase())
                        .collect(),
                    not_data_actions: Vec::new(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let setup_scopes = [
            "/subscriptions/sub-123/resourceGroups/rg-123",
            "/subscriptions/sub-123",
        ];
        let runtime_scopes = ["/subscriptions/sub-123/resourceGroups/rg-123"];
        let mut stale = wanted.data_actions.clone();
        stale.push("Microsoft.KeyVault/vaults/secrets/getSecret/action".to_string());
        // (found, write attempted, write refused)
        for (found, writes, refused) in [
            (
                Some(existing(wanted.data_actions.clone(), &setup_scopes)),
                false,
                false,
            ),
            (
                Some(existing(wanted.data_actions.clone(), &runtime_scopes)),
                false,
                false,
            ),
            (Some(existing(stale.clone(), &runtime_scopes)), true, false),
            (None, true, false),
            (None, true, true),
        ] {
            let mut authorization = MockAuthorizationApi::new();
            authorization
                .expect_get_role_definition()
                .returning(move |_, id| match &found {
                    Some(role) => Ok(role.clone()),
                    None => Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "Role definition".to_string(),
                            resource_name: id,
                        },
                    )),
                });
            authorization
                .expect_create_or_update_role_definition()
                .times(usize::from(writes))
                .returning(move |_, id, definition| {
                    if refused {
                        Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                            resource_type: "Role definition".to_string(),
                            resource_name: id,
                        }))
                    } else {
                        Ok(definition.clone())
                    }
                });
            let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);
            let result = AzurePermissionsHelper::ensure_management_custom_role_definitions(
                "test",
                &authorization,
                custom_roles.clone(),
                &scope,
                &azure_config(),
            )
            .await;
            if refused {
                let error = result.expect_err("a refused write fails the step");
                assert!(crate::requires_setup_rerun(&error), "{error:?}");
            } else {
                result.unwrap();
            }
        }
    }

    /// A refused role assignment waits for setup to run again; other failures do not.
    #[tokio::test]
    async fn a_refused_role_assignment_waits_for_setup() {
        for (denied, waits) in [(true, true), (false, false)] {
            let mut authorization = MockAuthorizationApi::new();
            authorization
                .expect_create_or_update_role_assignment_by_id()
                .returning(move |id, _| {
                    Err(AlienError::new(if denied {
                        CloudClientErrorData::RemoteAccessDenied {
                            resource_type: "Role assignment".to_string(),
                            resource_name: id,
                        }
                    } else {
                        CloudClientErrorData::RemoteServiceUnavailable {
                            message: "busy".to_string(),
                        }
                    }))
                });
            let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);
            let error = AzurePermissionsHelper::create_role_assignment(
                &authorization,
                &azure_config(),
                &Scope::ResourceGroup {
                    resource_group_name: "rg-123".to_string(),
                },
                "assignment",
                "principal",
                "/subscriptions/sub-123/providers/Microsoft.Authorization/roleDefinitions/role",
            )
            .await
            .expect_err("the assignment fails");
            assert_eq!(crate::requires_setup_rerun(&error), waits, "{error:?}");
        }
    }

    /// A stack-wide set the remote-bindings identity claims is not in setup's management grants,
    /// so the runtime keeps assigning it onto management at the resource, as before. Stack-wide
    /// sets setup grants at the resource group are not assigned again.
    #[test]
    fn a_remotely_claimed_stack_wide_set_stays_assigned_at_runtime() {
        let storage = |remote_access| alien_core::ResourceEntry {
            config: alien_core::Resource::new(
                alien_core::Storage::new("files".to_string()).build(),
            ),
            // Remote bindings exist for setup-owned resources.
            lifecycle: alien_core::ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access,
            enabled_when: None,
        };
        for (remote_access, expected) in
            [(true, vec!["storage/remote-data-write"]), (false, vec![])]
        {
            let mut stack = alien_core::Stack::new("test".to_string())
                .management(alien_core::ManagementPermissions::Extend(
                    alien_core::permissions::PermissionProfile::new()
                        .global(["storage/remote-data-write", "storage/heartbeat"]),
                ))
                .build();
            stack
                .resources
                .insert("files".to_string(), storage(remote_access));
            let refs = management_self_assignment_refs(&stack, "files", "storage").unwrap();
            assert_eq!(
                refs.all_wildcard_refs
                    .iter()
                    .map(|r| r.id())
                    .collect::<Vec<_>>(),
                ["storage/remote-data-write", "storage/heartbeat"]
            );
            assert_eq!(
                refs.self_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                expected,
                "remote_access={remote_access}"
            );
        }
    }

    #[tokio::test]
    async fn storage_trigger_assignment_uses_generated_resource_group_scope() {
        let permission_set = alien_permissions::get_permission_set("storage/trigger-management")
            .expect("storage trigger management permission set");
        let permission_context = PermissionContext::new()
            .with_subscription_id("sub-123")
            .with_resource_group("rg-123")
            .with_resource_name("storage-account/blobServices/default/containers/content");
        let grant_plan = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(permission_set, BindingTarget::Resource, &permission_context)
            .expect("storage trigger management grant plan");
        let binding = grant_plan
            .bindings
            .first()
            .expect("storage trigger management role binding");
        let expected_scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        assert_eq!(binding.scope, expected_scope);

        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .withf(move |assignment_id, assignment| {
                let Some(properties) = assignment.properties.as_ref() else {
                    return false;
                };
                assignment_id
                    == &format!(
                        "{expected_scope}/providers/Microsoft.Authorization/roleAssignments/assignment-123"
                    )
                    && properties.scope.as_deref() == Some(expected_scope)
                    && properties.principal_id == "principal-123"
                    && properties.role_definition_id == "role-definition-123"
                    && matches!(
                        properties.principal_type,
                        RoleAssignmentPropertiesPrincipalType::ServicePrincipal
                    )
            })
            .times(1)
            .returning(|_, assignment| Ok(assignment.clone()));
        let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);

        let assignment_id = AzurePermissionsHelper::create_role_assignment_at_scope(
            &authorization,
            &binding.scope,
            "assignment-123",
            "principal-123",
            "role-definition-123",
        )
        .await
        .expect("role assignment at generated binding scope");
        assert_eq!(
            assignment_id,
            "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/assignment-123"
        );
    }

    #[tokio::test]
    async fn existing_equivalent_role_assignment_is_idempotent() {
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "role assignment".to_string(),
                        resource_name: "assignment-123".to_string(),
                        message: "The role assignment already exists. The ID of the existing role assignment is '/subscriptions/sub-123/existing-assignment'.".to_string(),
                    },
                ))
            });
        let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);

        let assignment_id = AzurePermissionsHelper::create_role_assignment_at_scope(
            &authorization,
            "/subscriptions/sub-123/resourceGroups/rg-123",
            "assignment-123",
            "principal-123",
            "role-definition-123",
        )
        .await
        .expect("an equivalent role assignment must satisfy the desired grant");

        assert_eq!(
            assignment_id,
            "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/assignment-123"
        );
    }

    #[tokio::test]
    async fn unrelated_role_assignment_conflict_is_propagated() {
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "role assignment".to_string(),
                        resource_name: "assignment-123".to_string(),
                        message: "Concurrent role assignment update".to_string(),
                    },
                ))
            });
        let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);

        let error = AzurePermissionsHelper::create_role_assignment_at_scope(
            &authorization,
            "/subscriptions/sub-123/resourceGroups/rg-123",
            "assignment-123",
            "principal-123",
            "role-definition-123",
        )
        .await
        .expect_err("an unrelated conflict must remain actionable");

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
    }

    #[tokio::test]
    async fn partial_assignment_failure_preserves_complete_cleanup_progress() {
        let success_scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        let failure_scope = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Storage/storageAccounts/account-123";
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .times(2)
            .returning(|assignment_id, assignment| {
                if assignment_id.ends_with("/assignment-failure") {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteServiceUnavailable {
                            message: "injected second-assignment failure".to_string(),
                        },
                    ))
                } else {
                    Ok(assignment.clone())
                }
            });
        let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);
        let assignments = vec![
            PlannedRoleAssignment {
                scope: success_scope.to_string(),
                role_assignment_id: "assignment-success".to_string(),
                principal_id: "principal-123".to_string(),
                role_definition_id: "role-definition-123".to_string(),
                permission_set_id: "storage/data-write".to_string(),
                failure_message: "Failed to create first role assignment".to_string(),
            },
            PlannedRoleAssignment {
                scope: failure_scope.to_string(),
                role_assignment_id: "assignment-failure".to_string(),
                principal_id: "principal-123".to_string(),
                role_definition_id: "role-definition-456".to_string(),
                permission_set_id: "storage/trigger-management".to_string(),
                failure_message: "Failed to create second role assignment".to_string(),
            },
        ];
        let mut role_assignment_ids = Vec::new();

        let error = AzurePermissionsHelper::apply_planned_role_assignments(
            &authorization,
            "storage-123",
            assignments,
            &mut role_assignment_ids,
        )
        .await
        .expect_err("the injected assignment failure must be propagated");

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert_eq!(
            role_assignment_ids,
            vec![
                format!(
                    "{success_scope}/providers/Microsoft.Authorization/roleAssignments/assignment-success"
                ),
                format!(
                    "{failure_scope}/providers/Microsoft.Authorization/roleAssignments/assignment-failure"
                ),
            ],
            "all deterministic assignment IDs must be available for cleanup after partial success"
        );
    }
}

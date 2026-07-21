//! Azure permissions helper for applying resource-scoped permissions
//!
//! This module provides shared functionality for Azure resource controllers to apply
//! resource-scoped permissions using the Azure Authorization API.

use std::sync::Arc;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
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
        dedupe_azure_role_bindings, AzureCustomRole, AzureRoleBinding, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
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

        // Remote-access Frozen Storage is ordered before RemoteStackManagement.
        // Its exact management role assignment is therefore owned by the
        // management controller, after the container exists.
        if !ResourcePermissionsHelper::remote_management_owns_resource_grants(
            ctx,
            resource_id,
            resource_type,
        ) {
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
        }

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
        authorization_client: &Arc<dyn AuthorizationApi>,
        custom_roles: Vec<(String, AzureCustomRole)>,
        role_definition_scope: &Scope,
        azure_config: &alien_azure_clients::AzureClientConfig,
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

            return Err(error.context(ErrorData::CloudPlatformError {
                message: "Failed to create Azure role assignment".to_string(),
                resource_id: Some(role_assignment_id.to_string()),
            }));
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
        let management_profile = match ctx.desired_stack.management().profile() {
            Some(profile) => profile,
            None => return Ok(()),
        };

        let combined_refs = Self::explicit_management_resource_permission_refs(
            management_profile,
            resource_id,
            resource_type,
        );

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

        if apply {
            Self::ensure_management_custom_role_definitions(
                ctx,
                &authorization_client,
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
                        "deployment:azure:mgmt-res-role-assign:{}:{}:{}",
                        ctx.resource_prefix, resource_id, binding_index
                    )
                    .as_bytes(),
                )
                .to_string();

                PlannedRoleAssignment {
                    scope: assignment_scope,
                    role_assignment_id,
                    principal_id: management_principal_id.clone(),
                    role_definition_id,
                    failure_message: format!(
                        "Failed to create management role assignment for '{}'",
                        binding.permission_set_id
                    ),
                    permission_set_id: binding.permission_set_id,
                }
            })
            .collect();

        if apply {
            Self::apply_planned_role_assignments(
                &authorization_client,
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

    fn explicit_management_resource_permission_refs(
        management_profile: &alien_core::permissions::PermissionProfile,
        resource_id: &str,
        resource_type: &str,
    ) -> Vec<alien_core::permissions::PermissionSetReference> {
        // RemoteStackManagement applies wildcard management permissions at
        // resource-group scope. Resource controllers only reconcile explicit
        // resource scopes; otherwise bootstrap resources can wait on management
        // while management is waiting on them.
        management_profile
            .0
            .get(resource_id)
            .into_iter()
            .flatten()
            .filter(|reference| {
                !is_worker_command_transport_permission(resource_type, reference.id())
            })
            .cloned()
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::authorization::MockAuthorizationApi;
    use alien_azure_clients::{AzureClientConfig, AzureCredentials};
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use indexmap::IndexMap;
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
    fn management_resource_permissions_ignore_wildcard_scope() {
        let mut profile = IndexMap::new();
        profile.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name(
                "storage/data-read".to_string(),
            )],
        );
        profile.insert(
            "archive".to_string(),
            vec![PermissionSetReference::from_name(
                "storage/data-write".to_string(),
            )],
        );

        let refs = AzurePermissionsHelper::explicit_management_resource_permission_refs(
            &PermissionProfile(profile),
            "archive",
            "storage",
        );

        let ids: Vec<_> = refs.iter().map(|reference| reference.id()).collect();
        assert_eq!(ids, vec!["storage/data-write"]);
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

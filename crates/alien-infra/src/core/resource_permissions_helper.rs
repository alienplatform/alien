//! Common helper for applying resource-scoped permissions across all platforms
//!
//! This module provides unified functionality for resource controllers to apply
//! resource-scoped permissions on AWS, GCP, and Azure platforms.

use std::collections::HashMap;

use crate::core::{azure_permissions_helper::AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::PermissionSet;
use alien_error::{AlienError, Context, ContextError as _};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_permissions::{generators::*, BindingTarget, PermissionContext};

use tracing::{info, warn};

/// Helper for applying resource-scoped permissions across all platforms
pub struct ResourcePermissionsHelper;

impl ResourcePermissionsHelper {
    /// Apply resource-scoped permissions for Azure resources
    ///
    /// # Arguments
    /// * `ctx` - Resource controller context
    /// * `resource_id` - The resource ID from the alien config
    /// * `resource_name` - The actual cloud resource name
    /// * `resource_scope` - Azure Authorization API scope for the resource
    /// * `resource_type` - The type of resource for logging
    pub async fn apply_azure_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_scope: Scope,
        resource_type: &str,
    ) -> Result<()> {
        info!(
            resource_id = %resource_id,
            resource_name = %resource_name,
            resource_type = %resource_type,
            "Applying Azure resource-scoped permissions"
        );

        // Build permission context for this specific resource
        let permission_context = Self::build_azure_permission_context(ctx, resource_name)?;

        AzurePermissionsHelper::apply_resource_scoped_permissions(
            ctx,
            resource_id,
            resource_scope,
            &permission_context,
        )
        .await
    }

    /// Apply resource-scoped permissions for GCP resources using IAM policy
    ///
    /// # Arguments
    /// * `ctx` - Resource controller context
    /// * `resource_id` - The resource ID from the alien config
    /// * `resource_name` - The actual cloud resource name
    /// * `resource_type` - The type of resource for logging
    /// * `iam_resource` - The GCP resource that supports IAM (e.g., bucket, function)
    /// * `apply_policy` - Closure to apply the IAM policy to the resource
    pub async fn apply_gcp_resource_scoped_permissions<T, F, Fut>(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
        iam_resource: T,
        apply_policy: F,
    ) -> Result<()>
    where
        F: FnOnce(T, IamPolicy) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut all_bindings = Vec::new();
        Self::collect_gcp_resource_scoped_bindings(
            ctx,
            resource_id,
            resource_name,
            &mut all_bindings,
        )
        .await?;

        // Apply consolidated IAM policy if we have any bindings
        if !all_bindings.is_empty() {
            let iam_policy = IamPolicy {
                version: Some(3),
                bindings: all_bindings,
                etag: None,
                kind: None,
                resource_id: None,
            };

            info!(
                resource_name = %resource_name,
                resource_type = %resource_type,
                bindings_count = iam_policy.bindings.len(),
                "Applying consolidated GCP IAM policy"
            );

            apply_policy(iam_resource, iam_policy).await?;
        }

        Ok(())
    }

    /// Idempotently create or update a single GCP custom role from a permission set.
    ///
    /// If the role already exists (conflict), it is updated to match the current
    /// permission set definition. Errors from `generate_custom_role` are propagated
    /// — if a permission set is expected to produce a GCP custom role but can't
    /// (e.g., missing GCP platform definition), this is a real error that must surface.
    pub async fn ensure_single_gcp_custom_role(
        ctx: &ResourceControllerContext<'_>,
        permission_set: &PermissionSet,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let generator = GcpRuntimePermissionsGenerator::new();

        let custom_role = generator
            .generate_custom_role(permission_set, permission_context)
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to generate GCP custom role for permission set '{}'",
                    permission_set.id,
                ),
                operation: Some("ensure_single_gcp_custom_role".to_string()),
                resource_id: Some(permission_set.id.clone()),
            })?;

        let gcp_config = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        let role_id = custom_role
            .name
            .strip_prefix(&format!("projects/{}/roles/", gcp_config.project_id))
            .unwrap_or(&custom_role.name)
            .to_string();

        info!(
            role_id = %role_id,
            permission_set = %permission_set.id,
            permissions_count = custom_role.included_permissions.len(),
            "Ensuring GCP custom role exists"
        );

        let role_request = alien_gcp_clients::iam::CreateRoleRequest::builder()
            .role(
                alien_gcp_clients::iam::Role::builder()
                    .title(custom_role.title.clone())
                    .description(custom_role.description.clone())
                    .included_permissions(custom_role.included_permissions.clone())
                    .stage(alien_gcp_clients::iam::RoleLaunchStage::Ga)
                    .build(),
            )
            .build();

        let updated_role = alien_gcp_clients::iam::Role::builder()
            .title(custom_role.title.clone())
            .description(custom_role.description.clone())
            .included_permissions(custom_role.included_permissions.clone())
            .stage(alien_gcp_clients::iam::RoleLaunchStage::Ga)
            .build();

        match iam_client.get_role(custom_role.name.clone()).await {
            Ok(_) => {
                info!(
                    role_id = %role_id,
                    "GCP custom role already exists, updating permissions"
                );
                iam_client
                    .patch_role(
                        custom_role.name.clone(),
                        updated_role,
                        Some("includedPermissions,title,description".to_string()),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to update existing custom role '{}'", role_id),
                        resource_id: Some(permission_set.id.clone()),
                    })?;
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                iam_client
                    .create_role(role_id.clone(), role_request)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create custom role '{}'", role_id),
                        resource_id: Some(permission_set.id.clone()),
                    })?;
                info!(role_id = %role_id, "GCP custom role created");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to check existence of custom role '{}'", role_id),
                    resource_id: Some(permission_set.id.clone()),
                }));
            }
        }

        Ok(())
    }

    /// Ensure GCP custom roles exist for a set of stack-level permission sets.
    ///
    /// Used by service account and remote stack management controllers to create
    /// the per-permission-set roles that `generate_bindings(BindingTarget::Stack)`
    /// references in its output bindings.
    pub async fn ensure_gcp_stack_custom_roles(
        ctx: &ResourceControllerContext<'_>,
        permission_sets: &[PermissionSet],
    ) -> Result<()> {
        if permission_sets.is_empty() {
            return Ok(());
        }

        let gcp_config = ctx.get_gcp_config()?;
        let permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string());

        for permission_set in permission_sets {
            Self::ensure_single_gcp_custom_role(ctx, permission_set, &permission_context).await?;
        }

        Ok(())
    }

    /// Ensure all GCP custom roles required for resource-scoped permissions exist.
    ///
    /// `generate_bindings(BindingTarget::Resource)` produces IAM bindings that reference
    /// per-permission-set custom roles (e.g. `projects/{project}/roles/storageDataRead`).
    /// These roles must exist in the GCP project before the bindings can be applied.
    /// This method creates them idempotently — if a role already exists it is updated
    /// to match the current permission set definition.
    pub async fn ensure_gcp_resource_custom_roles(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
    ) -> Result<()> {
        let permission_context = Self::build_gcp_permission_context(ctx, resource_name)?;
        let generator = GcpRuntimePermissionsGenerator::new();

        // Collect all unique permission sets referenced for this resource across all profiles
        let mut unique_permission_sets: HashMap<String, PermissionSet> = HashMap::new();

        for (_profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            if let Some(permission_set_refs) = profile.0.get(resource_id) {
                for permission_set_ref in permission_set_refs {
                    let permission_set = permission_set_ref
                        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::ResourceConfigInvalid {
                                message: format!(
                                    "Permission set '{}' not found",
                                    permission_set_ref.id()
                                ),
                                resource_id: Some(resource_id.to_string()),
                            })
                        })?;

                    unique_permission_sets
                        .entry(permission_set.id.clone())
                        .or_insert(permission_set);
                }
            }
        }

        if unique_permission_sets.is_empty() {
            return Ok(());
        }

        // Use ensure_single_gcp_custom_role for each, but keep the resource-specific
        // permission context (with resource_name for variable interpolation)
        let gcp_config = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        for permission_set in unique_permission_sets.values() {
            let custom_role = generator
                .generate_custom_role(permission_set, &permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate custom role for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            let role_id = custom_role
                .name
                .strip_prefix(&format!("projects/{}/roles/", gcp_config.project_id))
                .unwrap_or(&custom_role.name)
                .to_string();

            info!(
                resource_id = %resource_id,
                role_id = %role_id,
                permission_set = %permission_set.id,
                permissions_count = custom_role.included_permissions.len(),
                "Ensuring GCP custom role exists for resource-scoped permissions"
            );

            let role_request = alien_gcp_clients::iam::CreateRoleRequest::builder()
                .role(
                    alien_gcp_clients::iam::Role::builder()
                        .title(custom_role.title.clone())
                        .description(custom_role.description.clone())
                        .included_permissions(custom_role.included_permissions.clone())
                        .stage(alien_gcp_clients::iam::RoleLaunchStage::Ga)
                        .build(),
                )
                .build();

            let updated_role = alien_gcp_clients::iam::Role::builder()
                .title(custom_role.title.clone())
                .description(custom_role.description.clone())
                .included_permissions(custom_role.included_permissions.clone())
                .stage(alien_gcp_clients::iam::RoleLaunchStage::Ga)
                .build();

            match iam_client.get_role(custom_role.name.clone()).await {
                Ok(_) => {
                    info!(
                        role_id = %role_id,
                        "GCP custom role already exists, updating permissions"
                    );
                    iam_client
                        .patch_role(
                            custom_role.name.clone(),
                            updated_role,
                            Some("includedPermissions,title,description".to_string()),
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to update existing custom role '{}' for resource-scoped permissions",
                                role_id
                            ),
                            resource_id: Some(resource_id.to_string()),
                        })?;
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    iam_client
                        .create_role(role_id.clone(), role_request)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to create custom role '{}' for resource-scoped permissions",
                                role_id
                            ),
                            resource_id: Some(resource_id.to_string()),
                        })?;
                    info!(role_id = %role_id, "GCP custom role created for resource-scoped permissions");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to check existence of custom role '{}' for resource-scoped permissions",
                            role_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    }));
                }
            }
        }

        Ok(())
    }

    /// Collect GCP resource-scoped bindings without applying them (for function controllers that need service-level IAM)
    ///
    /// This method first ensures that all GCP custom roles referenced by the
    /// resource-scoped bindings exist in the project, then collects the bindings.
    ///
    /// # Arguments
    /// * `ctx` - Resource controller context
    /// * `resource_id` - The resource ID from the alien config
    /// * `resource_name` - The actual cloud resource name
    /// * `all_bindings` - Vector to collect bindings into
    pub async fn collect_gcp_resource_scoped_bindings(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        all_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        // Ensure all custom roles referenced by the bindings exist before collecting them
        Self::ensure_gcp_resource_custom_roles(ctx, resource_id, resource_name).await?;

        let permission_context = Self::build_gcp_permission_context(ctx, resource_name)?;
        let generator = GcpRuntimePermissionsGenerator::new();

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            if let Some(permission_set_refs) = profile.0.get(resource_id) {
                info!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    profile = %profile_name,
                    permission_sets = ?permission_set_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Collecting GCP resource-scoped bindings"
                );

                // Try to process permissions for this profile, continue on errors
                if let Err(e) = Self::process_gcp_profile_permissions(
                    ctx,
                    profile_name,
                    permission_set_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await
                {
                    warn!(
                        resource_id = %resource_id,
                        resource_name = %resource_name,
                        profile = %profile_name,
                        error = %e,
                        "Failed to collect GCP permissions for profile, continuing with other profiles"
                    );
                }
            }
        }

        Ok(())
    }

    /// Build Azure permission context for a resource
    fn build_azure_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        let azure_config = ctx.get_azure_config()?;

        Ok(PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string()))
    }

    /// Build GCP permission context for a resource
    fn build_gcp_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        let gcp_config = ctx.get_gcp_config()?;

        Ok(PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string()))
    }

    /// Process GCP permissions for a specific profile
    async fn process_gcp_profile_permissions(
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        all_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        // Get the service account for this profile
        let service_account_email = Self::get_gcp_service_account_email(ctx, profile_name)?;

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

            // Generate IAM bindings for resource-scoped permissions
            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            // Convert and add bindings
            let member = format!("serviceAccount:{}", service_account_email);
            let bindings_count = bindings_result.bindings.len();
            for binding in bindings_result.bindings {
                all_bindings.push(Binding {
                    role: binding.role,
                    members: vec![member.clone()],
                    condition: binding.condition.map(|cond| alien_gcp_clients::iam::Expr {
                        expression: cond.expression,
                        title: Some(cond.title),
                        description: Some(cond.description),
                        location: None,
                    }),
                });
            }

            info!(
                profile = %profile_name,
                service_account = %service_account_email,
                permission_set = %permission_set.id,
                bindings_count = bindings_count,
                "Generated GCP IAM bindings for resource-scoped permissions"
            );
        }

        Ok(())
    }

    /// Get the GCP service account email for a permission profile
    fn get_gcp_service_account_email(
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
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .clone()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "permissions_helper".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }
}

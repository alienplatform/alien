//! Common helper for applying resource-scoped permissions across all platforms
//!
//! This module provides unified functionality for resource controllers to apply
//! resource-scoped permissions on AWS, GCP, and Azure platforms.

use std::collections::HashMap;

use crate::core::{azure_permissions_helper::AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::permissions::PermissionSetReference;
use alien_core::RemoteStackManagement;
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
        permission_type: &str,
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
            permission_type,
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
        permission_type: &str,
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
            permission_type,
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
        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string());
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

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
        resource_type: &str,
    ) -> Result<()> {
        let permission_context = Self::build_gcp_permission_context(ctx, resource_name)?;
        let generator = GcpRuntimePermissionsGenerator::new();

        // Collect all unique permission sets referenced for this resource across all profiles
        let mut unique_permission_sets: HashMap<String, PermissionSet> = HashMap::new();

        let type_prefix = format!("{}/", resource_type);

        for (_profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Process resource-specific permissions
            if let Some(permission_set_refs) = profile.0.get(resource_id) {
                Self::collect_unique_permission_sets(
                    permission_set_refs,
                    resource_id,
                    &mut unique_permission_sets,
                )?;
            }

            // Process wildcard permissions that match this resource type
            if let Some(wildcard_refs) = profile.0.get("*") {
                let matching_refs: Vec<_> = wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(&type_prefix))
                    .cloned()
                    .collect();
                Self::collect_unique_permission_sets(
                    &matching_refs,
                    resource_id,
                    &mut unique_permission_sets,
                )?;
            }
        }

        // Process management permissions that match this resource type
        if let Some(management_profile) = ctx.desired_stack.management().profile() {
            // Check resource-specific management permissions
            if let Some(permission_set_refs) = management_profile.0.get(resource_id) {
                Self::collect_unique_permission_sets(
                    permission_set_refs,
                    resource_id,
                    &mut unique_permission_sets,
                )?;
            }

            // Check wildcard management permissions matching this resource type
            if let Some(wildcard_refs) = management_profile.0.get("*") {
                let matching_refs: Vec<_> = wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(&type_prefix))
                    .cloned()
                    .collect();
                Self::collect_unique_permission_sets(
                    &matching_refs,
                    resource_id,
                    &mut unique_permission_sets,
                )?;
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
        resource_type: &str,
        all_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        // Ensure all custom roles referenced by the bindings exist before collecting them
        Self::ensure_gcp_resource_custom_roles(ctx, resource_id, resource_name, resource_type)
            .await?;

        let permission_context = Self::build_gcp_permission_context(ctx, resource_name)?;
        let generator = GcpRuntimePermissionsGenerator::new();
        let type_prefix = format!("{}/", resource_type);

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions,
            // deduplicating by permission set ID to avoid duplicate IAM bindings
            let mut seen_ids = std::collections::HashSet::new();
            let mut combined_refs: Vec<PermissionSetReference> = Vec::new();

            if let Some(permission_set_refs) = profile.0.get(resource_id) {
                for r in permission_set_refs {
                    if seen_ids.insert(r.id().to_string()) {
                        combined_refs.push(r.clone());
                    }
                }
            }

            if let Some(wildcard_refs) = profile.0.get("*") {
                for r in wildcard_refs.iter().filter(|r| r.id().starts_with(&type_prefix)) {
                    if seen_ids.insert(r.id().to_string()) {
                        combined_refs.push(r.clone());
                    }
                }
            }

            if !combined_refs.is_empty() {
                info!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Collecting GCP resource-scoped bindings"
                );

                // Try to process permissions for this profile, continue on errors
                if let Err(e) = Self::process_gcp_profile_permissions(
                    ctx,
                    profile_name,
                    &combined_refs,
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

        // Process management SA permissions that match this resource type
        Self::collect_gcp_management_bindings(
            ctx,
            resource_id,
            resource_name,
            resource_type,
            &generator,
            &permission_context,
            all_bindings,
        )
        .await?;

        Ok(())
    }

    /// Build Azure permission context for a resource
    fn build_azure_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        let azure_config = ctx.get_azure_config()?;
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;

        Ok(PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group)
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string()))
    }

    /// Build GCP permission context for a resource
    fn build_gcp_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        let gcp_config = ctx.get_gcp_config()?;

        let mut permission_ctx = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string());
        if let Some(ref project_number) = gcp_config.project_number {
            permission_ctx = permission_ctx.with_project_number(project_number.clone());
        }
        Ok(permission_ctx)
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

    /// Get the GCP management service account email from the remote stack management controller
    fn get_gcp_management_service_account_email(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<String>> {
        // Find the remote-stack-management resource in the stack
        for (_resource_id, resource_entry) in &ctx.desired_stack.resources {
            if resource_entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
                let controller = ctx
                    .require_dependency::<crate::remote_stack_management::GcpRemoteStackManagementController>(
                        &(&resource_entry.config).into(),
                    )?;

                return Ok(controller.service_account_email.clone());
            }
        }

        Ok(None)
    }

    /// Collect unique permission sets from a list of references into a map
    fn collect_unique_permission_sets(
        permission_set_refs: &[PermissionSetReference],
        resource_id: &str,
        unique_permission_sets: &mut HashMap<String, PermissionSet>,
    ) -> Result<()> {
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
        Ok(())
    }

    /// Collect GCP resource-scoped bindings for the management service account
    ///
    /// Processes management permissions (from `stack.permissions.management`) that match
    /// the given resource type and applies them via resource-level IAM using the
    /// management service account.
    async fn collect_gcp_management_bindings(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        all_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        let management_profile = match ctx.desired_stack.management().profile() {
            Some(profile) => profile,
            None => return Ok(()),
        };

        let type_prefix = format!("{}/", resource_type);

        // Combine resource-specific and wildcard management permissions,
        // deduplicating by permission set ID
        let mut seen_ids = std::collections::HashSet::new();
        let mut combined_refs: Vec<PermissionSetReference> = Vec::new();

        if let Some(permission_set_refs) = management_profile.0.get(resource_id) {
            for r in permission_set_refs {
                if seen_ids.insert(r.id().to_string()) {
                    combined_refs.push(r.clone());
                }
            }
        }

        if let Some(wildcard_refs) = management_profile.0.get("*") {
            for r in wildcard_refs.iter().filter(|r| r.id().starts_with(&type_prefix)) {
                if seen_ids.insert(r.id().to_string()) {
                    combined_refs.push(r.clone());
                }
            }
        }

        if combined_refs.is_empty() {
            return Ok(());
        }

        // Get the management service account email
        let management_sa_email = match Self::get_gcp_management_service_account_email(ctx)? {
            Some(email) => email,
            None => {
                warn!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    "Management service account not found, skipping management permission bindings"
                );
                return Ok(());
            }
        };

        info!(
            resource_id = %resource_id,
            resource_name = %resource_name,
            management_sa = %management_sa_email,
            permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
            "Collecting GCP management resource-scoped bindings"
        );

        let member = format!("serviceAccount:{}", management_sa_email);

        for permission_set_ref in &combined_refs {
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

            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM bindings for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

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
                management_sa = %management_sa_email,
                permission_set = %permission_set.id,
                bindings_count = bindings_count,
                "Generated GCP IAM bindings for management resource-scoped permissions"
            );
        }

        Ok(())
    }

    // ─────────────── AWS helpers ──────────────────────────────

    /// Apply resource-scoped permissions for AWS resources.
    ///
    /// This centralised helper mirrors `apply_azure_resource_scoped_permissions` and
    /// `apply_gcp_resource_scoped_permissions` for the AWS platform.  For each
    /// permission profile it:
    ///
    /// 1. Looks up **resource-specific** entries (`profile[resource_id]`).
    /// 2. Looks up **wildcard** entries (`profile["*"]`) whose permission-set ID
    ///    starts with `<resource_type>/`, so that `"*"` permissions are correctly
    ///    expanded to every matching resource.
    /// 3. Generates an IAM policy with `BindingTarget::Resource` and attaches it as
    ///    an inline policy on the SA role.
    ///
    /// After processing all app SA profiles it also applies **management SA**
    /// resource-scoped permissions (non-provision sets from the management profile).
    pub async fn apply_aws_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
    ) -> Result<()> {
        let aws_config = ctx.get_aws_config()?;

        // Build permission context for this specific resource
        let mut permission_context = PermissionContext::new()
            .with_aws_account_id(aws_config.account_id.to_string())
            .with_aws_region(aws_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string());

        if let Some(aws_management) = ctx.get_aws_management_config()? {
            permission_context =
                permission_context.with_managing_role_arn(aws_management.managing_role_arn.clone());
            if let Some(managing_account_id) =
                PermissionContext::extract_account_id_from_role_arn(
                    &aws_management.managing_role_arn,
                )
            {
                permission_context =
                    permission_context.with_managing_account_id(managing_account_id);
            }
        }

        let generator = AwsRuntimePermissionsGenerator::new();
        let type_prefix = format!("{}/", resource_type);

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            // Combine resource-specific permissions with matching wildcard permissions
            let mut combined_refs: Vec<PermissionSetReference> = Vec::new();

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
                    resource_name = %resource_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing AWS resource-scoped permissions"
                );

                if let Err(e) = Self::process_aws_profile_permissions(
                    ctx,
                    resource_id,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                )
                .await
                {
                    warn!(
                        resource_id = %resource_id,
                        profile = %profile_name,
                        error = %e,
                        "Failed to process AWS permissions for profile, continuing with other profiles"
                    );
                }
            }
        }

        // Process management SA resource-scoped permissions
        Self::apply_aws_management_resource_permissions(
            ctx,
            resource_id,
            resource_name,
            resource_type,
            &generator,
            &permission_context,
        )
        .await?;

        Ok(())
    }

    /// Process AWS permissions for a specific profile by attaching inline policies
    /// to the profile's service account IAM role.
    async fn process_aws_profile_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        profile_name: &str,
        permission_set_refs: &[PermissionSetReference],
        generator: &AwsRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let aws_config = ctx.get_aws_config()?;

        let service_account_role_name =
            Self::get_aws_service_account_role_name(ctx, profile_name)?;

        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, permission_context)
                .map_err(|e| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to generate policy for permission set '{}': {}",
                            permission_set.id, e
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            let policy_json = serde_json::to_string_pretty(&policy).map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to serialize IAM policy document: {}", e),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

            let policy_name = format!(
                "alien-{}-{}",
                resource_id,
                permission_set.id.replace('/', "-")
            );

            let iam_client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
            iam_client
                .put_role_policy(&service_account_role_name, &policy_name, &policy_json)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply permission '{}' to role '{}'",
                        permission_set.id, service_account_role_name
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            info!(
                role_name = %service_account_role_name,
                permission_set = %permission_set.id,
                resource_id = %resource_id,
                "Applied AWS resource-scoped permission"
            );
        }

        Ok(())
    }

    /// Apply management SA resource-scoped permissions for AWS resources.
    ///
    /// Processes management permissions (from `stack.permissions.management`) that
    /// match the given resource type and applies them as inline policies on the
    /// management IAM role. Only non-provision permission sets are processed here
    /// (provision sets are handled at project level by RemoteStackManagement).
    async fn apply_aws_management_resource_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
        generator: &AwsRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let management_profile = match ctx.desired_stack.management().profile() {
            Some(profile) => profile,
            None => return Ok(()),
        };

        let type_prefix = format!("{}/", resource_type);

        // Combine resource-specific and wildcard management permissions
        let mut combined_refs: Vec<PermissionSetReference> = Vec::new();

        if let Some(permission_set_refs) = management_profile.0.get(resource_id) {
            combined_refs.extend(permission_set_refs.iter().cloned());
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

        // Get the management role name from the RemoteStackManagement controller
        let management_role_name = match Self::get_aws_management_role_name(ctx)? {
            Some(name) => name,
            None => {
                warn!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    "Management IAM role not found, skipping management permission policies"
                );
                return Ok(());
            }
        };

        info!(
            resource_id = %resource_id,
            resource_name = %resource_name,
            management_role = %management_role_name,
            permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
            "Applying AWS management resource-scoped permissions"
        );

        let aws_config = ctx.get_aws_config()?;

        for permission_set_ref in &combined_refs {
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

            // Skip provision permission sets — they are handled by RemoteStackManagement
            // at project level, not by resource controllers.
            if permission_set.id.ends_with("/provision") {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, permission_context)
                .map_err(|e| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to generate policy for management permission set '{}': {}",
                            permission_set.id, e
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            let policy_json = serde_json::to_string_pretty(&policy).map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to serialize IAM policy document: {}", e),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

            let policy_name = format!(
                "alien-mgmt-{}-{}",
                resource_id,
                permission_set.id.replace('/', "-")
            );

            let iam_client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
            iam_client
                .put_role_policy(&management_role_name, &policy_name, &policy_json)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply management permission '{}' to role '{}'",
                        permission_set.id, management_role_name
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            info!(
                management_role = %management_role_name,
                permission_set = %permission_set.id,
                resource_id = %resource_id,
                "Applied AWS management resource-scoped permission"
            );
        }

        Ok(())
    }

    /// Get the AWS IAM role name for a service account permission profile
    fn get_aws_service_account_role_name(
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
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller.role_name.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: "permissions_helper".to_string(),
                dependency_id: profile_name.to_string(),
            })
        })
    }

    /// Get the AWS management IAM role name from the RemoteStackManagement controller
    fn get_aws_management_role_name(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<String>> {
        for (_resource_id, resource_entry) in &ctx.desired_stack.resources {
            if resource_entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
                let controller = ctx
                    .require_dependency::<crate::remote_stack_management::AwsRemoteStackManagementController>(
                        &(&resource_entry.config).into(),
                    )?;

                return Ok(controller.role_name.clone());
            }
        }

        Ok(None)
    }

    /// Public method for controllers that manage their own binding collection
    /// (e.g., function/gcp.rs) to add management SA bindings for pre-computed
    /// permission set references.
    pub async fn collect_gcp_management_bindings_for(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        management_refs: &[PermissionSetReference],
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        all_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        if management_refs.is_empty() {
            return Ok(());
        }

        // Get the management service account email
        let management_sa_email = match Self::get_gcp_management_service_account_email(ctx)? {
            Some(email) => email,
            None => {
                warn!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    "Management service account not found, skipping management permission bindings"
                );
                return Ok(());
            }
        };

        info!(
            resource_id = %resource_id,
            resource_name = %resource_name,
            management_sa = %management_sa_email,
            permission_sets = ?management_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
            "Collecting GCP management resource-scoped bindings"
        );

        let member = format!("serviceAccount:{}", management_sa_email);

        for permission_set_ref in management_refs {
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

            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM bindings for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

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
                management_sa = %management_sa_email,
                permission_set = %permission_set.id,
                bindings_count = bindings_count,
                "Generated GCP IAM bindings for management resource-scoped permissions"
            );
        }

        Ok(())
    }
}

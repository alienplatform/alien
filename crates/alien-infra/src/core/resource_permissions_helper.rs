//! Common helper for applying resource-scoped permissions across all platforms
//!
//! This module provides unified functionality for resource controllers to apply
//! resource-scoped permissions on AWS, GCP, and Azure platforms.

use crate::core::{azure_permissions_helper::AzurePermissionsHelper, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::PermissionSet;
use alien_core::RemoteStackManagement;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
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
            "Reconciling consolidated GCP IAM policy"
        );

        apply_policy(iam_resource, iam_policy).await?;

        Ok(())
    }

    /// Idempotently create or update GCP custom roles from a permission set.
    pub async fn ensure_single_gcp_custom_role(
        ctx: &ResourceControllerContext<'_>,
        permission_set: &PermissionSet,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let generator = GcpRuntimePermissionsGenerator::new();
        let custom_roles = generator
            .generate_custom_roles(permission_set, permission_context)
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to generate GCP custom roles for permission set '{}'",
                    permission_set.id
                ),
                operation: Some("ensure_single_gcp_custom_role".to_string()),
                resource_id: Some(permission_set.id.clone()),
            })?;

        Self::ensure_gcp_custom_roles(ctx, &permission_set.id, custom_roles).await
    }

    /// Idempotently create or update the selected GCP custom roles.
    pub async fn ensure_gcp_custom_roles(
        ctx: &ResourceControllerContext<'_>,
        permission_set_id: &str,
        custom_roles: Vec<GcpCustomRole>,
    ) -> Result<()> {
        let gcp_config = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        for custom_role in custom_roles {
            let role_id = custom_role.role_id.clone();

            info!(
                role_id = %role_id,
                permission_set = %permission_set_id,
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
                    iam_client
                        .patch_role(
                            custom_role.name.clone(),
                            updated_role,
                            Some("includedPermissions,title,description,stage".to_string()),
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to update existing custom role '{}'", role_id),
                            resource_id: Some(permission_set_id.to_string()),
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
                            resource_id: Some(permission_set_id.to_string()),
                        })?;
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to check existence of custom role '{}'", role_id),
                        resource_id: Some(permission_set_id.to_string()),
                    }));
                }
            }
        }

        Ok(())
    }

    /// Return the fully-qualified custom-role prefix owned by this stack.
    pub fn gcp_stack_custom_role_name_prefix(permission_context: &PermissionContext) -> String {
        let project = permission_context
            .project_name
            .as_deref()
            .unwrap_or("PROJECT_NAME");
        format!(
            "projects/{project}/roles/{}",
            custom_role_prefix(permission_context)
        )
    }

    /// Return fully-qualified custom-role prefixes for the permission sets owned
    /// by one reconciliation caller.
    pub fn gcp_permission_set_custom_role_name_prefixes<'a>(
        permission_context: &PermissionContext,
        permission_set_ids: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let project = permission_context
            .project_name
            .as_deref()
            .unwrap_or("PROJECT_NAME");

        permission_set_ids
            .into_iter()
            .map(|permission_set_id| {
                format!(
                    "projects/{project}/roles/{}",
                    custom_role_permission_set_prefix(permission_set_id, permission_context)
                )
            })
            .collect()
    }

    /// Return predefined GCP roles present in a desired binding plan.
    pub fn gcp_predefined_role_names(bindings: &[Binding]) -> Vec<String> {
        let mut roles = Vec::new();
        for binding in bindings {
            if binding.role.starts_with("roles/") && !roles.contains(&binding.role) {
                roles.push(binding.role.clone());
            }
        }
        roles
    }

    /// Reconcile project-level IAM bindings for one principal and this stack's
    /// caller-owned custom roles. Existing caller-owned custom-role bindings for
    /// the principal are removed before desired bindings are merged, so revoked
    /// permissions do not remain active under old hash-based role IDs.
    pub fn reconcile_gcp_project_member_bindings(
        bindings: &mut Vec<Binding>,
        desired_bindings: Vec<Binding>,
        member: &str,
        owned_role_name_prefixes: &[String],
        owned_exact_role_names: &[String],
    ) -> bool {
        let mut changed = Self::remove_gcp_project_member_bindings(
            bindings,
            member,
            Some(owned_role_name_prefixes),
            Some(owned_exact_role_names),
        );

        for desired_binding in desired_bindings {
            let existing = bindings.iter_mut().find(|binding| {
                binding.role == desired_binding.role
                    && Self::gcp_conditions_match(&binding.condition, &desired_binding.condition)
            });

            if let Some(existing) = existing {
                for desired_member in desired_binding.members {
                    if !existing.members.contains(&desired_member) {
                        existing.members.push(desired_member);
                        changed = true;
                    }
                }
            } else {
                bindings.push(desired_binding);
                changed = true;
            }
        }

        changed
    }

    /// Remove a service-account member from project IAM bindings. When
    /// `role_name_prefixes` is provided, only bindings for caller-owned custom
    /// roles are touched, except exact `deleted:` aliases for the same service
    /// account are removed everywhere because GCP rejects policies containing
    /// them.
    pub fn remove_gcp_project_member_bindings(
        bindings: &mut Vec<Binding>,
        member: &str,
        role_name_prefixes: Option<&[String]>,
        exact_role_names: Option<&[String]>,
    ) -> bool {
        let deleted_member_prefix = Self::deleted_gcp_service_account_member_prefix(member);
        let mut changed = false;

        for binding in bindings.iter_mut() {
            let role_matches = match (role_name_prefixes, exact_role_names) {
                (None, None) => true,
                (prefixes, exact_roles) => {
                    prefixes.is_some_and(|prefixes| {
                        prefixes
                            .iter()
                            .any(|prefix| binding.role.starts_with(prefix))
                    }) || exact_roles.is_some_and(|exact_roles| exact_roles.contains(&binding.role))
                }
            };
            let before = binding.members.len();
            binding.members.retain(|binding_member| {
                let is_target_member = binding_member == member;
                let is_deleted_target = deleted_member_prefix
                    .as_ref()
                    .is_some_and(|prefix| binding_member.starts_with(prefix));

                !(is_deleted_target || (role_matches && is_target_member))
            });
            changed |= binding.members.len() != before;
        }

        let before_bindings = bindings.len();
        bindings.retain(|binding| !binding.members.is_empty());
        changed | (bindings.len() != before_bindings)
    }

    fn deleted_gcp_service_account_member_prefix(member: &str) -> Option<String> {
        member
            .strip_prefix("serviceAccount:")
            .map(|email| format!("deleted:serviceAccount:{email}?"))
    }

    fn gcp_conditions_match(
        left: &Option<alien_gcp_clients::iam::Expr>,
        right: &Option<alien_gcp_clients::iam::Expr>,
    ) -> bool {
        match (left, right) {
            (None, None) => true,
            (Some(left), Some(right)) => {
                left.expression == right.expression && left.title == right.title
            }
            _ => false,
        }
    }

    /// Collect GCP resource-scoped bindings without applying them (for function controllers that need service-level IAM)
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
        let mut iam_bindings = Vec::new();
        Self::collect_gcp_resource_scoped_iam_bindings(
            ctx,
            resource_id,
            resource_name,
            resource_type,
            &mut iam_bindings,
        )
        .await?;
        all_bindings.extend(
            iam_bindings
                .into_iter()
                .map(Self::gcp_policy_binding_from_iam_binding),
        );
        Ok(())
    }

    /// Collect GCP resource-scoped IAM bindings with generator metadata intact.
    pub async fn collect_gcp_resource_scoped_iam_bindings(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
        all_bindings: &mut Vec<GcpIamBinding>,
    ) -> Result<()> {
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
                for r in wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(&type_prefix))
                {
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

                Self::process_gcp_profile_permissions(
                    ctx,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                    all_bindings,
                )
                .await?;
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
    pub fn build_azure_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        let azure_config = ctx.get_azure_config()?;
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;

        let mut permission_ctx = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(resource_name.to_string())
            // Managing subscription/resource group: used by worker/execute and
            // compute-cluster/execute permission sets for cross-tenant management.
            // In single-subscription mode, these are the same as the current values.
            .with_managing_subscription_id(azure_config.subscription_id.clone())
            .with_managing_resource_group(resource_group);

        // Resolve storage account name from infrastructure outputs if available.
        // Many permission sets (kv/*, storage/*) reference ${storageAccountName}
        // in their Azure binding scopes.
        if let Ok(sa_outputs) = ctx
            .state
            .get_resource_outputs::<alien_core::AzureStorageAccountOutputs>(
                "default-storage-account",
            )
        {
            permission_ctx =
                permission_ctx.with_storage_account_name(sa_outputs.account_name.clone());
        }

        Ok(permission_ctx)
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
            .with_stack_name(ctx.desired_stack.id().to_string())
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
        all_bindings: &mut Vec<GcpIamBinding>,
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

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;
            let selected_bindings =
                grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            let selected_custom_roles = grant_plan.custom_roles_for_bindings(&selected_bindings);
            Self::ensure_gcp_custom_roles(ctx, &permission_set.id, selected_custom_roles).await?;

            // Convert and add bindings
            let member = format!("serviceAccount:{}", service_account_email);
            let bindings_count = selected_bindings.len();
            for mut binding in selected_bindings {
                binding.members = vec![member.clone()];
                all_bindings.push(binding);
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
    pub fn get_gcp_management_service_account_email(
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
        all_bindings: &mut Vec<GcpIamBinding>,
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
            for r in wildcard_refs
                .iter()
                .filter(|r| r.id().starts_with(&type_prefix))
            {
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

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            let selected_bindings =
                grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            let selected_custom_roles = grant_plan.custom_roles_for_bindings(&selected_bindings);
            Self::ensure_gcp_custom_roles(ctx, &permission_set.id, selected_custom_roles).await?;

            let bindings_count = selected_bindings.len();
            for mut binding in selected_bindings {
                binding.members = vec![member.clone()];
                all_bindings.push(binding);
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
            if let Some(managing_account_id) = PermissionContext::extract_account_id_from_role_arn(
                &aws_management.managing_role_arn,
            ) {
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

        let service_account_role_name = Self::get_aws_service_account_role_name(ctx, profile_name)?;

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
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate policy for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            let policy_json = serde_json::to_string_pretty(&policy)
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to serialize IAM policy document".to_string(),
                    resource_id: Some(resource_id.to_string()),
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
        _resource_type: &str,
        generator: &AwsRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let management_profile = match ctx.desired_stack.management().profile() {
            Some(profile) => profile,
            None => return Ok(()),
        };

        let combined_refs =
            Self::aws_management_resource_permission_refs(management_profile, resource_id);

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
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate policy for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;

            let policy_json = serde_json::to_string_pretty(&policy)
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to serialize IAM policy document".to_string(),
                    resource_id: Some(resource_id.to_string()),
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
                policy_name = %policy_name,
                policy_size = policy_json.len(),
                "Applied AWS management resource-scoped permission"
            );

            // Verify the policy was actually stored by reading it back
            match iam_client
                .get_role_policy(&management_role_name, &policy_name)
                .await
            {
                Ok(resp) => {
                    info!(
                        management_role = %management_role_name,
                        policy_name = %policy_name,
                        stored_policy_size = resp.get_role_policy_result.policy_document.len(),
                        "Verified management inline policy exists on role"
                    );
                }
                Err(e) => {
                    warn!(
                        management_role = %management_role_name,
                        policy_name = %policy_name,
                        error = %e,
                        "Failed to verify management inline policy — PutRolePolicy may not have persisted"
                    );
                }
            }
        }

        Ok(())
    }

    fn aws_management_resource_permission_refs(
        management_profile: &PermissionProfile,
        resource_id: &str,
    ) -> Vec<PermissionSetReference> {
        // On AWS the RemoteStackManagement role policy is the stack-level
        // grant point for wildcard management permissions. Re-applying those
        // wildcard-derived permissions as per-resource inline policies duplicates
        // authority and can exceed IAM's per-role inline policy quota. Resource
        // controllers only attach management permissions explicitly scoped to
        // this resource ID.
        management_profile
            .0
            .get(resource_id)
            .cloned()
            .unwrap_or_default()
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
    fn get_aws_management_role_name(ctx: &ResourceControllerContext<'_>) -> Result<Option<String>> {
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
    /// (e.g., worker/gcp.rs) to add management SA bindings for pre-computed
    /// permission set references.
    pub async fn collect_gcp_management_bindings_for(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        management_refs: &[PermissionSetReference],
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        expected_target: GcpBindingTargetScope,
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

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            let selected_bindings = grant_plan.bindings_for_target(expected_target);
            let selected_custom_roles = grant_plan.custom_roles_for_bindings(&selected_bindings);
            Self::ensure_gcp_custom_roles(ctx, &permission_set.id, selected_custom_roles).await?;

            let bindings_count = selected_bindings.len();
            for binding in selected_bindings {
                Self::push_gcp_binding_for_target(
                    all_bindings,
                    binding,
                    &member,
                    expected_target,
                    &permission_set.id,
                    resource_id,
                )?;
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

    fn push_gcp_binding_for_target(
        all_bindings: &mut Vec<Binding>,
        binding: GcpIamBinding,
        member: &str,
        expected_target: GcpBindingTargetScope,
        permission_set_id: &str,
        resource_id: &str,
    ) -> Result<()> {
        if binding.target != expected_target {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "GCP permission set '{}' produced a {:?} IAM binding where {:?} was required",
                    permission_set_id, binding.target, expected_target
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        all_bindings.push(Binding {
            role: binding.role,
            members: vec![member.to_string()],
            condition: binding.condition.map(|cond| alien_gcp_clients::iam::Expr {
                expression: cond.expression,
                title: Some(cond.title),
                description: Some(cond.description),
                location: None,
            }),
        });

        Ok(())
    }

    pub fn gcp_policy_binding_from_iam_binding(binding: GcpIamBinding) -> Binding {
        Binding {
            role: binding.role,
            members: binding.members,
            condition: binding.condition.map(|cond| alien_gcp_clients::iam::Expr {
                expression: cond.expression,
                title: Some(cond.title),
                description: Some(cond.description),
                location: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use indexmap::IndexMap;

    #[test]
    fn aws_management_resource_permissions_ignore_wildcard_scope() {
        let mut profile = IndexMap::new();
        profile.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name(
                "worker/heartbeat".to_string(),
            )],
        );
        profile.insert(
            "worker-a".to_string(),
            vec![PermissionSetReference::from_name(
                "worker/invoke".to_string(),
            )],
        );

        let refs = ResourcePermissionsHelper::aws_management_resource_permission_refs(
            &PermissionProfile(profile),
            "worker-a",
        );

        let ids: Vec<_> = refs.iter().map(|r| r.id().to_string()).collect();
        assert_eq!(ids, vec!["worker/invoke"]);
    }

    #[test]
    fn aws_management_resource_permissions_empty_without_resource_scope() {
        let mut profile = IndexMap::new();
        profile.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name(
                "worker/heartbeat".to_string(),
            )],
        );

        let refs = ResourcePermissionsHelper::aws_management_resource_permission_refs(
            &PermissionProfile(profile),
            "worker-a",
        );

        assert!(refs.is_empty());
    }

    #[test]
    fn gcp_project_member_reconciliation_removes_stale_owned_roles_only() {
        let mut bindings = vec![
            Binding {
                role: "projects/p/roles/role_stack_storage_data_read_old".to_string(),
                members: vec![
                    "serviceAccount:app@p.iam.gserviceaccount.com".to_string(),
                    "serviceAccount:other@p.iam.gserviceaccount.com".to_string(),
                ],
                condition: None,
            },
            Binding {
                role: "roles/viewer".to_string(),
                members: vec![
                    "serviceAccount:app@p.iam.gserviceaccount.com".to_string(),
                    "deleted:serviceAccount:app@p.iam.gserviceaccount.com?uid=123".to_string(),
                    "deleted:serviceAccount:someone-else@p.iam.gserviceaccount.com?uid=456"
                        .to_string(),
                ],
                condition: None,
            },
        ];

        let owned_role_prefixes = vec!["projects/p/roles/role_stack_storage_data_read".to_string()];
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut bindings,
            vec![Binding {
                role: "projects/p/roles/role_stack_storage_data_read".to_string(),
                members: vec!["serviceAccount:app@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            }],
            "serviceAccount:app@p.iam.gserviceaccount.com",
            &owned_role_prefixes,
            &[],
        );

        assert!(changed);
        let stale_owned = bindings
            .iter()
            .find(|binding| binding.role == "projects/p/roles/role_stack_storage_data_read_old")
            .expect("stale role binding remains for other members");
        assert_eq!(
            stale_owned.members,
            vec!["serviceAccount:other@p.iam.gserviceaccount.com"]
        );

        let viewer = bindings
            .iter()
            .find(|binding| binding.role == "roles/viewer")
            .expect("unowned binding remains");
        assert!(viewer
            .members
            .contains(&"serviceAccount:app@p.iam.gserviceaccount.com".to_string()));
        assert!(viewer.members.contains(
            &"deleted:serviceAccount:someone-else@p.iam.gserviceaccount.com?uid=456".to_string()
        ));
        assert!(!viewer
            .members
            .iter()
            .any(|member| member
                .starts_with("deleted:serviceAccount:app@p.iam.gserviceaccount.com?")));

        let desired = bindings
            .iter()
            .find(|binding| binding.role == "projects/p/roles/role_stack_storage_data_read")
            .expect("desired role binding was added");
        assert_eq!(
            desired.members,
            vec!["serviceAccount:app@p.iam.gserviceaccount.com"]
        );
    }

    #[test]
    fn gcp_project_member_reconciliation_does_not_clobber_other_management_slices() {
        let mut bindings = vec![
            Binding {
                role: "projects/p/roles/role_stack_worker_management".to_string(),
                members: vec!["serviceAccount:management@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
            Binding {
                role: "projects/p/roles/role_stack_vault_data_write_old".to_string(),
                members: vec!["serviceAccount:management@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
        ];

        let vault_prefixes = vec!["projects/p/roles/role_stack_vault_data_write".to_string()];
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut bindings,
            vec![Binding {
                role: "projects/p/roles/role_stack_vault_data_write".to_string(),
                members: vec!["serviceAccount:management@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            }],
            "serviceAccount:management@p.iam.gserviceaccount.com",
            &vault_prefixes,
            &[],
        );

        assert!(changed);
        assert!(bindings.iter().any(|binding| {
            binding.role == "projects/p/roles/role_stack_worker_management"
                && binding
                    .members
                    .contains(&"serviceAccount:management@p.iam.gserviceaccount.com".to_string())
        }));
        assert!(!bindings
            .iter()
            .any(|binding| binding.role == "projects/p/roles/role_stack_vault_data_write_old"));
        assert!(bindings.iter().any(|binding| {
            binding.role == "projects/p/roles/role_stack_vault_data_write"
                && binding
                    .members
                    .contains(&"serviceAccount:management@p.iam.gserviceaccount.com".to_string())
        }));
    }

    #[test]
    fn gcp_project_member_reconciliation_removes_owned_slice_when_desired_empty() {
        let mut bindings = vec![
            Binding {
                role: "projects/p/roles/role_stack_worker_management".to_string(),
                members: vec!["serviceAccount:management@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
            Binding {
                role: "projects/p/roles/role_stack_vault_data_write".to_string(),
                members: vec!["serviceAccount:management@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
        ];

        let worker_prefixes = vec!["projects/p/roles/role_stack_worker_management".to_string()];
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut bindings,
            Vec::new(),
            "serviceAccount:management@p.iam.gserviceaccount.com",
            &worker_prefixes,
            &[],
        );

        assert!(changed);
        assert!(!bindings
            .iter()
            .any(|binding| binding.role == "projects/p/roles/role_stack_worker_management"));
        assert!(bindings.iter().any(|binding| {
            binding.role == "projects/p/roles/role_stack_vault_data_write"
                && binding
                    .members
                    .contains(&"serviceAccount:management@p.iam.gserviceaccount.com".to_string())
        }));
    }

    #[test]
    fn gcp_project_member_reconciliation_removes_stale_owned_predefined_roles() {
        let mut bindings = vec![
            Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec!["serviceAccount:app@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
            Binding {
                role: "roles/pubsub.viewer".to_string(),
                members: vec!["serviceAccount:app@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
            Binding {
                role: "roles/viewer".to_string(),
                members: vec!["serviceAccount:app@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            },
        ];

        let owned_exact_roles = vec![
            "roles/pubsub.publisher".to_string(),
            "roles/pubsub.viewer".to_string(),
        ];
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut bindings,
            vec![Binding {
                role: "roles/pubsub.publisher".to_string(),
                members: vec!["serviceAccount:app@p.iam.gserviceaccount.com".to_string()],
                condition: None,
            }],
            "serviceAccount:app@p.iam.gserviceaccount.com",
            &[],
            &owned_exact_roles,
        );

        assert!(changed);
        assert!(bindings.iter().any(|binding| {
            binding.role == "roles/pubsub.publisher"
                && binding
                    .members
                    .contains(&"serviceAccount:app@p.iam.gserviceaccount.com".to_string())
        }));
        assert!(!bindings.iter().any(|binding| {
            binding.role == "roles/pubsub.viewer"
                && binding
                    .members
                    .contains(&"serviceAccount:app@p.iam.gserviceaccount.com".to_string())
        }));
        assert!(bindings.iter().any(|binding| {
            binding.role == "roles/viewer"
                && binding
                    .members
                    .contains(&"serviceAccount:app@p.iam.gserviceaccount.com".to_string())
        }));
    }
}

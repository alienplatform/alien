//! Common helper for applying resource-scoped permissions across all platforms
//!
//! This module provides unified functionality for resource controllers to apply
//! resource-scoped permissions on AWS, GCP, and Azure platforms.

use std::collections::HashSet;

use crate::core::{
    azure_permissions_helper::AzurePermissionsHelper, GcpCustomRoleNaming,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::permissions::{PermissionProfile, PermissionSetReference};
use alien_core::{KubernetesCluster, PermissionSet, RemoteStackManagement, ResourceLifecycle};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::iam::{
    Binding, CreateRoleRequest, IamApi, IamPolicy, Role, RoleLaunchStage,
};
use alien_permissions::{generators::*, BindingTarget, PermissionContext};

use tracing::{debug, info, warn};

fn gcp_custom_role_matches(existing: &Role, desired: &Role) -> bool {
    let mut existing_permissions = existing.included_permissions.clone();
    let mut desired_permissions = desired.included_permissions.clone();
    existing_permissions.sort();
    desired_permissions.sort();

    existing.title == desired.title
        && existing.description == desired.description
        && existing.stage == desired.stage
        && existing_permissions == desired_permissions
        && !existing.deleted.unwrap_or(false)
}

const GCP_CUSTOM_ROLE_UPDATE_MASK: &str = "includedPermissions,title,description,stage";

/// Converge one GCP custom role on `custom_role`, reusing its ID in whatever
/// state GCP holds it.
///
/// A deleted custom role keeps its ID and its slot in the project's 300-role
/// limit for 7 days, and GCP rejects `create` for that ID while it does.
/// Undeleting it lets a deployment that is deleted and recreated with the same
/// resource prefix reuse its roles instead of consuming new slots. A `create`
/// that conflicts re-reads the role, so a concurrent caller ensuring the same
/// role converges instead of failing.
async fn ensure_gcp_custom_role(
    iam: &dyn IamApi,
    permission_set_id: &str,
    custom_role: &GcpCustomRole,
) -> Result<()> {
    info!(
        role_id = %custom_role.role_id,
        permission_set = %permission_set_id,
        permissions_count = custom_role.included_permissions.len(),
        "Ensuring GCP custom role exists"
    );

    let desired = Role::builder()
        .title(custom_role.title.clone())
        .description(custom_role.description.clone())
        .included_permissions(custom_role.included_permissions.clone())
        .stage(RoleLaunchStage::Ga)
        .build();

    if let Some(existing) = get_gcp_custom_role(iam, permission_set_id, custom_role).await? {
        return converge_gcp_custom_role(iam, permission_set_id, custom_role, existing, desired)
            .await;
    }

    let create_conflict = match iam
        .create_role(
            custom_role.role_id.clone(),
            CreateRoleRequest::builder().role(desired.clone()).build(),
        )
        .await
    {
        Ok(_) => return Ok(()),
        Err(e)
            if matches!(
                e.error,
                Some(CloudClientErrorData::RemoteResourceConflict { .. })
            ) =>
        {
            e
        }
        Err(e) => {
            return Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create custom role '{}'", custom_role.role_id),
                resource_id: Some(permission_set_id.to_string()),
            }))
        }
    };

    match get_gcp_custom_role(iam, permission_set_id, custom_role).await? {
        Some(existing) => {
            converge_gcp_custom_role(iam, permission_set_id, custom_role, existing, desired).await
        }
        // IAM reads are eventually consistent. An absent re-read proves no
        // permanent deletion unless create explicitly reported that state.
        None if matches!(
            &create_conflict.error,
            Some(CloudClientErrorData::RemoteResourceConflict { message, .. })
                if message.contains("which has been marked for deletion")
        ) => Err(create_conflict.context(ErrorData::GcpCustomRoleIdUnavailable {
            role_id: custom_role.role_id.clone(),
            message: "a role with this ID was deleted more than 7 days ago, so it can no longer be undeleted, and GCP rejects a new role with the same ID until the old one is purged, up to 44 days after deletion. Deploy with a different resource prefix, or retry after the old role is purged".to_string(),
        })),
        None => Err(create_conflict.context(ErrorData::CloudPlatformError {
            message: format!("Failed to create custom role '{}'", custom_role.role_id),
            resource_id: Some(permission_set_id.to_string()),
        })),
    }
}

async fn get_gcp_custom_role(
    iam: &dyn IamApi,
    permission_set_id: &str,
    custom_role: &GcpCustomRole,
) -> Result<Option<Role>> {
    match iam.get_role(custom_role.name.clone()).await {
        Ok(role) => Ok(Some(role)),
        Err(e)
            if matches!(
                e.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to read custom role '{}'", custom_role.role_id),
            resource_id: Some(permission_set_id.to_string()),
        })),
    }
}

async fn converge_gcp_custom_role(
    iam: &dyn IamApi,
    permission_set_id: &str,
    custom_role: &GcpCustomRole,
    existing: Role,
    desired: Role,
) -> Result<()> {
    if existing.deleted.unwrap_or(false) {
        // Undelete restores the role as it was when deleted; the patch below
        // brings it to the current permissions.
        iam.undelete_role(custom_role.name.clone()).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to undelete custom role '{}'", custom_role.role_id),
                resource_id: Some(permission_set_id.to_string()),
            },
        )?;
        info!(role_id = %custom_role.role_id, "Undeleted GCP custom role for reuse");
    } else if gcp_custom_role_matches(&existing, &desired) {
        info!(
            role_id = %custom_role.role_id,
            permission_set = %permission_set_id,
            "GCP custom role already matches desired permissions"
        );
        return Ok(());
    }

    iam.patch_role(
        custom_role.name.clone(),
        desired,
        Some(GCP_CUSTOM_ROLE_UPDATE_MASK.to_string()),
    )
    .await
    .context(ErrorData::CloudPlatformError {
        message: format!("Failed to update custom role '{}'", custom_role.role_id),
        resource_id: Some(permission_set_id.to_string()),
    })?;
    Ok(())
}

/// Helper for applying resource-scoped permissions across all platforms
pub struct ResourcePermissionsHelper;

impl ResourcePermissionsHelper {
    pub fn kubernetes_cluster_name_for_permissions(
        resource_prefix: &str,
        cluster: &KubernetesCluster,
    ) -> String {
        cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.cluster_name.clone())
            .unwrap_or_else(|| format!("{resource_prefix}-k8s"))
    }

    pub fn aws_kubernetes_cluster_permission_context(
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext> {
        let aws_config = ctx.get_aws_config()?;
        let cluster_name =
            Self::kubernetes_cluster_name_for_permissions(ctx.resource_prefix, cluster);
        let aws_account_id = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.account_id.clone())
            .unwrap_or_else(|| aws_config.account_id.clone());
        let aws_region = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.region.clone())
            .unwrap_or_else(|| aws_config.region.clone());

        Ok(PermissionContext::new()
            .with_aws_account_id(aws_account_id)
            .with_aws_region(aws_region)
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(cluster_name))
    }

    pub fn gcp_kubernetes_cluster_permission_context(
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
        service_account_name: Option<&str>,
    ) -> Result<PermissionContext> {
        let gcp_config = ctx.get_gcp_config()?;
        let project_id = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.project_id.clone())
            .unwrap_or_else(|| gcp_config.project_id.clone());
        let region = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.region.clone())
            .unwrap_or_else(|| gcp_config.region.clone());

        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_gcp_custom_role_namespace(Self::gcp_custom_role_namespace(ctx)?)
            .with_project_name(project_id)
            .with_region(region)
            .with_resource_name(Self::kubernetes_cluster_name_for_permissions(
                ctx.resource_prefix,
                cluster,
            ));
        if let Some(project_number) = &gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }
        if let Some(service_account_name) = service_account_name {
            permission_context =
                permission_context.with_service_account_name(service_account_name.to_string());
        }
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }

        Ok(permission_context)
    }

    pub fn azure_kubernetes_cluster_permission_context(
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext> {
        let azure_config = ctx.get_azure_config()?;
        let stack_resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let resource_group = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.resource_group.clone())
            .unwrap_or_else(|| stack_resource_group.clone());
        let subscription_id = cluster
            .cloud
            .as_ref()
            .and_then(|cloud| cloud.subscription_id.clone())
            .unwrap_or_else(|| azure_config.subscription_id.clone());

        let mut permission_context = PermissionContext::new()
            .with_subscription_id(subscription_id.clone())
            .with_resource_group(resource_group)
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(Self::kubernetes_cluster_name_for_permissions(
                ctx.resource_prefix,
                cluster,
            ))
            .with_managing_subscription_id(azure_config.subscription_id.clone())
            .with_managing_resource_group(stack_resource_group);
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }

        Ok(permission_context)
    }

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
        Self::ensure_permission_write_authority(ctx, resource_id)?;
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
        Self::ensure_permission_write_authority(ctx, resource_id)?;
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

    /// Setup-only: idempotently create or update GCP custom roles from a permission set.
    ///
    /// Live resource reconciliation must bind principals to roles created by
    /// setup and must not call this helper to repair missing role definitions.
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

    /// Setup-only: idempotently create or update all custom roles in a GCP grant plan.
    pub async fn ensure_all_gcp_custom_roles(
        ctx: &ResourceControllerContext<'_>,
        permission_set_id: &str,
        grant_plan: &GcpGrantPlan,
    ) -> Result<()> {
        Self::ensure_gcp_custom_roles(ctx, permission_set_id, grant_plan.custom_roles.clone()).await
    }

    /// Setup-only: idempotently create or update custom roles referenced by selected bindings.
    pub async fn ensure_gcp_custom_roles_for_bindings(
        ctx: &ResourceControllerContext<'_>,
        permission_set_id: &str,
        grant_plan: &GcpGrantPlan,
        bindings: &[GcpIamBinding],
    ) -> Result<()> {
        Self::ensure_gcp_custom_roles(
            ctx,
            permission_set_id,
            grant_plan.custom_roles_for_bindings(bindings),
        )
        .await
    }

    /// Setup-only: idempotently create or update the selected GCP custom roles.
    ///
    /// Terraform/CloudFormation/CLI setup owns role definitions. Runtime
    /// resource controllers should fail if a required role is missing.
    pub async fn ensure_gcp_custom_roles(
        ctx: &ResourceControllerContext<'_>,
        permission_set_id: &str,
        custom_roles: Vec<GcpCustomRole>,
    ) -> Result<()> {
        if custom_roles.is_empty() {
            return Ok(());
        }

        let gcp_config = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        let mut seen_role_names = HashSet::new();
        for custom_role in custom_roles {
            if seen_role_names.insert(custom_role.name.clone()) {
                ensure_gcp_custom_role(iam_client.as_ref(), permission_set_id, &custom_role)
                    .await?;
            }
        }

        Ok(())
    }

    /// Setup-delete: delete every GCP custom role this deployment created.
    ///
    /// A role belongs to this deployment when its ID is in one of the
    /// deployment's namespaces and its description names the deployment's
    /// resource prefix. The ID alone does not prove it: `role_acme_` also
    /// starts every role of a deployment with prefix `acme-prod`. Listing the
    /// project's roles, rather than regenerating IDs from the current stack,
    /// also finds the roles of permission sets an earlier update removed.
    ///
    /// Project IAM/resource IAM bindings must be removed before this runs. Missing
    /// roles are tolerated so delete stays idempotent.
    pub async fn delete_gcp_custom_roles(ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let gcp_config = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
        let mut namespaces = vec![
            GcpCustomRoleNaming::HashedLongPrefix.namespace(ctx.resource_prefix),
            GcpCustomRoleNaming::TruncatedPrefix.namespace(ctx.resource_prefix),
        ];
        namespaces.sort();
        namespaces.dedup();
        let role_name_prefixes: Vec<String> = namespaces
            .iter()
            .map(|namespace| format!("projects/{}/roles/role_{namespace}_", gcp_config.project_id))
            .collect();
        let mut role_names = Vec::new();
        let mut page_token = None;

        loop {
            let response = iam_client
                .list_roles(Some(100), page_token, Some(false))
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to list GCP custom roles before cleanup".to_string(),
                    resource_id: Some(ctx.resource_prefix.to_string()),
                })?;

            for role in response.roles {
                let Some(role_name) = role.name else {
                    continue;
                };
                if !role_name_prefixes
                    .iter()
                    .any(|prefix| role_name.starts_with(prefix))
                {
                    continue;
                }
                let Some(description) = role.description else {
                    warn!(
                        role_name = %role_name,
                        resource_prefix = %ctx.resource_prefix,
                        "Skipping GCP custom role without a description; ownership cannot be verified"
                    );
                    continue;
                };
                if custom_role_description_names_prefix(&description, ctx.resource_prefix) {
                    role_names.push(role_name);
                }
            }

            match response.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }

        for role_name in role_names {
            let role_id = role_name
                .rsplit('/')
                .next()
                .unwrap_or(role_name.as_str())
                .to_string();
            match iam_client.delete_role(role_name.clone()).await {
                Ok(_) => {
                    info!(
                        role_id = %role_id,
                        "Deleted GCP custom role"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        role_id = %role_id,
                        "GCP custom role already deleted"
                    );
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete GCP custom role '{}'", role_id),
                        resource_id: Some(ctx.resource_prefix.to_string()),
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
    ///
    /// Custom role definitions are setup-owned. For Frozen resources this helper
    /// may repair missing setup-owned role definitions during InitialSetup retry;
    /// Live resources only receive bindings to roles that setup already created.
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
                    resource_id,
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

        Self::collect_gcp_remote_bindings(
            ctx,
            resource_id,
            &generator,
            &permission_context,
            all_bindings,
        )
        .await?;

        Ok(())
    }

    async fn collect_gcp_remote_bindings(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        all_bindings: &mut Vec<GcpIamBinding>,
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
            .require_dependency::<crate::remote_bindings::GcpRemoteBindingsController>(
                &(&identity_entry.config).into(),
            )?;
        let email = controller.service_account_email.as_ref().ok_or_else(|| {
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
        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Resource, permission_context)
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to generate Remote Bindings grant '{}'",
                    definition.permission_set
                ),
                resource_id: Some(resource_id.to_string()),
            })?;
        let mut selected = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
        Self::ensure_gcp_custom_roles_for_resource_if_frozen(
            ctx,
            resource_id,
            &permission_set.id,
            &grant_plan,
            &selected,
        )
        .await?;
        let member = format!("serviceAccount:{email}");
        for binding in &mut selected {
            binding.members = vec![member.clone()];
        }
        all_bindings.extend(selected);
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
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_ctx = permission_ctx.with_deployment_name(deployment_name.to_string());
        }

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
    pub(crate) fn build_gcp_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        Ok(Self::gcp_permission_context(ctx)?.with_resource_name(resource_name.to_string()))
    }

    /// Build the deployment-wide GCP permission context.
    pub(crate) fn gcp_permission_context(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<PermissionContext> {
        let gcp_config = ctx.get_gcp_config()?;

        let mut permission_ctx = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_gcp_custom_role_namespace(Self::gcp_custom_role_namespace(ctx)?);
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_ctx = permission_ctx.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_ctx = permission_ctx.with_project_number(project_number.clone());
        }
        Ok(permission_ctx)
    }

    /// Return the namespace of this deployment's GCP custom role IDs.
    pub fn gcp_custom_role_namespace(ctx: &ResourceControllerContext<'_>) -> Result<String> {
        Ok(GcpCustomRoleNaming::for_deployment(ctx.state)?.namespace(ctx.resource_prefix))
    }

    /// Process GCP permissions for a specific profile
    async fn process_gcp_profile_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
        all_bindings: &mut Vec<GcpIamBinding>,
    ) -> Result<()> {
        // Get the service account for this profile
        let service_account_email = Self::get_gcp_service_account_email(ctx, profile_name)?;

        let permission_context = permission_context.clone().with_service_account_name(
            service_account_email
                .split('@')
                .next()
                .unwrap_or(&service_account_email)
                .to_string(),
        );

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
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;
            let selected_bindings =
                grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            Self::ensure_gcp_custom_roles_for_resource_if_frozen(
                ctx,
                resource_id,
                &permission_set.id,
                &grant_plan,
                &selected_bindings,
            )
            .await?;

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
        let permission_context = permission_context.clone().with_service_account_name(
            management_sa_email
                .split('@')
                .next()
                .unwrap_or(&management_sa_email)
                .to_string(),
        );

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
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            let selected_bindings =
                grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            Self::ensure_gcp_custom_roles_for_resource_if_frozen(
                ctx,
                resource_id,
                &permission_set.id,
                &grant_plan,
                &selected_bindings,
            )
            .await?;

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
    /// It applies these setup-owned policies only while Alien has direct setup
    /// authority. Imported handoffs and normal runtime execution must never
    /// create or broaden resource-scoped IAM policies.
    pub async fn apply_aws_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
    ) -> Result<()> {
        if !Self::resource_is_setup_owned(ctx, resource_id)? {
            debug!(
                resource_id = %resource_id,
                resource_name = %resource_name,
                "Skipping AWS resource-scoped data policy attachment for live resource; these policies are setup-owned"
            );
            return Ok(());
        }

        let aws_config = ctx.get_aws_config()?;

        // Build permission context for this specific resource
        let mut permission_context = PermissionContext::new()
            .with_aws_account_id(aws_config.account_id.to_string())
            .with_aws_region(aws_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_id(resource_id.to_string())
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
            let combined_refs = Self::without_refs_the_stack_policy_grants(
                ctx,
                profile_name,
                profile,
                resource_id,
                Self::aws_resource_scoped_refs(profile, resource_id, &type_prefix),
                &generator,
                &permission_context,
            )?;

            if !combined_refs.is_empty() {
                info!(
                    resource_id = %resource_id,
                    resource_name = %resource_name,
                    profile = %profile_name,
                    permission_sets = ?combined_refs.iter().map(|r| r.id()).collect::<Vec<_>>(),
                    "Processing AWS resource-scoped permissions"
                );

                Self::process_aws_profile_permissions(
                    ctx,
                    resource_id,
                    profile_name,
                    &combined_refs,
                    &generator,
                    &permission_context,
                )
                .await?;
            }
        }

        // Setup-owned resources run while setup credentials are still active.
        Self::apply_aws_management_resource_permissions(
            ctx,
            resource_id,
            resource_name,
            resource_type,
            &generator,
            &permission_context,
        )
        .await?;

        Self::apply_aws_remote_bindings_resource_permission(
            ctx,
            resource_id,
            &generator,
            &permission_context,
        )
        .await?;

        Ok(())
    }

    async fn apply_aws_remote_bindings_resource_permission(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        generator: &AwsRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        let Some(entry) = ctx.desired_stack.resources.get(resource_id) else {
            return Ok(());
        };
        let definition = alien_core::remote_bindings::remote_binding_for_entry(entry);
        let desired_bindings_entry = ctx.desired_stack.resources.iter().find(|(_, entry)| {
            entry.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE
        });
        let role_name = if let Some((_, bindings_entry)) = desired_bindings_entry {
            let controller = ctx
                .require_dependency::<crate::remote_bindings::AwsRemoteBindingsController>(
                    &(&bindings_entry.config).into(),
                )?;
            controller.role_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: resource_id.to_string(),
                    dependency_id: "remote-bindings".to_string(),
                })
            })?
        } else if let Some(role_name) = ctx
            .state
            .resources
            .values()
            .find(|state| state.resource_type == alien_core::RemoteBindings::RESOURCE_TYPE.as_ref())
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<alien_core::RemoteBindingsOutputs>())
            .and_then(|outputs| outputs.resource_id.rsplit('/').next())
        {
            role_name.to_string()
        } else {
            return Ok(());
        };
        let policy_name = aws_remote_access_policy_name(resource_id);
        let iam = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;

        let Some(definition) = definition else {
            return match iam.delete_role_policy(&role_name, &policy_name).await {
                Ok(()) => Ok(()),
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    Ok(())
                }
                Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to revoke Remote Bindings policy from role '{role_name}'"
                    ),
                    resource_id: Some(resource_id.to_string()),
                })),
            };
        };
        let policy =
            aws_remote_access_policy(generator, definition, permission_context, resource_id)?;
        let policy_json = serde_json::to_string_pretty(&policy)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Failed to serialize Remote Bindings IAM policy".to_string(),
                resource_id: Some(resource_id.to_string()),
            })?;
        iam.put_role_policy(&role_name, &policy_name, &policy_json)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to apply Remote Bindings policy to role '{role_name}'"),
                resource_id: Some(resource_id.to_string()),
            })?;
        Ok(())
    }

    /// Resource-specific permissions plus matching wildcard permissions.
    fn aws_resource_scoped_refs(
        profile: &PermissionProfile,
        resource_id: &str,
        type_prefix: &str,
    ) -> Vec<PermissionSetReference> {
        let mut combined_refs: Vec<PermissionSetReference> = Vec::new();
        if let Some(permission_set_refs) = profile.0.get(resource_id) {
            combined_refs.extend(permission_set_refs.iter().cloned());
        }
        if let Some(wildcard_refs) = profile.0.get("*") {
            combined_refs.extend(
                wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with(type_prefix))
                    .cloned(),
            );
        }
        combined_refs
    }

    /// Drops the wildcard sets that the service account's stack-level policy already grants for
    /// this resource.
    ///
    /// A profile's `"*"` sets are written once on its role as the stack-level policy, scoped to
    /// the stack prefix. Writing them again per resource duplicates that grant, and IAM caps a
    /// role's inline policies at 10,240 characters in total, so a stack with a handful of
    /// resources fails setup. A set stays when the resource names it, or when the stack-level
    /// grant does not reach this resource (a bring-your-own resource outside the stack prefix).
    /// Policies written before this check stay on the role; they are only redundant.
    fn without_refs_the_stack_policy_grants(
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        profile: &PermissionProfile,
        resource_id: &str,
        refs: Vec<PermissionSetReference>,
        generator: &AwsRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<Vec<PermissionSetReference>> {
        let named_by_resource = profile.0.get(resource_id);
        let stack_sets = ctx
            .desired_stack
            .resources
            .get(&format!("{profile_name}-sa"))
            .and_then(|entry| entry.config.downcast_ref::<alien_core::ServiceAccount>())
            .map(|service_account| service_account.stack_permission_sets.as_slice())
            .unwrap_or_default();

        let mut kept = Vec::new();
        for reference in refs {
            // Resolved first: a reference may name its set by an alias, and the stack-level
            // policy holds resolved sets.
            let stack_set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .and_then(|resolved| stack_sets.iter().find(|set| **set == resolved));
            let granted = match stack_set {
                Some(set) if !named_by_resource.is_some_and(|named| named.contains(&reference)) => {
                    let generate = |target| {
                        generator
                            .generate_policy(set, target, permission_context)
                            .context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to generate policy for permission set '{}'",
                                    set.id
                                ),
                                resource_id: Some(resource_id.to_string()),
                            })
                    };
                    aws_policy_grants(
                        &generate(BindingTarget::Stack)?,
                        &generate(BindingTarget::Resource)?,
                    )
                }
                _ => false,
            };
            if granted {
                debug!(
                    resource_id = %resource_id,
                    profile = %profile_name,
                    permission_set = %reference.id(),
                    "Stack-level policy already grants this set for the resource; skipping its resource policy"
                );
            } else {
                kept.push(reference);
            }
        }
        Ok(kept)
    }

    /// Remove inline policies that `apply_aws_resource_scoped_permissions`
    /// attached for grants the desired stack no longer has.
    ///
    /// The upsert path never deletes, so without this a dropped grant (or a
    /// dropped permission set) stays on the role. Only exact owned names are
    /// removed: `alien-<resource>-<set>` on service-account roles and
    /// `alien-mgmt-<resource>-<set>` on the management role, for registered
    /// `<resource_type>/` permission sets. A role that no longer exists has
    /// nothing to remove.
    pub async fn remove_stale_aws_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_type: &str,
    ) -> Result<()> {
        if !Self::resource_is_setup_owned(ctx, resource_id)? {
            return Ok(());
        }

        let type_prefix = format!("{}/", resource_type);
        let registered_sets: Vec<&str> = alien_permissions::list_permission_set_ids()
            .into_iter()
            .filter(|id| id.starts_with(&type_prefix))
            .collect();

        // (role, owned policy names, desired policy names)
        let mut roles: Vec<(String, HashSet<String>, HashSet<String>)> = Vec::new();
        // A role may outlive its entire permission profile. Include retained
        // service accounts so deleting a profile also revokes its owned grants.
        let mut profile_names: HashSet<&str> = ctx
            .desired_stack
            .permissions
            .profiles
            .keys()
            .map(String::as_str)
            .collect();
        profile_names.extend(
            ctx.desired_stack
                .resources
                .iter()
                .filter_map(|(id, entry)| {
                    (entry.config.resource_type() == alien_core::ServiceAccount::RESOURCE_TYPE)
                        .then(|| id.strip_suffix("-sa"))
                        .flatten()
                }),
        );
        for profile_name in profile_names {
            let Some(role_name) = Self::existing_aws_service_account_role_name(ctx, profile_name)?
            else {
                continue;
            };
            let owned = registered_sets
                .iter()
                .map(|id| aws_resource_policy_name(resource_id, id))
                .collect();
            let desired = ctx
                .desired_stack
                .permissions
                .profiles
                .get(profile_name)
                .map(|profile| Self::aws_resource_scoped_refs(profile, resource_id, &type_prefix))
                .unwrap_or_default()
                .iter()
                .map(|r| aws_resource_policy_name(resource_id, r.id()))
                .collect();
            roles.push((role_name, owned, desired));
        }
        if let Some(role_name) = Self::existing_aws_management_role_name(ctx)? {
            // Provision sets are granted by RemoteStackManagement, never per resource.
            let owned = registered_sets
                .iter()
                .filter(|id| !id.ends_with("/provision"))
                .map(|id| aws_management_resource_policy_name(resource_id, id))
                .collect();
            let desired = ctx
                .desired_stack
                .management()
                .profile()
                .map(|profile| Self::aws_management_resource_permission_refs(profile, resource_id))
                .unwrap_or_default()
                .iter()
                .map(|r| aws_management_resource_policy_name(resource_id, r.id()))
                .collect();
            roles.push((role_name, owned, desired));
        }
        if roles.is_empty() {
            return Ok(());
        }

        let iam = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        for (role_name, owned, desired) in roles {
            let listed = match iam.list_role_policies(&role_name).await {
                Ok(response) => response.list_role_policies_result,
                Err(error)
                    if matches!(
                        error.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    continue;
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to list inline policies of role '{role_name}' to remove previous '{resource_id}' grants"
                        ),
                        resource_id: Some(resource_id.to_string()),
                    }));
                }
            };
            if listed.is_truncated == Some(true) {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Role '{role_name}' has more inline policies than one ListRolePolicies page; cannot prove previous '{resource_id}' grants were removed"
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
            let stale = listed
                .policy_names
                .map(|names| names.member)
                .unwrap_or_default()
                .into_iter()
                .filter(|name| owned.contains(name) && !desired.contains(name));
            for policy_name in stale {
                match iam.delete_role_policy(&role_name, &policy_name).await {
                    Ok(()) => {}
                    // Already removed, e.g. by an earlier attempt whose response was lost.
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(CloudClientErrorData::RemoteAccessDenied { .. })
                        ) =>
                    {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Setup credentials cannot delete inline policy '{policy_name}' from role '{role_name}' (iam:DeleteRolePolicy); grant it and rerun setup to remove the previous '{resource_id}' grant"
                            ),
                            resource_id: Some(resource_id.to_string()),
                        }));
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete inline policy '{policy_name}' from role '{role_name}'"
                            ),
                            resource_id: Some(resource_id.to_string()),
                        }));
                    }
                }
                info!(
                    role_name = %role_name,
                    policy_name = %policy_name,
                    resource_id = %resource_id,
                    "Removed AWS resource-scoped permission that is no longer granted"
                );
            }
        }
        Ok(())
    }

    /// The role of a profile's service account, if setup has created it.
    fn existing_aws_service_account_role_name(
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<Option<String>> {
        let service_account_id = format!("{}-sa", profile_name);
        let Some(resource) = ctx.desired_stack.resources.get(&service_account_id) else {
            return Ok(None);
        };
        let has_controller_state = ctx
            .state
            .resources
            .get(&service_account_id)
            .is_some_and(|state| state.internal_state.is_some());
        if !has_controller_state {
            return Ok(None);
        }
        Ok(ctx
            .require_dependency::<crate::service_account::AwsServiceAccountController>(
                &(&resource.config).into(),
            )?
            .role_name)
    }

    /// The management role, if the stack has one and setup has created it.
    fn existing_aws_management_role_name(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<String>> {
        let Some((id, entry)) = ctx.desired_stack.resources.iter().find(|(_, entry)| {
            entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE
        }) else {
            return Ok(None);
        };
        let has_controller_state = ctx
            .state
            .resources
            .get(id)
            .is_some_and(|state| state.internal_state.is_some());
        if !has_controller_state {
            return Ok(None);
        }
        Ok(ctx
            .require_dependency::<crate::remote_stack_management::AwsRemoteStackManagementController>(
                &(&entry.config).into(),
            )?
            .role_name)
    }

    pub(crate) fn resource_is_setup_owned(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        let setup_owned = ctx
            .desired_stack
            .resources
            .get(resource_id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen);
        if setup_owned
            && ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
        {
            return Err(AlienError::new(ErrorData::ImportedSetupStateInvalid {
                message: format!(
                    "resource '{resource_id}' reached a permission-applying state after setup handoff; regenerate and rerun setup"
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }
        Ok(setup_owned)
    }

    fn ensure_permission_write_authority(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<()> {
        Self::resource_is_setup_owned(ctx, resource_id).map(|_| ())
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

            let policy_name = aws_resource_policy_name(resource_id, &permission_set.id);

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

            let policy_name = aws_management_resource_policy_name(resource_id, &permission_set.id);

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
    ///
    /// Custom role definitions are setup-owned. For Frozen resources this helper
    /// may repair missing setup-owned role definitions during InitialSetup retry;
    /// Live resources only receive bindings to roles that setup already created.
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
        let permission_context = permission_context.clone().with_service_account_name(
            management_sa_email
                .split('@')
                .next()
                .unwrap_or(&management_sa_email)
                .to_string(),
        );

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
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate IAM grant plan for management permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })?;
            let selected_bindings = grant_plan.bindings_for_target(expected_target);
            Self::ensure_gcp_custom_roles_for_resource_if_frozen(
                ctx,
                resource_id,
                &permission_set.id,
                &grant_plan,
                &selected_bindings,
            )
            .await?;

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

    async fn ensure_gcp_custom_roles_for_resource_if_frozen(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        permission_set_id: &str,
        grant_plan: &GcpGrantPlan,
        bindings: &[GcpIamBinding],
    ) -> Result<()> {
        if !Self::is_frozen_resource(ctx, resource_id) {
            return Ok(());
        }

        Self::ensure_gcp_custom_roles_for_bindings(ctx, permission_set_id, grant_plan, bindings)
            .await
    }

    fn is_frozen_resource(ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        Self::is_frozen_resource_in_stack(ctx.desired_stack, resource_id)
    }

    fn is_frozen_resource_in_stack(stack: &alien_core::Stack, resource_id: &str) -> bool {
        stack
            .resources
            .get(resource_id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen)
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

/// The inline policy on the shared Remote Bindings role that carries one resource's remote grant.
/// Inline policy a resource attaches to a profile's service-account role.
/// Whether `broad` allows every request `narrow` allows.
///
/// Answers yes only when it can prove it: every statement of `narrow` must be an `Allow` with
/// plain `Resource`s that one `Allow` statement of `broad` matches, with no condition `narrow`
/// lacks. A `Deny`, a `NotResource` or a differing condition answers no, which keeps the
/// narrow policy.
fn aws_policy_grants(broad: &AwsIamPolicy, narrow: &AwsIamPolicy) -> bool {
    let plain_allow = |statement: &AwsIamStatement| {
        statement.effect == "Allow" && statement.not_resource.is_empty()
    };
    narrow.statement.iter().all(|wanted| {
        plain_allow(wanted)
            && broad.statement.iter().any(|granted| {
                plain_allow(granted)
                    && (granted.condition.is_none() || granted.condition == wanted.condition)
                    && wanted.action.iter().all(|action| {
                        granted.action.iter().any(|pattern| {
                            iam_pattern_contains(
                                &pattern.to_ascii_lowercase(),
                                &action.to_ascii_lowercase(),
                            )
                        })
                    })
                    && wanted.resource.iter().all(|resource| {
                        granted
                            .resource
                            .iter()
                            .any(|pattern| iam_pattern_contains(pattern, resource))
                    })
            })
    })
}

/// Whether every string IAM matches with `narrow` is also matched by `pattern`, both using IAM's
/// `*` (any run of characters) and `?` (one character).
///
/// A wildcard in `narrow` is only accepted against a wildcard in `pattern` at least as wide, so
/// a `true` is always a real containment; some true containments answer `false`.
fn iam_pattern_contains(pattern: &str, narrow: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let narrow: Vec<char> = narrow.chars().collect();
    // covered[j] after processing pattern[..i]: pattern[..i] covers narrow[..j].
    let mut covered = vec![false; narrow.len() + 1];
    covered[0] = true;
    for &p in &pattern {
        let mut next = vec![false; narrow.len() + 1];
        for j in 0..=narrow.len() {
            next[j] = match p {
                // `*` covers whatever an earlier prefix covered, extended by any character.
                '*' => covered[j] || (j > 0 && next[j - 1]),
                // `?` covers one character, or a `?` in `narrow`, never a `*`.
                '?' => j > 0 && covered[j - 1] && narrow[j - 1] != '*',
                // A literal never covers a wildcard in `narrow`, which matches more than it.
                literal => j > 0 && covered[j - 1] && narrow[j - 1] == literal,
            };
        }
        covered = next;
    }
    covered[narrow.len()]
}

fn aws_resource_policy_name(resource_id: &str, permission_set_id: &str) -> String {
    format!(
        "alien-{}-{}",
        resource_id,
        permission_set_id.replace('/', "-")
    )
}

/// Inline policy a resource attaches to the management role.
fn aws_management_resource_policy_name(resource_id: &str, permission_set_id: &str) -> String {
    format!(
        "alien-mgmt-{}-{}",
        resource_id,
        permission_set_id.replace('/', "-")
    )
}

pub(crate) fn aws_remote_access_policy_name(resource_id: &str) -> String {
    format!("alien-{resource_id}-remote-access")
}

pub(crate) fn aws_remote_access_policy(
    generator: &AwsRuntimePermissionsGenerator,
    definition: &alien_core::remote_bindings::RemoteBindingDefinition,
    permission_context: &PermissionContext,
    resource_id: &str,
) -> Result<AwsIamPolicy> {
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
    generator
        .generate_policy(&permission_set, BindingTarget::Resource, permission_context)
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to generate Remote Bindings policy '{}'",
                definition.permission_set
            ),
            resource_id: Some(resource_id.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use alien_core::{Stack, Storage};
    use alien_gcp_clients::iam::MockIamApi;
    use indexmap::IndexMap;
    use mockall::Sequence;

    #[test]
    fn gcp_resource_custom_roles_are_selected_for_resource_bindings() {
        let generator = GcpRuntimePermissionsGenerator::new();
        let permission_set =
            alien_permissions::get_permission_set("storage/data-read").expect("permission set");
        let permission_context = PermissionContext::new()
            .with_project_name("test-project")
            .with_region("us-central1")
            .with_stack_prefix("test")
            .with_resource_name("test-bucket")
            .with_service_account_name("reader");

        let grant_plan = generator
            .generate_grant_plan(permission_set, BindingTarget::Resource, &permission_context)
            .expect("grant plan");

        let resource_bindings =
            grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
        let account_bindings =
            grant_plan.bindings_for_target(GcpBindingTargetScope::ServiceAccount);

        let resource_custom_roles = grant_plan.custom_roles_for_bindings(&resource_bindings);
        let account_custom_roles = grant_plan.custom_roles_for_bindings(&account_bindings);

        assert_eq!(resource_custom_roles.len(), 1);
        assert!(resource_custom_roles[0]
            .included_permissions
            .iter()
            .any(|permission| permission == "storage.objects.get"));
        assert_eq!(account_custom_roles.len(), 1);
        assert_eq!(
            account_custom_roles[0].included_permissions,
            vec!["iam.serviceAccounts.signBlob"]
        );
    }

    #[test]
    fn gcp_resource_custom_role_repair_is_frozen_only() {
        let frozen_stack = Stack::new("test-stack".to_string())
            .add(
                Storage::new("logs".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let live_stack = Stack::new("test-stack".to_string())
            .add(
                Storage::new("logs".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert!(ResourcePermissionsHelper::is_frozen_resource_in_stack(
            &frozen_stack,
            "logs"
        ));
        assert!(!ResourcePermissionsHelper::is_frozen_resource_in_stack(
            &live_stack,
            "logs"
        ));
        assert!(!ResourcePermissionsHelper::is_frozen_resource_in_stack(
            &frozen_stack,
            "missing"
        ));
    }

    fn aws_policies(set_id: &str, resource_name: &str) -> (AwsIamPolicy, AwsIamPolicy) {
        let set = alien_permissions::get_permission_set(set_id)
            .unwrap_or_else(|| panic!("permission set '{set_id}' is registered"));
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012".to_string())
            .with_aws_region("us-east-1".to_string())
            .with_stack_prefix("p26eed71".to_string())
            .with_resource_id("res".to_string())
            .with_resource_name(resource_name.to_string());
        let generator = AwsRuntimePermissionsGenerator::new();
        (
            generator
                .generate_policy(set, BindingTarget::Stack, &context)
                .unwrap(),
            generator
                .generate_policy(set, BindingTarget::Resource, &context)
                .unwrap(),
        )
    }

    /// Every `"*"` set of a typical worker stack, for resources named under the stack prefix:
    /// the stack-level policy already grants each one, so none needs a policy per resource.
    #[test]
    fn stack_level_policy_grants_wildcard_sets_for_resources_under_the_stack_prefix() {
        for (set_id, resource_name) in [
            ("storage/data-read", "p26eed71-test-alien-storage"),
            ("storage/data-write", "p26eed71-test-alien-storage"),
            ("build/execute", "p26eed71-test-alien-build"),
            (
                "artifact-registry/pull",
                "p26eed71-test-alien-artifact-registry",
            ),
            (
                "artifact-registry/push",
                "p26eed71-test-alien-artifact-registry",
            ),
            (
                "artifact-registry/provision",
                "p26eed71-test-alien-artifact-registry",
            ),
            ("vault/data-read", "p26eed71-test-vault"),
            ("vault/data-write", "p26eed71-secrets"),
            ("kv/data-read", "p26eed71-test-alien-kv"),
            ("kv/data-write", "p26eed71-test-alien-kv"),
            ("queue/data-read", "p26eed71-test-alien-queue"),
            ("queue/data-write", "p26eed71-test-alien-queue"),
        ] {
            let (stack, resource) = aws_policies(set_id, resource_name);
            assert!(
                aws_policy_grants(&stack, &resource),
                "{set_id} on {resource_name}: stack {stack:#?} resource {resource:#?}"
            );
        }
    }

    /// A bring-your-own bucket is named outside the stack prefix, so only its own policy
    /// reaches it.
    #[test]
    fn stack_level_policy_does_not_grant_a_resource_outside_the_stack_prefix() {
        for set_id in ["storage/data-read", "storage/data-write"] {
            let (stack, resource) = aws_policies(set_id, "customer-data");
            assert!(!aws_policy_grants(&stack, &resource), "{set_id}");
        }
    }

    #[test]
    fn iam_pattern_containment_never_claims_a_wider_pattern() {
        // (pattern, narrow, pattern matches everything narrow matches)
        for (pattern, narrow, contains) in [
            ("arn:aws:s3:::p-*", "arn:aws:s3:::p-bucket", true),
            ("arn:aws:s3:::p-*", "arn:aws:s3:::p-bucket/*", true),
            (
                "arn:aws:ecr:*:1:repository/p-*",
                "arn:aws:ecr:us-east-1:1:repository/p-r-*",
                true,
            ),
            ("role/p-*-pull", "role/p-registry-pull", true),
            ("*", "*", true),
            ("a*", "a?", true),
            ("a?c", "a?c", true),
            ("arn:aws:s3:::p-*", "arn:aws:s3:::customer", false),
            ("arn:aws:s3:::p-bucket", "arn:aws:s3:::p-*", false),
            ("a?c", "a*c", false),
            ("abc", "a?c", false),
            ("role/p-*-pull", "role/p-registry-push", false),
        ] {
            assert_eq!(
                iam_pattern_contains(pattern, narrow),
                contains,
                "{pattern} vs {narrow}"
            );
        }
    }

    /// A broad statement with a condition grants less than an unconditional one.
    #[test]
    fn conditioned_stack_statement_does_not_grant_an_unconditioned_one() {
        let (mut stack, resource) = aws_policies("kv/data-read", "p26eed71-test-alien-kv");
        assert!(aws_policy_grants(&stack, &resource));
        for statement in &mut stack.statement {
            statement.condition = Some(
                [(
                    "StringEquals".to_string(),
                    [("aws:ResourceTag/owner".to_string(), "x".to_string())].into(),
                )]
                .into(),
            );
        }
        assert!(!aws_policy_grants(&stack, &resource));
    }

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
    fn narrowed_signing_plan_removes_old_project_member_and_preserves_other_members() {
        let current = alien_permissions::get_permission_set("storage/data-read").unwrap();
        let mut old = current.clone();
        for entry in old.platforms.gcp.as_mut().unwrap() {
            if entry.grant.permissions.as_ref().is_some_and(|permissions| {
                permissions
                    .iter()
                    .any(|permission| permission == "iam.serviceAccounts.signBlob")
            }) {
                entry.binding.resource.as_mut().unwrap().scope =
                    "projects/${projectName}".to_string();
            }
        }
        let context = PermissionContext::new()
            .with_project_name("test-project")
            .with_stack_prefix("test")
            .with_resource_name("test-objects")
            .with_service_account_name("reader");
        let generator = GcpRuntimePermissionsGenerator::new();
        let before = generator
            .generate_grant_plan(&old, BindingTarget::Resource, &context)
            .unwrap();
        let after = generator
            .generate_grant_plan(current, BindingTarget::Resource, &context)
            .unwrap();
        let project = before.bindings_for_target(GcpBindingTargetScope::Project);
        assert_eq!(project.len(), 1);
        assert!(after
            .bindings_for_target(GcpBindingTargetScope::Project)
            .is_empty());
        assert_eq!(
            before.bindings_for_target(GcpBindingTargetScope::CurrentResource),
            after.bindings_for_target(GcpBindingTargetScope::CurrentResource)
        );
        let own = after.bindings_for_target(GcpBindingTargetScope::ServiceAccount);
        assert_eq!(own.len(), 1);
        assert_eq!(
            own[0].target_resource_name.as_deref(),
            Some(
                "projects/test-project/serviceAccounts/reader@test-project.iam.gserviceaccount.com"
            )
        );
        let roles = after.custom_roles_for_bindings(&own);
        assert_eq!(roles.len(), 1);
        assert_eq!(
            roles[0].included_permissions,
            vec!["iam.serviceAccounts.signBlob"]
        );
        let member = "serviceAccount:reader@test-project.iam.gserviceaccount.com";
        let other = "serviceAccount:writer@test-project.iam.gserviceaccount.com";
        let mut bindings = project
            .into_iter()
            .map(ResourcePermissionsHelper::gcp_policy_binding_from_iam_binding)
            .collect::<Vec<_>>();
        bindings[0].members = vec![member.to_string(), other.to_string()];
        let old_role = bindings[0].role.clone();
        let owned_prefix = ResourcePermissionsHelper::gcp_stack_custom_role_name_prefix(&context);
        assert!(old_role.starts_with(&owned_prefix));
        let unrelated = Binding {
            role: "projects/test-project/roles/unrelated_signer".to_string(),
            members: vec![member.to_string()],
            condition: None,
        };
        bindings.push(unrelated.clone());
        assert!(
            ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
                &mut bindings,
                vec![],
                member,
                &[owned_prefix.clone()],
                &[],
            )
        );
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].role, old_role);
        assert_eq!(bindings[0].members, vec![other]);
        assert_eq!(
            serde_json::to_value(&bindings[1]).unwrap(),
            serde_json::to_value(&unrelated).unwrap()
        );
        let converged = bindings.clone();
        assert!(
            !ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
                &mut bindings,
                vec![],
                member,
                &[owned_prefix],
                &[],
            )
        );
        assert_eq!(
            serde_json::to_value(&bindings).unwrap(),
            serde_json::to_value(&converged).unwrap()
        );
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

    const PROBE_ROLE_NAME: &str = "projects/p/roles/role_acme_storage_data_write";

    fn desired_custom_role() -> GcpCustomRole {
        GcpCustomRole {
            role_id: "role_acme_storage_data_write".to_string(),
            name: PROBE_ROLE_NAME.to_string(),
            title: "acme: Storage data write".to_string(),
            description: "Used by acme. Write objects. Resource prefix: acme.".to_string(),
            included_permissions: vec![
                "storage.objects.create".to_string(),
                "storage.objects.get".to_string(),
            ],
            stage: "GA".to_string(),
        }
    }

    fn gcp_role(permissions: &[&str], deleted: bool) -> Role {
        let desired = desired_custom_role();
        Role {
            name: Some(desired.name),
            title: Some(desired.title),
            description: Some(desired.description),
            included_permissions: permissions.iter().map(|p| p.to_string()).collect(),
            stage: Some(RoleLaunchStage::Ga),
            etag: Some("BwZc7xFUf9U=".to_string()),
            deleted: deleted.then_some(true),
        }
    }

    /// Map a GCP error body exactly as `IamClient` does for a real response.
    fn gcp_error(status: u16, body: &str) -> alien_error::AlienError<CloudClientErrorData> {
        AlienError::new(alien_gcp_clients::gcp_request_utils::map_gcp_error(
            status,
            body,
            "https://iam.googleapis.com/v1/projects/p/roles",
            "create_role",
            "role_acme_storage_data_write",
            "role",
            None,
        ))
    }

    fn not_found() -> alien_error::AlienError<CloudClientErrorData> {
        gcp_error(
            404,
            r#"{"error":{"code":404,"message":"The role named projects/p/roles/role_acme_storage_data_write was not found.","status":"NOT_FOUND"}}"#,
        )
    }

    fn expect_patch_to_desired(iam: &mut MockIamApi, seq: &mut Sequence) {
        iam.expect_patch_role()
            .times(1)
            .in_sequence(seq)
            .withf(|name, role, mask| {
                name == PROBE_ROLE_NAME
                    && role.included_permissions
                        == vec!["storage.objects.create", "storage.objects.get"]
                    && role.title.as_deref() == Some("acme: Storage data write")
                    && role.deleted.is_none()
                    && mask.as_deref() == Some(GCP_CUSTOM_ROLE_UPDATE_MASK)
            })
            .returning(|_, role, _| Ok(role));
    }

    #[tokio::test]
    async fn gcp_custom_role_deleted_by_a_previous_deployment_is_undeleted_and_updated() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], true)));
        iam.expect_undelete_role()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|name| name == PROBE_ROLE_NAME)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);
        iam.expect_create_role().never();

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("a soft-deleted role should be reused");
    }

    #[tokio::test]
    async fn gcp_custom_role_live_with_stale_permissions_is_updated_in_place() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);
        iam.expect_undelete_role().never();
        iam.expect_create_role().never();

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("a live role should be updated");
    }

    #[tokio::test]
    async fn gcp_custom_role_already_matching_is_left_alone() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role().times(1).returning(|_| {
            Ok(gcp_role(
                &["storage.objects.get", "storage.objects.create"],
                false,
            ))
        });
        iam.expect_patch_role().never();
        iam.expect_undelete_role().never();
        iam.expect_create_role().never();

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("a matching role needs no write");
    }

    #[tokio::test]
    async fn gcp_custom_role_missing_is_created_with_its_stable_id() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role()
            .times(1)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .withf(|role_id, request| {
                role_id == "role_acme_storage_data_write"
                    && request.role.included_permissions
                        == vec!["storage.objects.create", "storage.objects.get"]
            })
            .returning(|_, request| Ok(request.role));
        iam.expect_undelete_role().never();
        iam.expect_patch_role().never();

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("a missing role should be created");
    }

    #[tokio::test]
    async fn gcp_custom_role_created_concurrently_is_updated_instead_of_failing() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    409,
                    r#"{"error":{"code":409,"message":"A role named role_acme_storage_data_write in projects/p already exists.","status":"ALREADY_EXISTS"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("losing a create race should converge on the existing role");
    }

    #[tokio::test]
    async fn gcp_custom_role_deleted_between_read_and_create_is_undeleted() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    400,
                    r#"{"error":{"code":400,"message":"You can't create a role with role_id (role_acme_storage_data_write) where there is an existing role with that role_id in a deleted state.","status":"FAILED_PRECONDITION"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], true)));
        iam.expect_undelete_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("a role soft-deleted during setup should be reused");
    }

    #[tokio::test]
    async fn gcp_custom_role_stale_read_after_create_conflict_converges_on_retry() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    409,
                    r#"{"error":{"code":409,"message":"A role named role_acme_storage_data_write in projects/p already exists.","status":"ALREADY_EXISTS"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);
        iam.expect_undelete_role().never();

        let error = ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect_err("an inconclusive read should return the create conflict");
        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(error.retryable);
        assert_eq!(
            error.source.as_ref().unwrap().code,
            "REMOTE_RESOURCE_CONFLICT"
        );
        assert!(error
            .source
            .as_ref()
            .unwrap()
            .message
            .contains("already exists"));

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("the caller's retry should update the now-visible role");
    }

    #[tokio::test]
    async fn gcp_custom_role_stale_read_after_soft_delete_converges_on_retry() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    400,
                    r#"{"error":{"code":400,"message":"You can't create a role with role_id (role_acme_storage_data_write) where there is an existing role with that role_id in a deleted state.","status":"FAILED_PRECONDITION"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], true)));
        iam.expect_undelete_role()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|name| name == PROBE_ROLE_NAME)
            .returning(|_| Ok(gcp_role(&["storage.objects.get"], false)));
        expect_patch_to_desired(&mut iam, &mut seq);

        let error = ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect_err("a soft-deleted role hidden by a stale read can still be reused");
        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(error.retryable);
        assert!(error
            .source
            .as_ref()
            .unwrap()
            .message
            .contains("deleted state"));

        ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect("the caller's retry should undelete and update the now-visible role");
    }

    #[tokio::test]
    async fn gcp_custom_role_unknown_precondition_and_missing_read_remain_retryable() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    400,
                    r#"{"error":{"code":400,"message":"A role precondition was not met.","status":"FAILED_PRECONDITION"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_undelete_role().never();
        iam.expect_patch_role().never();

        let error = ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect_err("an unknown precondition does not establish permanent deletion");
        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(error.retryable);
        assert_eq!(
            error.source.as_ref().unwrap().code,
            "REMOTE_RESOURCE_CONFLICT"
        );
        assert!(error
            .source
            .as_ref()
            .unwrap()
            .message
            .contains("precondition was not met"));
    }

    #[tokio::test]
    async fn gcp_custom_role_failed_read_after_conflict_preserves_read_error() {
        let mut iam = MockIamApi::new();
        let mut seq = Sequence::new();
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Err(not_found()));
        iam.expect_create_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| {
                Err(gcp_error(
                    400,
                    r#"{"error":{"code":400,"message":"You can't create a role_id (role_acme_storage_data_write) which has been marked for deletion.","status":"FAILED_PRECONDITION"}}"#,
                ))
            });
        iam.expect_get_role()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| {
                Err(gcp_error(
                    503,
                    r#"{"error":{"code":503,"message":"IAM is temporarily unavailable.","status":"UNAVAILABLE"}}"#,
                ))
            });
        iam.expect_undelete_role().never();
        iam.expect_patch_role().never();

        let error = ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect_err("a failed read cannot establish role absence");
        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert!(error.retryable);
        assert_eq!(
            error.source.as_ref().unwrap().code,
            "REMOTE_SERVICE_UNAVAILABLE"
        );
    }

    #[tokio::test]
    async fn gcp_custom_role_id_in_permanent_deletion_fails_without_retry() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role()
            .times(2)
            .returning(|_| Err(not_found()));
        iam.expect_create_role().times(1).returning(|_, _| {
            Err(gcp_error(
                400,
                r#"{"error":{"code":400,"message":"You can't create a role_id (role_acme_storage_data_write) which has been marked for deletion.","status":"FAILED_PRECONDITION"}}"#,
            ))
        });
        iam.expect_undelete_role().never();
        iam.expect_patch_role().never();

        let error = ensure_gcp_custom_role(&iam, "storage/data-write", &desired_custom_role())
            .await
            .expect_err("GCP blocks the ID until the old role is purged");

        assert_eq!(error.code, "GCP_CUSTOM_ROLE_ID_UNAVAILABLE");
        assert!(
            !error.retryable,
            "retrying cannot succeed for weeks, so the deployment must fail fast"
        );
        assert!(error.message.contains("role_acme_storage_data_write"));
        assert!(error.message.contains("different resource prefix"));
    }
}

use std::time::Duration;
use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::permissions::PermissionSet;
use alien_core::{
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::iam::{
    Binding, CreateServiceAccountRequest, IamPolicy, ServiceAccount as GcpServiceAccount,
};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};

/// Generates the GCP service account ID for RemoteStackManagement.
fn get_gcp_management_service_account_id(prefix: &str) -> String {
    format!("{}-management", prefix)
}

#[controller]
pub struct GcpRemoteStackManagementController {
    /// The email of the created management service account.
    pub(crate) service_account_email: Option<String>,
    /// The unique ID of the created management service account.
    pub(crate) service_account_unique_id: Option<String>,
    /// The name/ID of the created custom management role.
    pub(crate) custom_role_name: Option<String>,
    /// Whether the custom role has been created.
    pub(crate) role_created: bool,
    /// Whether the service account has been bound to the role.
    pub(crate) role_bound: bool,
    /// Whether impersonation permissions have been granted
    pub(crate) impersonation_granted: bool,
}

#[controller]
impl GcpRemoteStackManagementController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingServiceAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        let service_account_id = get_gcp_management_service_account_id(ctx.resource_prefix);

        info!(
            service_account_id = %service_account_id,
            config_id = %config.id,
            "Creating GCP management service account"
        );

        let service_account = GcpServiceAccount::builder()
            .display_name(format!(
                "Alien Management Service Account: {}",
                ctx.resource_prefix
            ))
            .description(format!(
                "Management service account for Alien stack {}",
                ctx.resource_prefix
            ))
            .build();

        let request = CreateServiceAccountRequest::builder()
            .service_account(service_account)
            .build();

        let created_sa = client
            .create_service_account(service_account_id.clone(), request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create GCP management service account '{}'",
                    service_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let email = created_sa.email.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created management service account missing email".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let unique_id = created_sa.unique_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created management service account missing unique_id".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            service_account_id = %service_account_id,
            email = %email,
            unique_id = %unique_id,
            "Management service account created successfully"
        );

        self.service_account_email = Some(email);
        self.service_account_unique_id = Some(unique_id);

        Ok(HandlerAction::Continue {
            state: CreatingCustomRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingCustomRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_custom_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let permission_sets = Self::resolve_management_permission_sets(ctx)?;

        if permission_sets.is_empty() {
            info!("No management permission sets to create custom roles for");
        } else {
            // All permission sets need custom roles created (both provision and non-provision),
            // because resource controllers reference non-provision custom roles when applying
            // resource-level IAM bindings.
            info!(
                permission_sets_count = permission_sets.len(),
                "Ensuring per-permission-set custom roles exist for management"
            );

            ResourcePermissionsHelper::ensure_gcp_stack_custom_roles(ctx, &permission_sets).await?;

            self.role_created = true;
        }

        Ok(HandlerAction::Continue {
            state: BindingRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = BindingRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn binding_role(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        let service_account_email = self.service_account_email.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Management service account email not available for role binding"
                    .to_string(),
                operation: Some("binding_role".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let permission_sets = Self::resolve_management_permission_sets(ctx)?;

        // Only provision permission sets (ID ends with "/provision") need project-level IAM
        // bindings. Non-provision sets (management, heartbeat, etc.) are applied by resource
        // controllers via resource-level IAM, so they don't need project-level binding here.
        let provision_sets: Vec<_> = permission_sets
            .into_iter()
            .filter(|ps| ps.id.ends_with("/provision"))
            .collect();

        if !provision_sets.is_empty() {
            info!(
                service_account_email = %service_account_email,
                provision_sets_count = provision_sets.len(),
                "Binding provision permission-set roles to service account at project level"
            );

            let generator = GcpRuntimePermissionsGenerator::new();
            let gcp_config = ctx.get_gcp_config()?;

            // Extract the account ID (part before '@') from the full email.
            let service_account_id = service_account_email
                .split('@')
                .next()
                .unwrap_or(service_account_email);

            let mut permission_context = PermissionContext::new()
                .with_stack_prefix(ctx.resource_prefix.to_string())
                .with_project_name(gcp_config.project_id.clone())
                .with_region(gcp_config.region.clone())
                .with_service_account_name(service_account_id.to_string());
            if let Some(ref project_number) = gcp_config.project_number {
                permission_context = permission_context.with_project_number(project_number.clone());
            }

            let mut new_bindings = Vec::new();

            for permission_set in &provision_sets {
                let bindings = generator
                    .generate_bindings(permission_set, BindingTarget::Stack, &permission_context)
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate IAM bindings for management permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("binding_role".to_string()),
                        resource_id: Some(config.id.clone()),
                    })?;

                for binding in bindings.bindings {
                    new_bindings.push(Binding {
                        role: binding.role,
                        members: binding.members,
                        condition: binding.condition.map(|cond| alien_gcp_clients::iam::Expr {
                            expression: cond.expression,
                            title: Some(cond.title),
                            description: Some(cond.description),
                            location: None,
                        }),
                    });
                }
            }

            if !new_bindings.is_empty() {
                let project_id = &gcp_config.project_id;
                let rm_client = ctx
                    .service_provider
                    .get_gcp_resource_manager_client(gcp_config)?;

                let current_policy = rm_client
                    .get_project_iam_policy(project_id.clone(), Some(alien_gcp_clients::resource_manager::GetPolicyOptions { requested_policy_version: Some(3) }))
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to get project IAM policy before binding management roles. Refusing to proceed to avoid overwriting existing bindings.".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                let mut all_bindings = current_policy.bindings;
                for new_binding in new_bindings {
                    let existing = all_bindings.iter_mut().find(|b| {
                        b.role == new_binding.role
                            && match (&b.condition, &new_binding.condition) {
                                (None, None) => true,
                                (Some(a), Some(b)) => a.expression == b.expression,
                                _ => false,
                            }
                    });

                    if let Some(existing) = existing {
                        for member in &new_binding.members {
                            if !existing.members.contains(member) {
                                existing.members.push(member.clone());
                            }
                        }
                    } else {
                        all_bindings.push(new_binding);
                    }
                }

                let new_policy = IamPolicy::builder()
                    .version(3)
                    .bindings(all_bindings)
                    .maybe_etag(current_policy.etag)
                    .maybe_kind(current_policy.kind)
                    .maybe_resource_id(current_policy.resource_id)
                    .build();

                rm_client
                    .set_project_iam_policy(project_id.clone(), new_policy, None)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to bind management roles to service account '{}' at project level", service_account_email),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(
                    service_account_email = %service_account_email,
                    "Provision permission-set roles bound to service account at project level"
                );

                self.role_bound = true;
            }
        }

        Ok(HandlerAction::Continue {
            state: GrantingImpersonation,
            suggested_delay: None,
        })
    }

    #[handler(
        state = GrantingImpersonation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn granting_impersonation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let gcp_config = ctx.get_gcp_config()?;

        let service_account_email = self.service_account_email.as_ref().unwrap();

        // Get management service account email from stack settings (required for cross-account management)
        let gcp_management = ctx.get_gcp_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "GCP management configuration is required for RemoteStackManagement. Please configure management settings in your stack.".to_string(),
                operation: Some("grant_impersonation_permissions".to_string()),
                resource_id: Some(config.id.clone()),
            }))?;
        let management_service_account_email = &gcp_management.service_account_email;

        info!(
            target_service_account = %service_account_email,
            management_service_account = %management_service_account_email,
            "Granting impersonation permissions to management service account"
        );

        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        // Get current service account IAM policy
        let current_policy = iam_client
            .get_service_account_iam_policy(service_account_email.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for service account '{}' before granting impersonation. Refusing to proceed to avoid overwriting existing bindings.", service_account_email),
                resource_id: Some(config.id.clone()),
            })?;

        // Add impersonation bindings
        let mut all_bindings = current_policy.bindings;

        // Grant roles/iam.serviceAccountTokenCreator
        all_bindings.push(
            Binding::builder()
                .role("roles/iam.serviceAccountTokenCreator".to_string())
                .members(vec![format!(
                    "serviceAccount:{}",
                    management_service_account_email
                )])
                .build(),
        );

        // Grant roles/iam.serviceAccountUser
        all_bindings.push(
            Binding::builder()
                .role("roles/iam.serviceAccountUser".to_string())
                .members(vec![format!(
                    "serviceAccount:{}",
                    management_service_account_email
                )])
                .build(),
        );

        let new_policy = IamPolicy::builder()
            .version(3)
            .bindings(all_bindings)
            .maybe_etag(current_policy.etag)
            .maybe_kind(current_policy.kind)
            .maybe_resource_id(current_policy.resource_id)
            .build();

        iam_client
            .set_service_account_iam_policy(service_account_email.clone(), new_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to grant impersonation permissions on service account '{}'",
                    service_account_email
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            target_service_account = %service_account_email,
            management_service_account = %management_service_account_email,
            "Impersonation permissions granted successfully"
        );

        self.impersonation_granted = true;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        // Heartbeat check: verify custom role still exists if we have one
        if let Some(role_name) = &self.custom_role_name {
            let role = client.get_role(role_name.clone()).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to get management custom role during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            // Check if role name matches what we expect
            if let Some(fetched_name) = &role.name {
                if fetched_name != role_name {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Management role name changed from {} to {}",
                            role_name, fetched_name
                        ),
                    }));
                }
            }
        }

        // Heartbeat check: verify service account still exists
        if let Some(service_account_email) = &self.service_account_email {
            let sa = client
                .get_service_account(service_account_email.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get management service account during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if service account email matches what we expect
            if let Some(fetched_email) = &sa.email {
                if fetched_email != service_account_email {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Management service account email changed from {} to {}",
                            service_account_email, fetched_email
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

        info!(
            config_id = %config.id,
            "Updating GCP management service account permissions"
        );

        // Ensure per-permission-set custom roles are up-to-date.
        // All permission sets (both provision and non-provision) need custom roles,
        // because resource controllers reference non-provision custom roles when applying
        // resource-level IAM bindings.
        let permission_sets = Self::resolve_management_permission_sets(ctx)?;
        if !permission_sets.is_empty() {
            ResourcePermissionsHelper::ensure_gcp_stack_custom_roles(ctx, &permission_sets).await?;

            info!(
                config_id = %config.id,
                "Management per-permission-set custom roles updated successfully"
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

        // Remove all IAM bindings where our service account is a member
        if !self.role_bound {
            info!(config_id = %config.id, "Role was never bound, skipping unbinding");
        } else if let Some(service_account_email) = &self.service_account_email {
            let gcp_config = ctx.get_gcp_config()?;
            let project_id = &gcp_config.project_id;
            let rm_client = ctx
                .service_provider
                .get_gcp_resource_manager_client(gcp_config)?;

            match rm_client
                .get_project_iam_policy(
                    project_id.clone(),
                    Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                        requested_policy_version: Some(3),
                    }),
                )
                .await
            {
                Ok(mut current_policy) => {
                    let service_account_member =
                        format!("serviceAccount:{}", service_account_email);

                    current_policy
                        .bindings
                        .retain(|binding| !binding.members.contains(&service_account_member));

                    rm_client
                        .set_project_iam_policy(project_id.clone(), current_policy, None)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to unbind roles from management service account '{}' at project level", service_account_email),
                            resource_id: Some(config.id.clone()),
                        })?;

                    info!(
                        config_id = %config.id,
                        service_account_email = %service_account_email,
                        "All role bindings removed for management service account at project level"
                    );
                }
                Err(_) => {
                    info!(
                        config_id = %config.id,
                        service_account_email = %service_account_email,
                        "Could not retrieve project IAM policy for unbinding, continuing with deletion"
                    );
                }
            }

            self.role_bound = false;
        }

        Ok(HandlerAction::Continue {
            state: DeletingRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        if let Some(role_name) = &self.custom_role_name {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

            match client.delete_role(role_name.clone()).await {
                Ok(_) => {
                    info!(config_id = %config.id, role_name = %role_name, "Management custom role deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(config_id = %config.id, role_name = %role_name, "Management custom role already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete management custom role '{}'", role_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }

            self.custom_role_name = None;
            self.role_created = false;
        } else {
            info!(config_id = %config.id, "No management custom role was created, skipping role deletion");
        }

        Ok(HandlerAction::Continue {
            state: DeletingServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingServiceAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        if let Some(service_account_email) = &self.service_account_email {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

            match client
                .delete_service_account(service_account_email.clone())
                .await
            {
                Ok(_) => {
                    info!(config_id = %config.id, email = %service_account_email, "Management service account deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(config_id = %config.id, email = %service_account_email, "Management service account already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete management service account '{}'",
                            service_account_email
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            info!(config_id = %config.id, "No management service account was created, skipping service account deletion");
        }

        self.service_account_email = None;
        self.service_account_unique_id = None;
        self.role_bound = false;
        self.impersonation_granted = false;

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
        if let Some(email) = &self.service_account_email {
            Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: email.clone(),
                access_configuration: email.clone(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl GcpRemoteStackManagementController {
    /// Resolve the management permission sets from the stack's management profile.
    ///
    /// Returns the individual permission sets (not merged into a single synthetic set),
    /// so that per-permission-set custom roles and conditional bindings work correctly.
    fn resolve_management_permission_sets(
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<PermissionSet>> {
        let management_permissions = ctx.desired_stack.management();
        let management_profile = match management_permissions.profile() {
            Some(profile) => profile,
            None => return Ok(Vec::new()),
        };

        let global_permission_set_refs = match management_profile.0.get("*") {
            Some(refs) => refs,
            None => return Ok(Vec::new()),
        };

        let mut permission_sets = Vec::new();

        for permission_set_ref in global_permission_set_refs {
            if let Some(permission_set) =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned())
            {
                permission_sets.push(permission_set);
            }
        }

        Ok(permission_sets)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(service_account_name: &str) -> Self {
        Self {
            state: GcpRemoteStackManagementState::Ready,
            service_account_email: Some(format!(
                "{}@mock-project.iam.gserviceaccount.com",
                service_account_name
            )),
            service_account_unique_id: Some("123456789012345678901".to_string()),
            custom_role_name: None,
            role_created: true,
            role_bound: true,
            impersonation_granted: true,
            _internal_stay_count: None,
        }
    }
}

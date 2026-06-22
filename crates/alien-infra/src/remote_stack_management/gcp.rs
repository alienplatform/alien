use std::time::Duration;
use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::gcp_iam_admin::{get_service_account_iam_policy, set_service_account_iam_policy};
use crate::gcp_resource_manager::{get_project_iam_policy, set_project_iam_policy};
use alien_core::permissions::PermissionSet;
use alien_core::{
    GcpRemoteStackManagementHeartbeatData, HeartbeatBackend, KubernetesCluster, ObservedHealth,
    Platform, ProviderLifecycleState, RemoteStackManagement, RemoteStackManagementHeartbeatData,
    RemoteStackManagementHeartbeatStatus, RemoteStackManagementOutputs, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpIamBinding, GcpRuntimePermissionsGenerator},
    get_permission_set, list_permission_set_ids, BindingTarget, PermissionContext,
};
use chrono::Utc;
use google_cloud_iam_admin_v1::model::{
    CreateServiceAccountRequest, ServiceAccount as GcpServiceAccount,
};
use google_cloud_iam_v1::model::{Binding, GetPolicyOptions, Policy};
use google_cloud_type::model::Expr;

/// Generates the GCP service account ID for RemoteStackManagement.
fn get_gcp_management_service_account_id(prefix: &str) -> String {
    format!("{}-management", prefix)
}

fn is_gcp_not_found<T>(error: &AlienError<T>) -> bool
where
    T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_NOT_FOUND" | "CLOUD_RESOURCE_NOT_FOUND"
    )
}

fn gcp_expr_from_condition(condition: alien_permissions::generators::GcpIamCondition) -> Expr {
    Expr::new()
        .set_expression(condition.expression)
        .set_title(condition.title)
        .set_description(condition.description)
}

fn gcp_binding_from_grant(binding: GcpIamBinding) -> Binding {
    Binding::new()
        .set_role(binding.role)
        .set_members(binding.members)
        .set_or_clear_condition(binding.condition.map(gcp_expr_from_condition))
}

#[controller]
pub struct GcpRemoteStackManagementController {
    /// The email of the created management service account.
    pub(crate) service_account_email: Option<String>,
    /// The unique ID of the created management service account.
    pub(crate) service_account_unique_id: Option<String>,
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

        let display_name = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => format!("{deployment_name}: Management service account"),
            None => "Management service account".to_string(),
        };
        let description = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => format!(
                "Management cloud identity for {deployment_name}. Resource prefix: {}.",
                ctx.resource_prefix
            ),
            None => format!(
                "Management cloud identity. Resource prefix: {}.",
                ctx.resource_prefix
            ),
        };
        let service_account = GcpServiceAccount::new()
            .set_display_name(display_name)
            .set_description(description);

        let request = CreateServiceAccountRequest::new()
            .set_name(format!("projects/{}", gcp_config.project_id))
            .set_account_id(service_account_id.clone())
            .set_service_account(service_account);

        let created_sa = client.create_service_account(request).await.context(
            ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create GCP management service account '{}'",
                    service_account_id
                ),
                resource_id: Some(config.id.clone()),
            },
        )?;

        let email = if created_sa.email.is_empty() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "Created management service account missing email".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        } else {
            created_sa.email
        };

        let unique_id = if created_sa.unique_id.is_empty() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "Created management service account missing unique_id".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        } else {
            created_sa.unique_id
        };

        info!(
            service_account_id = %service_account_id,
            email = %email,
            unique_id = %unique_id,
            "Management service account created successfully"
        );

        self.service_account_email = Some(email);
        self.service_account_unique_id = Some(unique_id);

        Ok(HandlerAction::Continue {
            state: BindingRole,
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
        self.binding_role(ctx).await
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

        // Include all permission sets from the management profile in the project-level
        // IAM bindings. The management profile is curated by
        // ManagementPermissionProfileMutation to include only what the management SA needs.
        let stack_sets = permission_sets;

        info!(
            service_account_email = %service_account_email,
            stack_sets_count = stack_sets.len(),
            "Reconciling management permission-set roles on service account"
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
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let mut new_bindings = Vec::new();

        for permission_set in &stack_sets {
            let grant_plan = generator
                .generate_grant_plan(permission_set, BindingTarget::Stack, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate IAM grant plan for management permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("binding_role".to_string()),
                    resource_id: Some(config.id.clone()),
                })?;
            ResourcePermissionsHelper::ensure_all_gcp_custom_roles(
                ctx,
                &permission_set.id,
                &grant_plan,
            )
            .await?;

            let project_bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
            if project_bindings.is_empty() {
                continue;
            }

            new_bindings.extend(project_bindings.into_iter().map(gcp_binding_from_grant));
        }

        let mut owned_role_prefixes = Self::global_management_role_prefixes(&permission_context);
        Self::append_resource_scoped_management_bindings(
            ctx,
            &generator,
            service_account_id,
            &mut new_bindings,
            &mut owned_role_prefixes,
        )
        .await?;

        let project_id = &gcp_config.project_id;
        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)
            .await?;

        let current_policy = get_project_iam_policy(
            &rm_client,
            project_id,
            Some(GetPolicyOptions::new().set_requested_policy_version(3)),
        )
        .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get project IAM policy before binding management roles. Refusing to proceed to avoid overwriting existing bindings.".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let member = format!("serviceAccount:{service_account_email}");
        let owned_exact_roles = ResourcePermissionsHelper::gcp_predefined_role_names(&new_bindings);
        let mut all_bindings = current_policy.bindings;
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut all_bindings,
            new_bindings,
            &member,
            &owned_role_prefixes,
            &owned_exact_roles,
        );

        if changed {
            let new_policy = Policy::new()
                .set_version(3)
                .set_bindings(all_bindings)
                .set_audit_configs(current_policy.audit_configs)
                .set_etag(current_policy.etag);

            set_project_iam_policy(&rm_client, project_id, new_policy, None)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to bind management roles to service account '{}' at project level",
                        service_account_email
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                service_account_email = %service_account_email,
                "Management permission-set roles reconciled on service account"
            );
        } else {
            info!(
                service_account_email = %service_account_email,
                "Management permission-set role bindings already reconciled"
            );
        }

        self.role_bound = true;

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

        let iam_client = ctx
            .service_provider
            .get_gcp_iam_admin_client(gcp_config)
            .await?;

        // Get current service account IAM policy
        let current_policy = get_service_account_iam_policy(
            &iam_client,
            &gcp_config.project_id,
            service_account_email,
            None,
        )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for service account '{}' before granting impersonation. Refusing to proceed to avoid overwriting existing bindings.", service_account_email),
                resource_id: Some(config.id.clone()),
            })?;

        // Reconcile impersonation bindings for the configured management service account.
        let mut all_bindings = current_policy.bindings;
        let member = format!("serviceAccount:{management_service_account_email}");
        let desired_bindings = vec![
            Binding::new()
                .set_role("roles/iam.serviceAccountTokenCreator")
                .set_members([member.clone()]),
            Binding::new()
                .set_role("roles/iam.serviceAccountUser")
                .set_members([member.clone()]),
        ];
        let owned_exact_roles =
            ResourcePermissionsHelper::gcp_predefined_role_names(&desired_bindings);
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut all_bindings,
            desired_bindings,
            &member,
            &[],
            &owned_exact_roles,
        );

        if !changed {
            info!(
                target_service_account = %service_account_email,
                management_service_account = %management_service_account_email,
                "Impersonation permissions already reconciled"
            );
            self.impersonation_granted = true;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let new_policy = Policy::new()
            .set_version(3)
            .set_bindings(all_bindings)
            .set_audit_configs(current_policy.audit_configs)
            .set_etag(current_policy.etag);

        set_service_account_iam_policy(
            &iam_client,
            &gcp_config.project_id,
            service_account_email,
            new_policy,
        )
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
            if !sa.email.is_empty() && &sa.email != service_account_email {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "Management service account email changed from {} to {}",
                        service_account_email, sa.email
                    ),
                }));
            }
        }

        emit_gcp_remote_stack_management_heartbeat(ctx, self);

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

        self.binding_role(ctx).await
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
                .get_gcp_resource_manager_client(gcp_config)
                .await?;

            match get_project_iam_policy(
                &rm_client,
                project_id,
                Some(GetPolicyOptions::new().set_requested_policy_version(3)),
            )
            .await
            {
                Ok(mut current_policy) => {
                    let service_account_member =
                        format!("serviceAccount:{}", service_account_email);

                    ResourcePermissionsHelper::remove_gcp_project_member_bindings(
                        &mut current_policy.bindings,
                        &service_account_member,
                        None,
                        None,
                    );

                    set_project_iam_policy(&rm_client, project_id, current_policy, None)
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
        let permission_context =
            ResourcePermissionsHelper::build_gcp_permission_context(ctx, ctx.resource_prefix)?;

        ResourcePermissionsHelper::delete_gcp_custom_roles(ctx, &permission_context).await?;

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
                Err(e) if is_gcp_not_found(&e) => {
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

fn emit_gcp_remote_stack_management_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    controller: &GcpRemoteStackManagementController,
) {
    let resource_id = ctx
        .desired_resource_config::<RemoteStackManagement>()
        .map(|config| config.id.clone())
        .unwrap_or_else(|_| "remote-stack-management".to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id,
        resource_type: RemoteStackManagement::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::RemoteStackManagement(
            RemoteStackManagementHeartbeatData::GcpServiceAccount(
                GcpRemoteStackManagementHeartbeatData {
                    status: RemoteStackManagementHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: controller.service_account_email.as_ref().map(|email| {
                            format!("GCP management service account '{}' is reachable", email)
                        }),
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    service_account_email: controller.service_account_email.clone(),
                    service_account_unique_id: controller.service_account_unique_id.clone(),
                    role_bound: controller.role_bound,
                    impersonation_granted: controller.impersonation_granted,
                },
            ),
        ),
        raw: vec![],
    });
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

    fn global_management_role_prefixes(permission_context: &PermissionContext) -> Vec<String> {
        ResourcePermissionsHelper::gcp_permission_set_custom_role_name_prefixes(
            permission_context,
            list_permission_set_ids()
                .into_iter()
                // GCP vault management/data grants are reconciled by the vault
                // controller because they need resource-specific IAM conditions.
                .filter(|id| !id.starts_with("vault/")),
        )
    }

    async fn append_resource_scoped_management_bindings(
        ctx: &ResourceControllerContext<'_>,
        generator: &GcpRuntimePermissionsGenerator,
        service_account_id: &str,
        new_bindings: &mut Vec<Binding>,
        owned_role_prefixes: &mut Vec<String>,
    ) -> Result<()> {
        let Some(management_profile) = ctx.desired_stack.management().profile() else {
            return Ok(());
        };

        for (resource_id, permission_set_refs) in management_profile
            .0
            .iter()
            .filter(|(scope, _)| scope.as_str() != "*")
        {
            let Some(resource_entry) = ctx.desired_stack.resources.get(resource_id) else {
                continue;
            };
            let Some(cluster) = resource_entry.config.downcast_ref::<KubernetesCluster>() else {
                continue;
            };
            let permission_context =
                ResourcePermissionsHelper::gcp_kubernetes_cluster_permission_context(
                    ctx,
                    cluster,
                    Some(service_account_id),
                )?;

            for permission_set_ref in permission_set_refs {
                let Some(permission_set) =
                    permission_set_ref.resolve(|name| get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.gcp.is_none() {
                    continue;
                }

                let grant_plan = generator
                    .generate_grant_plan(
                        &permission_set,
                        BindingTarget::Resource,
                        &permission_context,
                    )
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate resource-scoped IAM grant plan for management permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("binding_role".to_string()),
                        resource_id: Some(resource_id.clone()),
                    })?;

                let project_bindings =
                    grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
                ResourcePermissionsHelper::ensure_all_gcp_custom_roles(
                    ctx,
                    &permission_set.id,
                    &grant_plan,
                )
                .await?;

                if project_bindings.is_empty() {
                    continue;
                }
                owned_role_prefixes.extend(
                    ResourcePermissionsHelper::gcp_permission_set_custom_role_name_prefixes(
                        &permission_context,
                        std::iter::once(permission_set.id.as_str()),
                    ),
                );

                for binding in project_bindings {
                    new_bindings.push(gcp_binding_from_grant(binding));
                }
            }
        }

        Ok(())
    }

    #[cfg(test)]
    fn project_management_bindings(bindings: Vec<GcpIamBinding>) -> Vec<GcpIamBinding> {
        bindings
            .into_iter()
            .filter(|binding| binding.target == GcpBindingTargetScope::Project)
            .collect()
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
            role_bound: true,
            impersonation_granted: true,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_permission_context() -> PermissionContext {
        PermissionContext::new()
            .with_stack_prefix("test-stack".to_string())
            .with_deployment_name("Test Deployment".to_string())
            .with_project_name("test-project".to_string())
            .with_region("us-central1".to_string())
            .with_project_number("123456789012".to_string())
    }

    #[test]
    fn project_management_bindings_skip_resource_scoped_artifact_registry_heartbeat() {
        let permission_set = get_permission_set("artifact-registry/heartbeat").unwrap();
        let generator = GcpRuntimePermissionsGenerator::new();
        let bindings = generator
            .generate_bindings(
                permission_set,
                BindingTarget::Resource,
                &test_permission_context(),
            )
            .unwrap();

        assert!(
            GcpRemoteStackManagementController::project_management_bindings(bindings.bindings)
                .is_empty()
        );
    }

    #[test]
    fn project_management_bindings_keep_project_scoped_storage_heartbeat() {
        let permission_set = get_permission_set("storage/heartbeat").unwrap();
        let generator = GcpRuntimePermissionsGenerator::new();
        let bindings = generator
            .generate_bindings(
                permission_set,
                BindingTarget::Stack,
                &test_permission_context(),
            )
            .unwrap();

        let project_bindings =
            GcpRemoteStackManagementController::project_management_bindings(bindings.bindings);

        assert_eq!(project_bindings.len(), 1);
        assert_eq!(project_bindings[0].target, GcpBindingTargetScope::Project);
    }
}

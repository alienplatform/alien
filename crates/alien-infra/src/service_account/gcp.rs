use std::time::Duration;
use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::iam::{
    Binding, CreateServiceAccountRequest, IamPolicy, ServiceAccount as GcpServiceAccount,
};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};

/// Generates the GCP service account ID from the ServiceAccount name.
fn get_gcp_service_account_id(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpServiceAccountController {
    /// The email of the created service account.
    pub service_account_email: Option<String>,
    /// The unique ID of the created service account.
    pub(crate) service_account_unique_id: Option<String>,
}

#[controller]
impl GcpServiceAccountController {
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        let service_account_id = get_gcp_service_account_id(ctx.resource_prefix, &config.id);

        info!(
            service_account_id = %service_account_id,
            config_id = %config.id,
            "Creating GCP service account"
        );

        let service_account = GcpServiceAccount::builder()
            .display_name(format!("Alien ServiceAccount: {}", config.id))
            .description(format!(
                "Service account for Alien ServiceAccount {}",
                config.id
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
                    "Failed to create GCP service account '{}'",
                    service_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let email = created_sa.email.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created service account missing email".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let unique_id = created_sa.unique_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Created service account missing unique_id".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            service_account_id = %service_account_id,
            email = %email,
            unique_id = %unique_id,
            "Service account created successfully"
        );

        self.service_account_email = Some(email);
        self.service_account_unique_id = Some(unique_id);

        Ok(HandlerAction::Continue {
            state: BindingStackRoles,
            suggested_delay: None,
        })
    }

    #[handler(
        state = BindingStackRoles,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn binding_stack_roles(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.sync_stack_role_bindings(ctx).await?;

        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_resource_permissions_to_service_account(ctx)
            .await?;

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Heartbeat check: verify service account still exists
        if let Some(service_account_email) = &self.service_account_email {
            let sa = client
                .get_service_account(service_account_email.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get service account during heartbeat check".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if service account email matches what we expect
            if let Some(fetched_email) = &sa.email {
                if fetched_email != service_account_email {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Service account email changed from {} to {}",
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            "Updating GCP service account permissions"
        );

        self.sync_stack_role_bindings(ctx).await?;
        self.apply_resource_permissions_to_service_account(ctx)
            .await?;

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            "Starting deletion of GCP service account resources"
        );

        if let Some(service_account_email) = &self.service_account_email {
            self.remove_project_iam_bindings_for_service_account(ctx, service_account_email)
                .await?;
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        if let Some(service_account_email) = &self.service_account_email {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

            match client
                .delete_service_account(service_account_email.clone())
                .await
            {
                Ok(_) => {
                    info!(config_id = %config.id, email = %service_account_email, "Service account deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(config_id = %config.id, email = %service_account_email, "Service account already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete service account '{}'",
                            service_account_email
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        } else {
            info!(config_id = %config.id, "No service account was created, skipping service account deletion");
        }

        self.service_account_email = None;
        self.service_account_unique_id = None;

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
            // For service account-based roles, use the email as identifier and extract name
            let role_name = email.split('@').next().unwrap_or("unknown").to_string();

            Some(ResourceOutputs::new(ServiceAccountOutputs {
                identity: email.clone(),
                resource_id: role_name,
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        if let (Some(email), Some(unique_id)) =
            (&self.service_account_email, &self.service_account_unique_id)
        {
            let binding = ServiceAccountBinding::gcp_service_account(
                BindingValue::Value(email.clone()),
                BindingValue::Value(unique_id.clone()),
            );
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

// Separate impl block for helper methods
impl GcpServiceAccountController {
    async fn sync_stack_role_bindings(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        let service_account_email = self.service_account_email.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Service account email not available for stack role binding".to_string(),
                operation: Some("sync_stack_role_bindings".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            config_id = %config.id,
            service_account_email = %service_account_email,
            permission_sets_count = config.stack_permission_sets.len(),
            "Binding stack-level permission-set roles to service account at project level"
        );

        let generator = GcpRuntimePermissionsGenerator::new();
        let gcp_config = ctx.get_gcp_config()?;

        let service_account_id = service_account_email
            .split('@')
            .next()
            .unwrap_or(service_account_email);

        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_stack_name(ctx.desired_stack.id().to_string())
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_service_account_name(service_account_id.to_string());
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let mut new_bindings = Vec::new();

        for permission_set in &config.stack_permission_sets {
            ResourcePermissionsHelper::ensure_single_gcp_custom_role(
                ctx,
                permission_set,
                &permission_context,
            )
            .await?;

            let bindings = generator
                .generate_bindings(permission_set, BindingTarget::Stack, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate IAM bindings for stack permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("sync_stack_role_bindings".to_string()),
                    resource_id: Some(config.id.clone()),
                })?;

            for binding in bindings.bindings {
                if binding.target != GcpBindingTargetScope::Project {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "GCP stack permission set '{}' produced a {:?} IAM binding where Project was required",
                            permission_set.id, binding.target
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }

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

        let project_id = &gcp_config.project_id;
        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)?;

        let current_policy = rm_client
            .get_project_iam_policy(
                project_id.clone(),
                Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                    requested_policy_version: Some(3),
                }),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get project IAM policy before binding stack roles".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let member = format!("serviceAccount:{service_account_email}");
        let owned_role_prefixes =
            vec![ResourcePermissionsHelper::gcp_stack_custom_role_name_prefix(&permission_context)];
        let mut all_bindings = current_policy.bindings;
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut all_bindings,
            new_bindings,
            &member,
            &owned_role_prefixes,
        );

        if !changed {
            info!(
                config_id = %config.id,
                service_account_email = %service_account_email,
                "Project-level permission-set role bindings already reconciled"
            );
            return Ok(());
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
                message: format!(
                    "Failed to bind stack roles to service account '{}' at project level",
                    service_account_email
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            config_id = %config.id,
            service_account_email = %service_account_email,
            "Stack-level permission-set roles bound at project level"
        );

        Ok(())
    }

    async fn apply_resource_permissions_to_service_account(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            "Applying resource-level IAM on service account"
        );

        // Apply resource-scoped permissions (e.g., service-account/impersonate) via
        // SA-level IAM. On GCP the only way to scope iam.serviceAccounts.getAccessToken
        // is to grant tokenCreator directly on the target service account resource,
        // not at the project level.
        if let Some(service_account_email) = &self.service_account_email {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
            let sa_email_owned = service_account_email.clone();
            let config_id_owned = config.id.clone();

            ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "Service account",
                "service-account",
                client,
                |client, iam_policy| async move {
                    client
                        .set_service_account_iam_policy(sa_email_owned.clone(), iam_policy)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to apply IAM policy to service account '{}'",
                                sa_email_owned
                            ),
                            resource_id: Some(config_id_owned),
                        })?;
                    info!(
                        service_account = %sa_email_owned,
                        "Applied resource-level IAM policy to service account"
                    );
                    Ok(())
                },
            )
            .await?;
        }

        Ok(())
    }

    async fn remove_project_iam_bindings_for_service_account(
        &self,
        ctx: &ResourceControllerContext<'_>,
        service_account_email: &str,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let project_id = &gcp_config.project_id;
        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)?;

        let mut current_policy = rm_client
            .get_project_iam_policy(
                project_id.clone(),
                Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                    requested_policy_version: Some(3),
                }),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get project IAM policy before unbinding stack roles"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let mut changed = false;

        let member = format!("serviceAccount:{service_account_email}");
        changed |= ResourcePermissionsHelper::remove_gcp_project_member_bindings(
            &mut current_policy.bindings,
            &member,
            None,
        );

        if !changed {
            info!(
                config_id = %config.id,
                service_account_email = %service_account_email,
                "No project-level IAM bindings to remove for service account"
            );
            return Ok(());
        }

        rm_client
            .set_project_iam_policy(project_id.clone(), current_policy, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to unbind stack roles from service account '{}' at project level",
                    service_account_email
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            config_id = %config.id,
            service_account_email = %service_account_email,
            "Project-level IAM bindings removed for service account"
        );

        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(role_name: &str) -> Self {
        Self {
            state: GcpServiceAccountState::Ready,
            service_account_email: Some(format!(
                "{}@mock-project.iam.gserviceaccount.com",
                role_name
            )),
            service_account_unique_id: Some("123456789012345678901".to_string()),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcp_service_account_id_within_limit() {
        // With the "-sa" suffix from the mutation, typical IDs are well under 30 chars:
        // prefix (8) + "-" (1) + "execution-sa" (12) = 21
        let id = get_gcp_service_account_id("nd3ef88e", "execution-sa");
        assert_eq!(id, "nd3ef88e-execution-sa");
        assert!(id.len() <= 30);

        let id = get_gcp_service_account_id("nd3ef88e", "runtime-sa");
        assert_eq!(id, "nd3ef88e-runtime-sa");
        assert!(id.len() <= 30);
    }
}

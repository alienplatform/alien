use std::time::Duration;
use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::iam::{CreateServiceAccountRequest, ServiceAccount as GcpServiceAccount};
use alien_macros::{controller, flow_entry, handler, terminal_state};

/// Generates the GCP service account ID from the ServiceAccount name.
fn get_gcp_service_account_id(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpServiceAccountController {
    /// The email of the created service account.
    pub(crate) service_account_email: Option<String>,
    /// The unique ID of the created service account.
    pub(crate) service_account_unique_id: Option<String>,
    /// The name/ID of the created custom role.
    pub(crate) custom_role_name: Option<String>,
    /// Whether the custom role has been created.
    pub(crate) role_created: bool,
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        if config.stack_permission_sets.is_empty() {
            info!(
                config_id = %config.id,
                "No stack-level permissions to create custom roles for"
            );
        } else {
            info!(
                config_id = %config.id,
                permission_sets_count = config.stack_permission_sets.len(),
                "Ensuring per-permission-set custom roles exist"
            );

            ResourcePermissionsHelper::ensure_gcp_stack_custom_roles(
                ctx,
                &config.stack_permission_sets,
            )
            .await?;

            self.role_created = true;
        }

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

        // Heartbeat check: verify role still exists if we have one
        if let Some(role_name) = &self.custom_role_name {
            let role = client.get_role(role_name.clone()).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to get custom role during heartbeat check".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            // Check if role name matches what we expect
            if let Some(fetched_name) = &role.name {
                if fetched_name != role_name {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Role name changed from {} to {}",
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

        // Ensure per-permission-set custom roles are up-to-date
        if !config.stack_permission_sets.is_empty() {
            ResourcePermissionsHelper::ensure_gcp_stack_custom_roles(
                ctx,
                &config.stack_permission_sets,
            )
            .await?;

            info!(
                config_id = %config.id,
                "Per-permission-set custom roles updated successfully"
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            "Starting deletion of GCP service account resources"
        );

        // No project-level IAM policy unbinding needed — permissions are now
        // applied at the resource level by individual resource controllers.

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        if let Some(role_name) = &self.custom_role_name {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

            match client.delete_role(role_name.clone()).await {
                Ok(_) => {
                    info!(config_id = %config.id, role_name = %role_name, "Custom role deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(config_id = %config.id, role_name = %role_name, "Custom role already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete custom role '{}'", role_name),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }

            self.custom_role_name = None;
            self.role_created = false;
        } else {
            info!(config_id = %config.id, "No custom role was created, skipping role deletion");
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
        // Only return outputs when we have either a service account email or role name
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
            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
}

// Separate impl block for helper methods
impl GcpServiceAccountController {
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
            custom_role_name: None,
            role_created: true,
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

        let id = get_gcp_service_account_id("nd3ef88e", "deepstore-sa");
        assert_eq!(id, "nd3ef88e-deepstore-sa");
        assert!(id.len() <= 30);
    }
}

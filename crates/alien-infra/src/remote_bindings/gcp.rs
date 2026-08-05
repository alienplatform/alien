use std::time::Duration;

use alien_core::{RemoteBindings, RemoteBindingsOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::iam::{Binding, CreateServiceAccountRequest, IamPolicy, ServiceAccount};
use alien_macros::controller;
use sha2::{Digest, Sha256};

use crate::{
    core::{ResourceControllerContext, ResourcePermissionsHelper},
    error::{ErrorData, Result},
};

#[controller]
pub struct GcpRemoteBindingsController {
    pub(crate) service_account_email: Option<String>,
    pub(crate) service_account_unique_id: Option<String>,
}

#[controller]
impl GcpRemoteBindingsController {
    #[flow_entry(Create)]
    #[handler(state = CreatingServiceAccount, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteBindings>()?;
        let account_id = service_account_id(ctx.resource_prefix);
        let created = ctx.service_provider.get_gcp_iam_client(ctx.get_gcp_config()?)?
            .create_service_account(account_id, CreateServiceAccountRequest::builder()
                .service_account(ServiceAccount::builder()
                    .display_name("Application access service account".to_string())
                    .description(format!("Data-plane identity for explicitly published resources. Resource prefix: {}.", ctx.resource_prefix))
                    .build())
                .build())
            .await.context(ErrorData::CloudPlatformError {
                message: "Failed to create Remote Bindings service account".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        self.service_account_email = created.email;
        self.service_account_unique_id = created.unique_id;
        if self.service_account_email.is_none() || self.service_account_unique_id.is_none() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "Created Remote Bindings service account is missing identifiers"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
        Ok(HandlerAction::Continue {
            state: GrantingImpersonation,
            suggested_delay: None,
        })
    }

    #[handler(state = GrantingImpersonation, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn granting_impersonation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        reconcile_impersonation(self, ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        reconcile_impersonation(self, ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if let Some(email) = self.service_account_email.as_ref() {
            let client = ctx
                .service_provider
                .get_gcp_iam_client(ctx.get_gcp_config()?)?;
            match client.delete_service_account(email.clone()).await {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to delete Remote Bindings service account".to_string(),
                        resource_id: Some(
                            ctx.desired_resource_config::<RemoteBindings>()?.id.clone(),
                        ),
                    }))
                }
            }
        }
        self.service_account_email = None;
        self.service_account_unique_id = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let email = self.service_account_email.clone()?;
        Some(ResourceOutputs::new(RemoteBindingsOutputs {
            resource_id: email.clone(),
            access_configuration: email,
        }))
    }
}

async fn reconcile_impersonation(
    controller: &GcpRemoteBindingsController,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    let config = ctx.desired_resource_config::<RemoteBindings>()?;
    let email = controller
        .service_account_email
        .clone()
        .ok_or_else(|| missing_identity(ctx))?;
    let management = ctx.get_gcp_management_config()?.ok_or_else(|| {
        AlienError::new(ErrorData::InfrastructureError {
            message: "GCP management configuration is required for Remote Bindings".to_string(),
            operation: Some("grant_remote_bindings_impersonation".to_string()),
            resource_id: Some(config.id.clone()),
        })
    })?;
    let client = ctx
        .service_provider
        .get_gcp_iam_client(ctx.get_gcp_config()?)?;
    let current = client
        .get_service_account_iam_policy(email.clone())
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to read Remote Bindings service-account IAM policy".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    let member = format!("serviceAccount:{}", management.service_account_email);
    let desired = vec![
        Binding::builder()
            .role("roles/iam.serviceAccountTokenCreator".to_string())
            .members(vec![member.clone()])
            .build(),
        Binding::builder()
            .role("roles/iam.serviceAccountUser".to_string())
            .members(vec![member.clone()])
            .build(),
    ];
    let mut bindings = current.bindings;
    let owned_roles = ResourcePermissionsHelper::gcp_predefined_role_names(&desired);
    if ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
        &mut bindings,
        desired,
        &member,
        &[],
        &owned_roles,
    ) {
        client
            .set_service_account_iam_policy(
                email,
                IamPolicy::builder()
                    .version(3)
                    .bindings(bindings)
                    .maybe_etag(current.etag)
                    .maybe_kind(current.kind)
                    .maybe_resource_id(current.resource_id)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to grant Remote Bindings impersonation".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
    }
    Ok(())
}

fn missing_identity(ctx: &ResourceControllerContext<'_>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InfrastructureError {
        message: "Remote Bindings service-account identity is missing".to_string(),
        operation: Some("remote_bindings_identity".to_string()),
        resource_id: ctx
            .desired_resource_config::<RemoteBindings>()
            .ok()
            .map(|config| config.id.clone()),
    })
}

/// GCP service-account IDs are limited to 30 characters. Keep short prefixes readable and
/// make longer customer-provided prefixes deterministic without risking collisions.
fn service_account_id(prefix: &str) -> String {
    let raw = format!("{prefix}-access");
    if raw.len() <= 30 {
        return raw;
    }

    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let stem = prefix.chars().take(21).collect::<String>();
    format!("{}-{}", stem.trim_end_matches('-'), &digest[..8])
}

#[cfg(test)]
mod tests {
    use super::service_account_id;

    #[test]
    fn service_account_ids_respect_gcp_limits_and_are_stable() {
        assert_eq!(service_account_id("acme"), "acme-access");
        let long = service_account_id("customer-provided-resource-prefix");
        assert!(long.len() <= 30);
        assert_eq!(
            long,
            service_account_id("customer-provided-resource-prefix")
        );
        assert_ne!(
            long,
            service_account_id("customer-provided-resource-prefix-2")
        );
    }
}

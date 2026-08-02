use std::{collections::HashMap, time::Duration};

use alien_azure_clients::{
    managed_identity::{FederatedCredentialProperties, FederatedIdentityCredential},
    models::managed_identity::Identity,
};
use alien_core::{RemoteBindings, RemoteBindingsOutputs, ResourceOutputs, ResourceStatus};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    infra_requirements::azure_utils,
};

#[controller]
pub struct AzureRemoteBindingsController {
    pub(crate) identity_resource_id: Option<String>,
    pub(crate) client_id: Option<String>,
    pub(crate) principal_id: Option<String>,
    pub(crate) tenant_id: Option<String>,
    pub(crate) fic_name: Option<String>,
}

#[controller]
impl AzureRemoteBindingsController {
    #[flow_entry(Create)]
    #[handler(state = CreatingManagedIdentity, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteBindings>()?;
        let azure = ctx.get_azure_config()?;
        let identity = Identity {
            id: None,
            location: azure.region.clone().unwrap_or_else(|| "eastus".to_string()),
            name: None,
            properties: None,
            system_data: None,
            tags: HashMap::new(),
            type_: None,
        };
        let created = ctx
            .service_provider
            .get_azure_managed_identity_client(azure)?
            .create_or_update_user_assigned_identity(
                &azure_utils::get_resource_group_name(ctx.state)?,
                &identity_name(ctx.resource_prefix),
                &identity,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Remote Bindings managed identity".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        let properties = created
            .properties
            .ok_or_else(|| missing(config.id.clone(), "created identity properties"))?;
        self.identity_resource_id = Some(
            created
                .id
                .ok_or_else(|| missing(config.id.clone(), "created identity ID"))?,
        );
        self.client_id = Some(
            properties
                .client_id
                .ok_or_else(|| missing(config.id.clone(), "created identity client ID"))?
                .to_string(),
        );
        self.principal_id = Some(
            properties
                .principal_id
                .ok_or_else(|| missing(config.id.clone(), "created identity principal ID"))?
                .to_string(),
        );
        self.tenant_id = Some(azure.tenant_id.clone());
        Ok(HandlerAction::Continue {
            state: CreatingFederatedCredential,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(state = CreatingFederatedCredential, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_federated_credential(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        reconcile_federated_credential(self, ctx).await?;
        Ok(HandlerAction::Continue {
            state: WaitingForFederatedCredentialPropagation,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(state = WaitingForFederatedCredentialPropagation, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn waiting_for_federated_credential_propagation(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        ctx.service_provider
            .get_azure_managed_identity_client(ctx.get_azure_config()?)?
            .get_user_assigned_identity(
                &azure_utils::get_resource_group_name(ctx.state)?,
                &identity_name(ctx.resource_prefix),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read Remote Bindings managed identity".to_string(),
                resource_id: Some(ctx.desired_resource_config::<RemoteBindings>()?.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        reconcile_federated_credential(self, ctx).await?;
        Ok(HandlerAction::Continue {
            state: WaitingForFederatedCredentialUpdatePropagation,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(state = WaitingForFederatedCredentialUpdatePropagation, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn waiting_for_federated_credential_update_propagation(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_group = azure_utils::get_resource_group_name(ctx.state)?;
        let identity_name = identity_name(ctx.resource_prefix);
        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(ctx.get_azure_config()?)?;
        if let Some(fic_name) = self.fic_name.as_ref() {
            ignore_not_found(
                client
                    .delete_federated_credential(&resource_group, &identity_name, fic_name)
                    .await,
            )?;
        }
        ignore_not_found(
            client
                .delete_user_assigned_identity(&resource_group, &identity_name)
                .await,
        )?;
        self.identity_resource_id = None;
        self.client_id = None;
        self.principal_id = None;
        self.tenant_id = None;
        self.fic_name = None;
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
        let identity_id = self.identity_resource_id.clone()?;
        Some(ResourceOutputs::new(RemoteBindingsOutputs {
            resource_id: identity_id,
            access_configuration: serde_json::json!({
                "uamiClientId": self.client_id.as_ref()?, "tenantId": self.tenant_id.as_ref()?,
            })
            .to_string(),
            external_id: None,
        }))
    }
}

async fn reconcile_federated_credential(
    controller: &mut AzureRemoteBindingsController,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    let config = ctx.desired_resource_config::<RemoteBindings>()?;
    let management = ctx.get_azure_management_config()?.ok_or_else(|| {
        AlienError::new(ErrorData::InfrastructureError {
            message: "Azure management configuration is required for Remote Bindings".to_string(),
            operation: Some("create_remote_bindings_federated_credential".to_string()),
            resource_id: Some(config.id.clone()),
        })
    })?;
    let fic_name = format!(
        "{}-remote-bindings-federated-credential",
        ctx.resource_prefix
    );
    let credential = FederatedIdentityCredential {
        id: None,
        name: None,
        type_: None,
        properties: Some(FederatedCredentialProperties {
            issuer: management.oidc_issuer.clone(),
            subject: management.oidc_subject.clone(),
            audiences: vec!["api://AzureADTokenExchange".to_string()],
        }),
    };
    ctx.service_provider
        .get_azure_managed_identity_client(ctx.get_azure_config()?)?
        .create_or_update_federated_credential(
            &azure_utils::get_resource_group_name(ctx.state)?,
            &identity_name(ctx.resource_prefix),
            &fic_name,
            &credential,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create Remote Bindings federated credential".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    controller.fic_name = Some(fic_name);
    Ok(())
}

fn identity_name(prefix: &str) -> String {
    format!("{prefix}-remote-bindings-identity")
}
fn missing(resource_id: String, field: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InfrastructureError {
        message: format!("Remote Bindings {field} is missing"),
        operation: Some("create_remote_bindings_identity".to_string()),
        resource_id: Some(resource_id),
    })
}
fn ignore_not_found<T>(
    result: std::result::Result<T, alien_error::AlienError<alien_client_core::ErrorData>>,
) -> Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: "Failed to delete Remote Bindings Azure identity resource".to_string(),
            resource_id: Some("remote-bindings".to_string()),
        })),
    }
}

use std::time::Duration;
use tracing::info;
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::managed_identity::{
    FederatedCredentialProperties, FederatedIdentityCredential,
};
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_azure_clients::models::managed_identity::Identity;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use std::collections::HashMap;

fn get_management_identity_name(prefix: &str) -> String {
    format!("{}-management-identity", prefix)
}

fn get_fic_name(prefix: &str) -> String {
    format!("{}-alien-fic", prefix)
}

#[controller]
pub struct AzureRemoteStackManagementController {
    /// The resource ID of the target UAMI
    pub(crate) uami_resource_id: Option<String>,
    /// The client ID of the target UAMI (used in access_configuration output)
    pub(crate) uami_client_id: Option<String>,
    /// The principal ID (object ID) of the target UAMI (used for role assignment)
    pub(crate) uami_principal_id: Option<String>,
    /// The customer's tenant ID (stored for build_outputs)
    pub(crate) tenant_id: Option<String>,
    /// The name of the FIC (None if OIDC not configured)
    pub(crate) fic_name: Option<String>,
    /// The full resource ID of the custom role definition
    pub(crate) role_definition_id: Option<String>,
    /// Resource IDs of created role assignments
    pub(crate) role_assignment_ids: Vec<String>,
}

#[controller]
impl AzureRemoteStackManagementController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingManagedIdentity,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let identity_name = get_management_identity_name(ctx.resource_prefix);

        let azure_cfg = ctx.get_azure_config()?;
        let location = azure_cfg.region.as_deref().unwrap_or("eastus");

        let identity = Identity {
            id: None,
            location: location.to_string(),
            name: None,
            properties: None,
            system_data: None,
            tags: HashMap::new(),
            type_: None,
        };

        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        let created = client
            .create_or_update_user_assigned_identity(
                &resource_group_name,
                &identity_name,
                &identity,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create management identity '{}'",
                    identity_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let identity_id = created.id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created management identity missing ID".to_string(),
                operation: Some("create_management_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let properties = created.properties.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created management identity missing properties".to_string(),
                operation: Some("create_management_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let principal_id = properties.principal_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created management identity missing principal ID".to_string(),
                operation: Some("create_management_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let client_id = properties.client_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created management identity missing client ID".to_string(),
                operation: Some("create_management_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            identity_name = %identity_name,
            identity_id = %identity_id,
            principal_id = %principal_id,
            client_id = %client_id,
            "Management identity created"
        );

        self.uami_resource_id = Some(identity_id);
        self.uami_client_id = Some(client_id.to_string());
        self.uami_principal_id = Some(principal_id.to_string());
        self.tenant_id = Some(azure_cfg.tenant_id.clone());

        Ok(HandlerAction::Continue {
            state: CreatingFederatedCredential,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = CreatingFederatedCredential,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_federated_credential(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_management = ctx.get_azure_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration is required for RemoteStackManagement"
                    .to_string(),
                operation: Some("create_federated_credential".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let oidc_issuer = match &azure_management.oidc_issuer {
            Some(issuer) => issuer,
            None => {
                info!("No OIDC issuer configured, skipping FIC creation (SP fallback mode)");
                return Ok(HandlerAction::Continue {
                    state: CreatingRoleDefinition,
                    suggested_delay: None,
                });
            }
        };

        let oidc_subject = azure_management.oidc_subject.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "OIDC subject is required when OIDC issuer is set".to_string(),
                operation: Some("create_federated_credential".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let identity_name = get_management_identity_name(ctx.resource_prefix);
        let fic_name = get_fic_name(ctx.resource_prefix);

        let credential = FederatedIdentityCredential {
            id: None,
            name: None,
            type_: None,
            properties: Some(FederatedCredentialProperties {
                issuer: oidc_issuer.clone(),
                subject: oidc_subject.clone(),
                audiences: vec!["api://AzureADTokenExchange".to_string()],
            }),
        };

        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        client
            .create_or_update_federated_credential(
                &resource_group_name,
                &identity_name,
                &fic_name,
                &credential,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create federated identity credential '{}' on identity '{}'",
                    fic_name, identity_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            fic_name = %fic_name,
            issuer = %oidc_issuer,
            subject = %oidc_subject,
            "Federated identity credential created"
        );

        self.fic_name = Some(fic_name);

        Ok(HandlerAction::Continue {
            state: CreatingRoleDefinition,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRoleDefinition,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let role_definition_props = self.generate_management_role_definition(ctx)?;

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        let role_definition_uuid = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("alien:azure:mgmt-role-def:{}", ctx.resource_prefix).as_bytes(),
        )
        .to_string();

        let role_definition = RoleDefinition {
            properties: Some(role_definition_props),
            ..Default::default()
        };

        let created = client
            .create_or_update_role_definition(&scope, role_definition_uuid.clone(), &role_definition)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create management role definition for stack '{}'",
                    ctx.resource_prefix
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let role_id = created.id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created role definition missing ID".to_string(),
                operation: Some("create_management_role_definition".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            role_definition_id = %role_id,
            "Management role definition created"
        );

        self.role_definition_id = Some(role_id);

        Ok(HandlerAction::Continue {
            state: CreatingRoleAssignments,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRoleAssignments,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        let role_definition_id = self
            .role_definition_id
            .clone()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "Role definition ID not available for role assignments".to_string(),
                    operation: Some("create_role_assignments".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let uami_principal_id = self
            .uami_principal_id
            .clone()
            .ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "UAMI principal ID not available for role assignments".to_string(),
                    operation: Some("create_role_assignments".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // 1. Assign custom role to target UAMI (always)
        let uami_assignment_id = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "alien:azure:mgmt-role-assign:{}:uami",
                ctx.resource_prefix
            )
            .as_bytes(),
        )
        .to_string();

        self.create_role_assignment_helper(
            &client,
            &scope,
            &uami_assignment_id,
            &uami_principal_id,
            &role_definition_id,
            &format!("management UAMI for stack {}", ctx.resource_prefix),
            &config.id,
        )
        .await?;

        // 2. Assign custom role to SP principal (local dev fallback)
        let azure_management = ctx.get_azure_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration required".to_string(),
                operation: Some("create_role_assignments".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        if let Some(sp_principal_id) = &azure_management.management_principal_id {
            let sp_assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "alien:azure:mgmt-role-assign:{}:sp",
                    ctx.resource_prefix
                )
                .as_bytes(),
            )
            .to_string();

            self.create_role_assignment_helper(
                &client,
                &scope,
                &sp_assignment_id,
                sp_principal_id,
                &role_definition_id,
                &format!("management SP for stack {}", ctx.resource_prefix),
                &config.id,
            )
            .await?;
        }

        // 3. Assign ACR built-in roles to UAMI if needed
        let management_permissions = ctx.desired_stack.management();
        if let Some(profile) = management_permissions.profile() {
            if let Some(global_refs) = profile.0.get("*") {
                let needs_acr = global_refs.iter().any(|r| {
                    let id = r.id();
                    id.starts_with("artifact-registry/")
                        || id.starts_with("function/")
                        || id.starts_with("container-cluster/")
                });

                if needs_acr {
                    let subscription_scope = Scope::Subscription;
                    let acr_push_role_id = "8311e382-0749-4cb8-b61a-304f252e45ec";
                    let full_acr_role_def_id = format!(
                        "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                        azure_cfg.subscription_id, acr_push_role_id
                    );

                    let acr_assignment_id = Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!(
                            "alien:azure:mgmt-acr-assign:{}",
                            ctx.resource_prefix
                        )
                        .as_bytes(),
                    )
                    .to_string();

                    let full_assignment_id = client.build_role_assignment_id(
                        &subscription_scope,
                        acr_assignment_id,
                    );

                    let role_assignment = RoleAssignment {
                        id: None,
                        name: None,
                        type_: None,
                        properties: Some(RoleAssignmentProperties {
                            principal_id: uami_principal_id.clone(),
                            role_definition_id: full_acr_role_def_id,
                            scope: Some(format!(
                                "/subscriptions/{}",
                                azure_cfg.subscription_id
                            )),
                            principal_type:
                                RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                            description: Some(format!(
                                "AcrPush for management UAMI of stack {}",
                                ctx.resource_prefix
                            )),
                            condition: None,
                            condition_version: None,
                            created_by: None,
                            created_on: None,
                            delegated_managed_identity_resource_id: None,
                            updated_by: None,
                            updated_on: None,
                        }),
                    };

                    client
                        .create_or_update_role_assignment_by_id(
                            full_assignment_id.clone(),
                            &role_assignment,
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to assign AcrPush to management UAMI".to_string(),
                            resource_id: Some(config.id.clone()),
                        })?;

                    info!(assignment_id = %full_assignment_id, "AcrPush role assigned to management UAMI");
                    self.role_assignment_ids.push(full_assignment_id);
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_cfg = ctx.get_azure_config()?;

        if let Some(uami_resource_id) = &self.uami_resource_id {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let identity_name = get_management_identity_name(ctx.resource_prefix);

            let client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;

            let identity = client
                .get_user_assigned_identity(&resource_group_name, &identity_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get management identity during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(fetched_id) = &identity.id {
                if fetched_id != uami_resource_id {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Management identity ID changed from {} to {}",
                            uami_resource_id, fetched_id
                        ),
                    }));
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
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

        info!(config_id = %config.id, "Updating management role definition and FIC");

        // Update role definition with current permissions
        let role_definition_props = self.generate_management_role_definition(ctx)?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };

        if let Some(role_def_id) = &self.role_definition_id {
            let role_def_uuid = role_def_id.split('/').last().unwrap_or(role_def_id);

            let role_definition = RoleDefinition {
                properties: Some(role_definition_props),
                ..Default::default()
            };

            auth_client
                .create_or_update_role_definition(
                    &scope,
                    role_def_uuid.to_string(),
                    &role_definition,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update management role definition".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(role_definition_id = %role_def_id, "Management role definition updated");
        }

        // Update FIC if OIDC config changed
        let azure_management = ctx.get_azure_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration required for update".to_string(),
                operation: Some("update_rsm".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        if let (Some(oidc_issuer), Some(oidc_subject)) =
            (&azure_management.oidc_issuer, &azure_management.oidc_subject)
        {
            let identity_name = get_management_identity_name(ctx.resource_prefix);
            let fic_name = get_fic_name(ctx.resource_prefix);

            let credential = FederatedIdentityCredential {
                id: None,
                name: None,
                type_: None,
                properties: Some(FederatedCredentialProperties {
                    issuer: oidc_issuer.clone(),
                    subject: oidc_subject.clone(),
                    audiences: vec!["api://AzureADTokenExchange".to_string()],
                }),
            };

            let mi_client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;

            mi_client
                .create_or_update_federated_credential(
                    &resource_group_name,
                    &identity_name,
                    &fic_name,
                    &credential,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update federated identity credential".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            self.fic_name = Some(fic_name);
            info!("Federated identity credential updated");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeletingRoleAssignments,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        for assignment_id in &self.role_assignment_ids {
            match client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Role assignment deleted");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(assignment_id = %assignment_id, "Role assignment already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete role assignment '{}'", assignment_id),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        }
        self.role_assignment_ids.clear();

        Ok(HandlerAction::Continue {
            state: DeletingRoleDefinition,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRoleDefinition,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role_definition(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        if let Some(role_def_id) = &self.role_definition_id {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let azure_cfg = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_authorization_client(azure_cfg)?;

            let scope = Scope::ResourceGroup {
                resource_group_name: resource_group_name.clone(),
            };

            let role_def_uuid = role_def_id.split('/').last().unwrap_or(role_def_id);

            match client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_def_id, "Role definition deleted");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_def_id, "Role definition already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete role definition '{}'", role_def_id),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.role_definition_id = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingFederatedCredential,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingFederatedCredential,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_federated_credential(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        if let Some(fic_name) = &self.fic_name {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let identity_name = get_management_identity_name(ctx.resource_prefix);
            let azure_cfg = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;

            match client
                .delete_federated_credential(&resource_group_name, &identity_name, fic_name)
                .await
            {
                Ok(_) => {
                    info!(fic_name = %fic_name, "Federated credential deleted");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(fic_name = %fic_name, "Federated credential already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete federated credential '{}'",
                            fic_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.fic_name = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingManagedIdentity,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagedIdentity,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        if self.uami_resource_id.is_some() {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let identity_name = get_management_identity_name(ctx.resource_prefix);
            let azure_cfg = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;

            match client
                .delete_user_assigned_identity(&resource_group_name, &identity_name)
                .await
            {
                Ok(_) => {
                    info!(identity_name = %identity_name, "Management identity deleted");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(identity_name = %identity_name, "Management identity already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete management identity '{}'",
                            identity_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }

            self.uami_resource_id = None;
            self.uami_client_id = None;
            self.uami_principal_id = None;
        }

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
        if let (Some(client_id), Some(uami_resource_id), Some(tenant_id)) = (
            &self.uami_client_id,
            &self.uami_resource_id,
            &self.tenant_id,
        ) {
            let access_config = serde_json::json!({
                "uamiClientId": client_id,
                "tenantId": tenant_id,
            });

            Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: uami_resource_id.clone(),
                access_configuration: access_config.to_string(),
            }))
        } else {
            None
        }
    }
}

impl AzureRemoteStackManagementController {
    /// Generate management role definition properties from /provision permission sets
    fn generate_management_role_definition(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<RoleDefinitionProperties> {
        let management_permissions = ctx.desired_stack.management();
        let management_profile =
            management_permissions
                .profile()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: "Management permissions not configured. Required for remote stack management.".to_string(),
                        operation: Some("generate_management_role_definition".to_string()),
                        resource_id: Some("management".to_string()),
                    })
                })?;

        let global_refs = management_profile.0.get("*").ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Management permission profile missing global permissions (*)".to_string(),
                operation: Some("generate_management_role_definition".to_string()),
                resource_id: Some("management".to_string()),
            })
        })?;

        let mut combined_actions = Vec::new();
        let mut combined_data_actions = Vec::new();
        let azure_config = ctx.get_azure_config()?;

        for permission_set_ref in global_refs {
            let permission_set =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned());

            if let Some(permission_set) = permission_set {
                if !permission_set.id.ends_with("/provision") {
                    continue;
                }

                let permission_context = PermissionContext::new()
                    .with_subscription_id(azure_config.subscription_id.clone())
                    .with_stack_prefix(ctx.resource_prefix.to_string());

                let generator = AzureRuntimePermissionsGenerator::new();
                let azure_role_def = generator
                    .generate_role_definition(
                        &permission_set,
                        BindingTarget::Stack,
                        &permission_context,
                    )
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate Azure role definition for permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("generate_management_role_definition".to_string()),
                        resource_id: Some("management".to_string()),
                    })?;

                combined_actions.extend(azure_role_def.actions);
                combined_data_actions.extend(azure_role_def.data_actions);
            } else {
                tracing::warn!(
                    permission_set_id = %permission_set_ref.id(),
                    "Management permission set not found, skipping"
                );
            }
        }

        combined_actions.sort();
        combined_actions.dedup();
        combined_data_actions.sort();
        combined_data_actions.dedup();

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let assignable_scopes = vec![format!(
            "/subscriptions/{}/resourceGroups/{}",
            azure_config.subscription_id, resource_group_name
        )];

        let role_name = format!("{}-management-role", ctx.resource_prefix);
        let description = format!(
            "Management role for Alien stack '{}'",
            ctx.resource_prefix
        );

        Ok(RoleDefinitionProperties {
            role_name: Some(role_name),
            description: Some(description),
            type_: Some("CustomRole".to_string()),
            permissions: vec![Permission {
                actions: combined_actions,
                not_actions: vec![],
                data_actions: combined_data_actions,
                not_data_actions: vec![],
            }],
            assignable_scopes,
            ..Default::default()
        })
    }

    async fn create_role_assignment_helper(
        &mut self,
        client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
        scope: &Scope,
        assignment_uuid: &str,
        principal_id: &str,
        role_definition_id: &str,
        description: &str,
        config_id: &str,
    ) -> Result<()> {
        let full_assignment_id =
            client.build_role_assignment_id(scope, assignment_uuid.to_string());

        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: role_definition_id.to_string(),
                scope: None,
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                description: Some(description.to_string()),
                condition: None,
                condition_version: None,
                created_by: None,
                created_on: None,
                delegated_managed_identity_resource_id: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        client
            .create_or_update_role_assignment_by_id(
                full_assignment_id.clone(),
                &role_assignment,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create role assignment for {}", description),
                resource_id: Some(config_id.to_string()),
            })?;

        info!(
            assignment_id = %full_assignment_id,
            principal_id = %principal_id,
            "Role assignment created"
        );

        self.role_assignment_ids.push(full_assignment_id);
        Ok(())
    }

    #[cfg(feature = "test-utils")]
    pub fn mock_ready(prefix: &str) -> Self {
        Self {
            state: AzureRemoteStackManagementState::Ready,
            uami_resource_id: Some(format!(
                "/subscriptions/sub-1234/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}-management-identity",
                prefix
            )),
            uami_client_id: Some("12345678-1234-1234-1234-123456789012".to_string()),
            uami_principal_id: Some("87654321-4321-4321-4321-210987654321".to_string()),
            tenant_id: Some("tenant-1234".to_string()),
            fic_name: Some(format!("{}-alien-fic", prefix)),
            role_definition_id: Some(format!(
                "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/{}-mgmt-role",
                prefix
            )),
            role_assignment_ids: vec![],
            _internal_stay_count: None,
        }
    }
}

use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    AzureRemoteStackManagementHeartbeatData, HeartbeatBackend, NetworkSettings, ObservedHealth,
    PermissionProfile, Platform, ProviderLifecycleState, RemoteStackManagement,
    RemoteStackManagementHeartbeatData, RemoteStackManagementHeartbeatStatus,
    RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use alien_permissions::{
    generators::{AzureGrantPlan, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use chrono::Utc;
use std::collections::{BTreeSet, HashMap};

use super::azure_remote_storage::{
    custom_roles_for_combined_management_role, desired_remote_storage_scopes,
    REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID,
};

mod management_grants;
pub(super) use management_grants::*;
mod ownership;

#[cfg(not(test))]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 300;
#[cfg(test)]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 0;

const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;
// The absolute wait deadline controls Azure RBAC propagation. Terraform-imported
// setup can drive this state quickly, so keep the Stay guard comfortably above
// the number of reconciles that can happen inside the deadline.
const AZURE_RBAC_WAIT_MAX_ATTEMPTS: u32 = 100_000;

#[controller]
pub struct AzureRemoteStackManagementController {
    /// Whether setup owns the identity, FIC, and management grants.
    ///
    /// `None` is a legacy checkpoint; ownership is then inferred from the
    /// controller state that predates this field.
    #[serde(default)]
    pub(crate) setup_managed: Option<bool>,
    /// The resource ID of the target UAMI
    pub(crate) uami_resource_id: Option<String>,
    /// The client ID of the target UAMI (used in access_configuration output)
    pub(crate) uami_client_id: Option<String>,
    /// The principal ID (object ID) of the target UAMI (used for role assignment)
    pub(crate) uami_principal_id: Option<String>,
    /// The customer's tenant ID (stored for build_outputs)
    pub(crate) tenant_id: Option<String>,
    /// The name of the FIC.
    pub(crate) fic_name: Option<String>,
    /// The full resource ID of the custom role definition
    pub(crate) role_definition_id: Option<String>,
    /// Exact-scope custom role definitions keyed by permission entry and resource scope.
    #[serde(default)]
    pub(crate) resource_role_definition_ids: HashMap<String, String>,
    /// Resource IDs of created role assignments
    pub(crate) role_assignment_ids: Vec<String>,
    /// Deadline for Azure RBAC propagation after management assignments.
    pub(crate) role_assignment_wait_until_epoch_secs: Option<u64>,
    /// Fingerprint of the management grant plan last applied to the UAMI.
    #[serde(default)]
    pub(crate) applied_management_grant_fingerprint: Option<String>,
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
        // This flow creates and therefore owns every structural resource.
        // Persist the decision before the first cloud mutation so a failed
        // direct setup cannot later be mistaken for an IaC import.
        self.setup_managed = Some(false);

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
                message: format!("Failed to create management identity '{}'", identity_name),
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

        let oidc_issuer = &azure_management.oidc_issuer;
        let oidc_subject = &azure_management.oidc_subject;

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

        if let Some(role_definition_props) = role_definition_props {
            let role_definition_uuid = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("deployment:azure:mgmt-role-def:{}", ctx.resource_prefix).as_bytes(),
            )
            .to_string();
            let role_definition = RoleDefinition {
                properties: Some(role_definition_props),
                ..Default::default()
            };
            let scope = management_role_definition_scope(
                role_definition
                    .properties
                    .as_ref()
                    .map(|properties| properties.assignable_scopes.as_slice())
                    .unwrap_or_default(),
                &azure_cfg.subscription_id,
                &resource_group_name,
            );

            let created = client
                .create_or_update_role_definition(
                    &scope,
                    role_definition_uuid.clone(),
                    &role_definition,
                )
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
        } else {
            info!("No residual Azure stack-level management custom role required");
            self.role_definition_id = None;
        }

        self.create_remote_storage_role_definitions(
            ctx,
            &client,
            azure_cfg,
            &resource_group_name,
            &config.id,
        )
        .await?;

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

        let uami_principal_id = self.uami_principal_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "UAMI principal ID not available for role assignments".to_string(),
                operation: Some("create_role_assignments".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let grant_plan = self.generate_management_grant_plan(ctx)?;
        for binding in &grant_plan.bindings {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    role_definition_id.clone()
                }
                AzureRoleDefinitionRef::Custom { key }
                    if binding.permission_set_id == REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID =>
                {
                    self.resource_role_definition_ids
                        .get(&resource_role_definition_key(key, &binding.scope))
                        .cloned()
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::InfrastructureError {
                                message: format!(
                                    "Exact-scope role definition is not available for '{}' at '{}'",
                                    binding.permission_set_id, binding.scope
                                ),
                                operation: Some("create_role_assignments".to_string()),
                                resource_id: Some(config.id.clone()),
                            })
                        })?
                }
                AzureRoleDefinitionRef::Custom { .. } => {
                    self.role_definition_id.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::InfrastructureError {
                            message: "Role definition ID not available for custom management role assignment".to_string(),
                            operation: Some("create_role_assignments".to_string()),
                            resource_id: Some(config.id.clone()),
                        })
                    })?
                }
            };
            let assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                management_role_assignment_key(
                    ctx.resource_prefix,
                    &uami_principal_id,
                    &role_definition_id,
                    &binding.scope,
                )
                .as_bytes(),
            )
            .to_string();
            self.create_role_assignment_by_scope(
                &client,
                &assignment_id,
                &uami_principal_id,
                &role_definition_id,
                &binding.scope,
                &format!(
                    "management uami {} ({})",
                    binding.role_name, ctx.resource_prefix
                ),
                &config.id,
            )
            .await?;
        }

        if let Some(vnet_resource_id) = existing_azure_vnet_resource_id(ctx) {
            let role_definition_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7",
                azure_cfg.subscription_id
            );
            let assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                existing_vnet_reader_assignment_key(
                    ctx.resource_prefix,
                    "uami",
                    &uami_principal_id,
                    &vnet_resource_id,
                )
                .as_bytes(),
            )
            .to_string();
            self.create_role_assignment_by_scope(
                &client,
                &assignment_id,
                &uami_principal_id,
                &role_definition_id,
                &vnet_resource_id,
                &format!(
                    "management uami existing VNet reader ({})",
                    ctx.resource_prefix
                ),
                &config.id,
            )
            .await?;
        }

        self.applied_management_grant_fingerprint =
            Some(super::desired_management_grant_fingerprint(
                ctx,
                &self.desired_remote_storage_scopes(ctx)?,
            )?);

        if self.role_assignment_ids.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        self.role_assignment_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: WaitingForRbacPropagation,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForRbacPropagation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let deadline = ensure_wait_deadline(
            &mut self.role_assignment_wait_until_epoch_secs,
            AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS,
        );

        if let Some(delay) = wait_delay(deadline) {
            info!(
                config_id = %config.id,
                remaining_secs = deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure management role assignment propagation"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.role_assignment_wait_until_epoch_secs = None;
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
                if !azure_utils::azure_resource_ids_equal(uami_resource_id, fetched_id) {
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

        emit_azure_remote_stack_management_heartbeat(ctx, self);

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

        if self.setup_managed_resources() {
            info!(
                config_id = %config.id,
                "Skipping runtime mutation of setup-managed Azure identity and grants"
            );
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        info!(config_id = %config.id, "Reconciling management role assignments and FIC");

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        for assignment_id in &self.role_assignment_ids {
            match auth_client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Management role assignment deleted for update");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(assignment_id = %assignment_id, "Management role assignment already absent for update");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete management role assignment '{}' for update",
                            assignment_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        self.role_assignment_ids.clear();

        if let Some(role_def_id) = &self.role_definition_id {
            let role_def_uuid = role_def_id.split('/').last().unwrap_or(role_def_id);
            let scope = role_definition_scope_from_id(role_def_id, &resource_group_name);

            match auth_client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_def_id, "Management role definition deleted for update");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_def_id, "Management role definition already absent for update");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete management role definition '{}' for update",
                            role_def_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        self.role_definition_id = None;
        self.delete_resource_role_definitions(&auth_client, &resource_group_name, &config.id)
            .await?;

        // Update FIC if OIDC config changed
        let azure_management = ctx.get_azure_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Azure management configuration required for update".to_string(),
                operation: Some("update_rsm".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let oidc_issuer = &azure_management.oidc_issuer;
        let oidc_subject = &azure_management.oidc_subject;
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

        Ok(HandlerAction::Continue {
            state: CreatingRoleDefinition,
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

        if self.setup_managed_resources() {
            info!(
                config_id = %config.id,
                "Leaving setup-managed Azure identity and grants for setup teardown"
            );
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

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
                    }));
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
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        if let Some(role_def_id) = &self.role_definition_id {
            let scope = role_definition_scope_from_id(role_def_id, &resource_group_name);

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
                    }));
                }
            }

            self.role_definition_id = None;
        }
        self.delete_resource_role_definitions(&client, &resource_group_name, &config.id)
            .await?;

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
                        message: format!("Failed to delete federated credential '{}'", fic_name),
                        resource_id: Some(config.id.clone()),
                    }));
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
                    }));
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

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if self.setup_managed_resources() {
            return Ok(false);
        }

        let desired = super::desired_management_grant_fingerprint(
            ctx,
            &self.desired_remote_storage_scopes(ctx)?,
        )?;
        Ok(self.applied_management_grant_fingerprint.as_ref() != Some(&desired))
    }
}

#[cfg(test)]
mod ownership_tests;

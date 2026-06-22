use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;
use uuid::Uuid;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper, Scope};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use crate::{azure_authorization, azure_msi};
use alien_core::{
    AzureRemoteStackManagementHeartbeatData, HeartbeatBackend, KubernetesCluster, NetworkSettings,
    ObservedHealth, PermissionProfile, Platform, ProviderLifecycleState, RemoteStackManagement,
    RemoteStackManagementHeartbeatData, RemoteStackManagementHeartbeatStatus,
    RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;
use alien_permissions::{
    generators::{
        dedupe_azure_role_bindings, AzureGrantPlan, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use azure_mgmt_authorization::package_2022_04_01::models::{
    role_assignment_properties::PrincipalType as RoleAssignmentPropertiesPrincipalType, Permission,
    RoleAssignmentCreateParameters, RoleAssignmentProperties, RoleDefinition,
    RoleDefinitionProperties,
};
use azure_mgmt_msi::package_2023_01_31::models::{
    FederatedIdentityCredential, FederatedIdentityCredentialProperties, Identity, TrackedResource,
};
use chrono::Utc;
use std::collections::BTreeSet;

#[cfg(not(test))]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 300;
#[cfg(test)]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 0;

const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;
// The absolute wait deadline controls Azure RBAC propagation. Terraform-imported
// setup can drive this state quickly, so keep the Stay guard comfortably above
// the number of reconciles that can happen inside the deadline.
const AZURE_RBAC_WAIT_MAX_ATTEMPTS: u32 = 100_000;

fn get_management_identity_name(prefix: &str) -> String {
    format!("{}-management-identity", prefix)
}

fn get_fic_name(prefix: &str) -> String {
    format!("{}-management-fic", prefix)
}

fn management_role_definition_scope(
    assignable_scopes: &[String],
    subscription_id: &str,
    resource_group_name: &str,
) -> Scope {
    let subscription_scope = format!("/subscriptions/{subscription_id}");
    if assignable_scopes
        .iter()
        .any(|scope| scope == &subscription_scope)
    {
        Scope::Subscription
    } else {
        Scope::ResourceGroup {
            resource_group_name: resource_group_name.to_string(),
        }
    }
}

fn role_definition_scope_from_id(role_definition_id: &str, resource_group_name: &str) -> Scope {
    if role_definition_id.contains("/resourceGroups/") {
        Scope::ResourceGroup {
            resource_group_name: resource_group_name.to_string(),
        }
    } else {
        Scope::Subscription
    }
}

fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn ensure_wait_deadline(wait_until_epoch_secs: &mut Option<u64>, wait_secs: u64) -> u64 {
    let now = current_unix_timestamp_secs();
    *wait_until_epoch_secs.get_or_insert_with(|| now.saturating_add(wait_secs))
}

fn wait_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_RBAC_WAIT_POLL_SECS),
        ))
    }
}

fn management_role_assignment_key(
    resource_prefix: &str,
    principal_id: &str,
    role_definition_id: &str,
    scope: &str,
) -> String {
    format!(
        "deployment:azure:mgmt-role-assign:{resource_prefix}:uami:{principal_id}:{role_definition_id}:{scope}"
    )
}

fn existing_role_assignment_id_from_conflict(
    scope: &str,
    err: &AlienError<ErrorData>,
) -> Option<String> {
    let ErrorData::CloudResourceConflict { message, .. } = err.error.as_ref()? else {
        return None;
    };

    let lower_message = message.to_ascii_lowercase();
    if !lower_message.contains("role assignment already exists")
        || !lower_message.contains("role assignment")
    {
        return None;
    }

    let normalized = message
        .chars()
        .map(|ch| {
            if ch.is_ascii_hexdigit() || ch == '-' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();

    normalized
        .split_whitespace()
        .rev()
        .find_map(|candidate| Uuid::parse_str(candidate).ok())
        .map(|assignment_uuid| {
            format!(
                "{}/providers/Microsoft.Authorization/roleAssignments/{}",
                scope, assignment_uuid
            )
        })
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
    /// The name of the FIC.
    pub(crate) fic_name: Option<String>,
    /// The full resource ID of the custom role definition
    pub(crate) role_definition_id: Option<String>,
    /// Resource IDs of created role assignments
    pub(crate) role_assignment_ids: Vec<String>,
    /// Deadline for Azure RBAC propagation after management assignments.
    pub(crate) role_assignment_wait_until_epoch_secs: Option<u64>,
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

        let identity = Identity::new(TrackedResource::new(location.to_string()));

        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        let created = azure_msi::create_or_update_user_assigned_identity(
            &client,
            &azure_cfg.subscription_id,
            &resource_group_name,
            &identity_name,
            &identity,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create management identity '{}'", identity_name),
            resource_id: Some(config.id.clone()),
        })?;

        let identity_id = created.tracked_resource.resource.id.ok_or_else(|| {
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

        let mut credential = FederatedIdentityCredential::new();
        credential.properties = Some(FederatedIdentityCredentialProperties::new(
            oidc_issuer.clone(),
            oidc_subject.clone(),
            vec!["api://AzureADTokenExchange".to_string()],
        ));

        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        azure_msi::create_or_update_federated_credential(
            &client,
            &azure_cfg.subscription_id,
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

        let role_definition_uuid = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("deployment:azure:mgmt-role-def:{}", ctx.resource_prefix).as_bytes(),
        )
        .to_string();

        let Some(role_definition_props) = role_definition_props else {
            info!("No residual Azure management custom role required");
            self.role_definition_id = None;
            return Ok(HandlerAction::Continue {
                state: CreatingRoleAssignments,
                suggested_delay: None,
            });
        };

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

        let created = azure_authorization::create_or_update_role_definition(
            &client,
            azure_cfg,
            &scope,
            &role_definition_uuid,
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
                AzureRoleDefinitionRef::Custom { .. } => self.role_definition_id.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: "Role definition ID not available for custom management role assignment".to_string(),
                        operation: Some("create_role_assignments".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                })?,
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
                max_times: AZURE_RBAC_WAIT_MAX_ATTEMPTS,
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

            let identity = azure_msi::get_user_assigned_identity(
                &client,
                &azure_cfg.subscription_id,
                &resource_group_name,
                &identity_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get management identity during heartbeat".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

            if let Some(fetched_id) = &identity.tracked_resource.resource.id {
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

        info!(config_id = %config.id, "Reconciling management role assignments and FIC");

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        for assignment_id in &self.role_assignment_ids {
            match azure_authorization::delete_role_assignment_by_id(&auth_client, assignment_id)
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Management role assignment deleted for update");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

            match azure_authorization::delete_role_definition(
                &auth_client,
                azure_cfg,
                &scope,
                role_def_uuid,
            )
            .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_def_id, "Management role definition deleted for update");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

        let mut credential = FederatedIdentityCredential::new();
        credential.properties = Some(FederatedIdentityCredentialProperties::new(
            oidc_issuer.clone(),
            oidc_subject.clone(),
            vec!["api://AzureADTokenExchange".to_string()],
        ));

        let mi_client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        azure_msi::create_or_update_federated_credential(
            &mi_client,
            &azure_cfg.subscription_id,
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
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        for assignment_id in &self.role_assignment_ids {
            match azure_authorization::delete_role_assignment_by_id(&client, assignment_id).await {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Role assignment deleted");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

        if let Some(role_def_id) = &self.role_definition_id {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let azure_cfg = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_authorization_client(azure_cfg)?;

            let scope = role_definition_scope_from_id(role_def_id, &resource_group_name);

            let role_def_uuid = role_def_id.split('/').last().unwrap_or(role_def_id);

            match azure_authorization::delete_role_definition(
                &client,
                azure_cfg,
                &scope,
                role_def_uuid,
            )
            .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_def_id, "Role definition deleted");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

            match azure_msi::delete_federated_credential(
                &client,
                &azure_cfg.subscription_id,
                &resource_group_name,
                &identity_name,
                fic_name,
            )
            .await
            {
                Ok(_) => {
                    info!(fic_name = %fic_name, "Federated credential deleted");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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

            match azure_msi::delete_user_assigned_identity(
                &client,
                &azure_cfg.subscription_id,
                &resource_group_name,
                &identity_name,
            )
            .await
            {
                Ok(_) => {
                    info!(identity_name = %identity_name, "Management identity deleted");
                }
                Err(e) if matches!(&e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{PermissionProfile, PermissionSetReference};

    fn permission_context() -> PermissionContext {
        PermissionContext::new()
            .with_subscription_id("sub-123".to_string())
            .with_resource_group("rg-123".to_string())
            .with_stack_prefix("e2e-01-azcr".to_string())
            .with_managing_subscription_id("sub-123".to_string())
            .with_managing_resource_group("rg-123".to_string())
    }

    #[test]
    fn role_assignment_conflict_parser_extracts_existing_assignment_id() {
        let err = AlienError::new(ErrorData::CloudResourceConflict {
            resource_type: "Resource".to_string(),
            resource_name: "roleAssignments/requested".to_string(),
            message: "The role assignment already exists. The ID of the conflicting role assignment is 593d47719b195096804b7b96d6e5a5ac.".to_string(),
        });

        let existing_assignment_id = existing_role_assignment_id_from_conflict(
            "/subscriptions/sub-123/resourceGroups/rg-123",
            &err,
        )
        .expect("conflict should include an existing role assignment id");

        assert_eq!(
            existing_assignment_id,
            "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/593d4771-9b19-5096-804b-7b96d6e5a5ac"
        );
    }

    #[test]
    fn management_role_assignment_key_includes_azure_immutable_fields() {
        let prefix = "e2e-03-azcr";
        let principal_id = "principal-a";
        let role_definition_id = "/subscriptions/sub-123/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7";
        let scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        let base_key =
            management_role_assignment_key(prefix, principal_id, role_definition_id, scope);

        assert_ne!(
            base_key,
            management_role_assignment_key(prefix, "principal-b", role_definition_id, scope),
            "Azure rejects updating principalId on an existing role assignment ID"
        );
        assert_ne!(
            base_key,
            management_role_assignment_key(
                prefix,
                principal_id,
                "/subscriptions/sub-123/providers/Microsoft.Authorization/roleDefinitions/custom-role",
                scope,
            ),
            "Azure rejects updating roleDefinitionId on an existing role assignment ID"
        );
        assert_ne!(
            base_key,
            management_role_assignment_key(
                prefix,
                principal_id,
                role_definition_id,
                "/subscriptions/sub-123",
            ),
            "Azure rejects updating scope on an existing role assignment ID"
        );
    }

    #[test]
    fn stack_management_grant_plan_includes_global_heartbeat_reader_grants() {
        let profile = PermissionProfile::new().global([
            PermissionSetReference::from_name("worker/provision"),
            PermissionSetReference::from_name("storage/provision"),
            PermissionSetReference::from_name("artifact-registry/provision"),
            PermissionSetReference::from_name("azure-resource-group/heartbeat"),
            PermissionSetReference::from_name("service-account/heartbeat"),
        ]);

        let grant_plan =
            generate_stack_management_grant_plan(&profile, &permission_context()).unwrap();

        assert!(
            grant_plan.custom_roles.iter().any(|role| role
                .role_definition
                .actions
                .iter()
                .any(|action| { action == "Microsoft.App/containerApps/write" })),
            "worker/provision should still contribute residual Azure management actions"
        );
        assert_eq!(
            grant_plan
                .bindings
                .iter()
                .filter(|binding| matches!(
                    binding.role_definition,
                    AzureRoleDefinitionRef::Custom { .. }
                ))
                .count(),
            1,
            "all residual custom management actions share one combined custom role assignment"
        );

        let reader_bindings: Vec<_> = grant_plan
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.role_definition,
                    AzureRoleDefinitionRef::Predefined { role_definition_id }
                        if role_definition_id.ends_with("/acdd72a7-3385-48ef-bd42-f606fba81ae7")
                )
            })
            .collect();

        assert_eq!(
            reader_bindings.len(),
            1,
            "resource-group and service-account heartbeats should share one deduped Reader assignment"
        );
        assert_eq!(
            reader_bindings[0].scope,
            "/subscriptions/sub-123/resourceGroups/rg-123"
        );
    }

    #[test]
    fn stack_management_grant_plan_includes_worker_dispatch_command_once() {
        let profile = PermissionProfile::new()
            .resource(
                "api",
                [PermissionSetReference::from_name("worker/dispatch-command")],
            )
            .resource(
                "jobs",
                [PermissionSetReference::from_name("worker/dispatch-command")],
            );

        let grant_plan =
            generate_stack_management_grant_plan(&profile, &permission_context()).unwrap();

        assert_eq!(
            grant_plan
                .bindings
                .iter()
                .filter(|binding| binding.permission_set_id == "worker/dispatch-command")
                .count(),
            1,
            "worker dispatch is a stack management transport grant and should be deduped"
        );
    }
}

fn emit_azure_remote_stack_management_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    controller: &AzureRemoteStackManagementController,
) {
    let resource_id = ctx
        .desired_resource_config::<RemoteStackManagement>()
        .map(|config| config.id.clone())
        .unwrap_or_else(|_| "remote-stack-management".to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id,
        resource_type: RemoteStackManagement::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::RemoteStackManagement(
            RemoteStackManagementHeartbeatData::AzureManagedIdentity(
                AzureRemoteStackManagementHeartbeatData {
                    status: RemoteStackManagementHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: controller.uami_resource_id.as_ref().map(|resource_id| {
                            format!("Azure management identity '{}' is reachable", resource_id)
                        }),
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    uami_resource_id: controller.uami_resource_id.clone(),
                    uami_client_id: controller.uami_client_id.clone(),
                    uami_principal_id: controller.uami_principal_id.clone(),
                    tenant_id: controller.tenant_id.clone(),
                    fic_name: controller.fic_name.clone(),
                    role_definition_id: controller.role_definition_id.clone(),
                    role_assignment_ids: controller.role_assignment_ids.clone(),
                },
            ),
        ),
        raw: vec![],
    });
}

fn existing_vnet_reader_assignment_key(
    resource_prefix: &str,
    principal_kind: &str,
    principal_id: &str,
    vnet_resource_id: &str,
) -> String {
    format!(
        "deployment:azure:existing-vnet-reader:{resource_prefix}:{principal_kind}:{principal_id}:{vnet_resource_id}"
    )
}

fn existing_azure_vnet_resource_id(ctx: &ResourceControllerContext<'_>) -> Option<String> {
    match ctx.deployment_config.stack_settings.network.as_ref()? {
        NetworkSettings::ByoVnetAzure {
            vnet_resource_id, ..
        } => Some(vnet_resource_id.clone()),
        _ => None,
    }
}

fn generate_stack_management_grant_plan(
    management_profile: &PermissionProfile,
    permission_context: &PermissionContext,
) -> Result<AzureGrantPlan> {
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();
    let generator = AzureRuntimePermissionsGenerator::new();

    if let Some(global_refs) = management_profile.0.get("*") {
        for permission_set_ref in global_refs {
            let Some(permission_set) =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned())
            else {
                tracing::warn!(
                    permission_set_id = %permission_set_ref.id(),
                    "Management permission set not found, skipping"
                );
                continue;
            };
            if permission_set.platforms.azure.is_none() {
                continue;
            }

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Stack, permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate Azure role definition for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_management_grant_plan".to_string()),
                    resource_id: Some("management".to_string()),
                })?;

            custom_roles.extend(grant_plan.custom_roles);
            bindings.extend(grant_plan.bindings);
        }
    }

    let mut seen_stack_management_refs = BTreeSet::new();
    for permission_set_ref in management_profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(_, refs)| refs.iter())
        .filter(|reference| reference.id() == "worker/dispatch-command")
    {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| get_permission_set(name).cloned())
        else {
            tracing::warn!(
                permission_set_id = %permission_set_ref.id(),
                "Management permission set not found, skipping"
            );
            continue;
        };
        if !seen_stack_management_refs.insert(permission_set.id.clone()) {
            continue;
        }
        if permission_set.platforms.azure.is_none() {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Stack, permission_context)
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to generate Azure role definition for permission set '{}'",
                    permission_set.id
                ),
                operation: Some("generate_management_grant_plan".to_string()),
                resource_id: Some("management".to_string()),
            })?;

        custom_roles.extend(grant_plan.custom_roles);
        bindings.extend(grant_plan.bindings);
    }

    Ok(AzureGrantPlan {
        custom_roles,
        bindings: dedupe_management_role_bindings(bindings),
    })
}

fn dedupe_management_role_bindings(
    bindings: Vec<alien_permissions::generators::AzureRoleBinding>,
) -> Vec<alien_permissions::generators::AzureRoleBinding> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();

    for binding in bindings {
        let role_key = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                format!("predefined:{role_definition_id}")
            }
            AzureRoleDefinitionRef::Custom { .. } => "combined-custom-management-role".to_string(),
        };

        if seen.insert((binding.scope.clone(), role_key)) {
            deduped.push(binding);
        }
    }

    deduped
}

impl AzureRemoteStackManagementController {
    /// Generate management role definition properties from /provision permission sets
    fn generate_management_role_definition(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<RoleDefinitionProperties>> {
        let grant_plan = self.generate_management_grant_plan(ctx)?;

        if grant_plan.custom_roles.is_empty() {
            return Ok(None);
        }

        let mut combined_actions = Vec::new();
        let mut combined_data_actions = Vec::new();
        let mut assignable_scopes = Vec::new();

        for custom_role in grant_plan.custom_roles {
            combined_actions.extend(custom_role.role_definition.actions);
            combined_data_actions.extend(custom_role.role_definition.data_actions);
            assignable_scopes.extend(custom_role.role_definition.assignable_scopes);
        }

        combined_actions.sort();
        combined_actions.dedup();
        combined_data_actions.sort();
        combined_data_actions.dedup();
        assignable_scopes.sort();
        assignable_scopes.dedup();

        let role_name = format!("{}-management-role", ctx.resource_prefix);
        let description = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => format!(
                "Management role for {deployment_name}. Resource prefix: {}.",
                ctx.resource_prefix
            ),
            None => format!("Management role. Resource prefix: {}.", ctx.resource_prefix),
        };

        Ok(Some(RoleDefinitionProperties {
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
        }))
    }

    fn generate_management_grant_plan(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<AzureGrantPlan> {
        let management_permissions = ctx.desired_stack.management();
        let management_profile = management_permissions.profile().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message:
                    "Management permissions not configured. Required for remote stack management."
                        .to_string(),
                operation: Some("generate_management_role_definition".to_string()),
                resource_id: Some("management".to_string()),
            })
        })?;

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let mut custom_roles = Vec::new();
        let mut bindings = Vec::new();

        let permission_context = PermissionContext::new()
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group_name.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_managing_subscription_id(azure_config.subscription_id.clone())
            .with_managing_resource_group(resource_group_name.clone());
        let permission_context = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => {
                permission_context.with_deployment_name(deployment_name.to_string())
            }
            None => permission_context,
        };

        let generator = AzureRuntimePermissionsGenerator::new();
        let grant_plan =
            generate_stack_management_grant_plan(management_profile, &permission_context)?;
        custom_roles.extend(grant_plan.custom_roles);
        bindings.extend(grant_plan.bindings);

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
                ResourcePermissionsHelper::azure_kubernetes_cluster_permission_context(
                    ctx, cluster,
                )?;

            for permission_set_ref in permission_set_refs {
                let Some(permission_set) =
                    permission_set_ref.resolve(|name| get_permission_set(name).cloned())
                else {
                    tracing::warn!(
                        permission_set_id = %permission_set_ref.id(),
                        "Management permission set not found, skipping"
                    );
                    continue;
                };
                if permission_set.platforms.azure.is_none() {
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
                            "Failed to generate Azure resource-scoped role definition for permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("generate_management_grant_plan".to_string()),
                        resource_id: Some(resource_id.clone()),
                    })?;

                custom_roles.extend(grant_plan.custom_roles);
                bindings.extend(grant_plan.bindings);
            }
        }

        Ok(AzureGrantPlan {
            custom_roles,
            bindings: dedupe_azure_role_bindings(bindings),
        })
    }

    async fn create_role_assignment_by_scope(
        &mut self,
        client: &azure_mgmt_authorization::package_2022_04_01::Client,
        assignment_uuid: &str,
        principal_id: &str,
        role_definition_id: &str,
        scope: &str,
        description: &str,
        config_id: &str,
    ) -> Result<()> {
        let full_assignment_id = format!(
            "{}/providers/Microsoft.Authorization/roleAssignments/{}",
            scope, assignment_uuid
        );

        let role_assignment = RoleAssignmentCreateParameters::new(RoleAssignmentProperties {
            principal_id: principal_id.to_string(),
            role_definition_id: role_definition_id.to_string(),
            scope: Some(scope.to_string()),
            principal_type: Some(RoleAssignmentPropertiesPrincipalType::ServicePrincipal),
            description: Some(description.to_string()),
            condition: None,
            condition_version: None,
            created_by: None,
            created_on: None,
            delegated_managed_identity_resource_id: None,
            updated_by: None,
            updated_on: None,
        });

        let create_result = azure_authorization::create_or_update_role_assignment_by_id(
            client,
            &full_assignment_id,
            &role_assignment,
        )
        .await;

        if let Err(err) = create_result {
            if let Some(existing_assignment_id) =
                existing_role_assignment_id_from_conflict(scope, &err)
            {
                info!(
                    assignment_id = %existing_assignment_id,
                    requested_assignment_id = %full_assignment_id,
                    principal_id = %principal_id,
                    role_definition_id = %role_definition_id,
                    "Role assignment already exists"
                );
                self.role_assignment_ids.push(existing_assignment_id);
                return Ok(());
            }

            return Err(err.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create role assignment for {}", description),
                resource_id: Some(config_id.to_string()),
            }));
        }

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
            fic_name: Some(format!("{}-management-fic", prefix)),
            role_definition_id: Some(format!(
                "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/{}-mgmt-role",
                prefix
            )),
            role_assignment_ids: vec![],
            role_assignment_wait_until_epoch_secs: None,
            _internal_stay_count: None,
        }
    }
}

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;
use uuid::Uuid;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
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
        custom_roles_by_scope, dedupe_azure_role_bindings, AzureGrantPlan,
        AzureManagementScopeKind, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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

/// Where to create a role assignable only at `assignable_scope`: that resource group when it is
/// in this subscription, otherwise the subscription.
fn role_definition_scope_for(assignable_scope: &str, subscription_id: &str) -> Scope {
    let segments: Vec<&str> = assignable_scope.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [subscriptions, subscription, groups, group]
            if subscriptions.eq_ignore_ascii_case("subscriptions")
                && subscription.eq_ignore_ascii_case(subscription_id)
                && groups.eq_ignore_ascii_case("resourceGroups") =>
        {
            Scope::ResourceGroup {
                resource_group_name: group.to_string(),
            }
        }
        _ => Scope::Subscription,
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
    err: &AlienError<CloudClientErrorData>,
) -> Option<String> {
    let CloudClientErrorData::RemoteResourceConflict { message, .. } = err.error.as_ref()? else {
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
    /// The single combined custom role that earlier versions created and assigned at every
    /// binding scope, which gave resource-group actions to the whole subscription. Only read
    /// so update and delete remove it.
    pub(crate) role_definition_id: Option<String>,
    /// Full resource IDs of the custom management role definitions, keyed by the one scope each
    /// is assignable and assigned at.
    #[serde(default)]
    pub(crate) scoped_role_definition_ids: BTreeMap<String, String>,
    /// Resource IDs of created role assignments
    pub(crate) role_assignment_ids: Vec<String>,
    /// Deadline for Azure RBAC propagation after management assignments.
    pub(crate) role_assignment_wait_until_epoch_secs: Option<u64>,
    /// Revision of the management permissions last assigned. Missing on
    /// controllers from before revisions were recorded.
    #[serde(default)]
    pub(crate) management_permissions_revision: Option<String>,
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
        let role_definitions = self.generate_management_role_definitions(ctx)?;

        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        // A controller saved mid-setup by an earlier version may already hold the single combined
        // role, and assignments made with it. Remove both before creating the per-scope roles.
        if let Some(legacy_role_id) = self.role_definition_id.clone() {
            for assignment_id in &self.role_assignment_ids {
                delete_management_role_assignment(&client, assignment_id, &config.id).await?;
            }
            self.role_assignment_ids.clear();
            delete_management_role_definition(
                &client,
                &legacy_role_id,
                &role_definition_scope_from_id(&legacy_role_id, &resource_group_name),
                "replaced by per-scope roles",
                &config.id,
            )
            .await?;
            self.role_definition_id = None;
        }

        if role_definitions.is_empty() {
            info!("No residual Azure management custom role required");
        }
        self.scoped_role_definition_ids.clear();
        for role in role_definitions {
            let role_definition = RoleDefinition {
                properties: Some(role.properties),
                ..Default::default()
            };
            let scope = role_definition_scope_for(&role.scope, &azure_cfg.subscription_id);
            let created = client
                .create_or_update_role_definition(&scope, role.uuid.clone(), &role_definition)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create {} management role definition for stack '{}'",
                        role.scope_kind.as_str(),
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
                scope = %role.scope,
                "Management role definition created"
            );
            self.scoped_role_definition_ids.insert(role.scope, role_id);
        }

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

        // Saved by an earlier version between creating its combined role and assigning it.
        if self.role_definition_id.is_some() {
            return Ok(HandlerAction::Continue {
                state: CreatingRoleDefinition,
                suggested_delay: None,
            });
        }

        let grant_plan = self.generate_management_grant_plan(ctx)?;
        for binding in &grant_plan.bindings {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    role_definition_id.clone()
                }
                AzureRoleDefinitionRef::Custom { .. } => self
                    .scoped_role_definition_ids
                    .get(&binding.scope)
                    .cloned()
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::InfrastructureError {
                            message: format!(
                                "No management role definition for custom grants at '{}'",
                                binding.scope
                            ),
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
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.role_assignment_wait_until_epoch_secs = None;
        self.management_permissions_revision = super::management_permissions_revision(ctx)?;
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
            // Setup may import an identity in an existing resource group without
            // adding a resource-group controller to the stack.
            let parts: Vec<_> = uami_resource_id.split('/').collect();
            let (resource_group_name, identity_name) = match parts.as_slice() {
                ["", subscriptions, subscription, groups, group, providers, namespace, identities, name]
                    if subscriptions.eq_ignore_ascii_case("subscriptions")
                        && subscription.eq_ignore_ascii_case(&azure_cfg.subscription_id)
                        && groups.eq_ignore_ascii_case("resourceGroups")
                        && providers.eq_ignore_ascii_case("providers")
                        && namespace.eq_ignore_ascii_case("Microsoft.ManagedIdentity")
                        && identities.eq_ignore_ascii_case("userAssignedIdentities")
                        && !group.is_empty()
                        && !name.is_empty() =>
                {
                    (*group, *name)
                }
                _ => {
                    return Err(AlienError::new(ErrorData::InfrastructureError {
                        message: format!("Invalid management identity ID: {uami_resource_id}"),
                        operation: Some("management_identity_heartbeat".to_string()),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            let client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;

            let identity = client
                .get_user_assigned_identity(resource_group_name, identity_name)
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

        for (role_def_id, scope) in
            self.management_role_definition_ids(&azure_cfg.subscription_id, &resource_group_name)
        {
            delete_management_role_definition(
                &auth_client,
                &role_def_id,
                &scope,
                "for update",
                &config.id,
            )
            .await?;
        }
        self.scoped_role_definition_ids.clear();
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

        if self.role_definition_id.is_some() || !self.scoped_role_definition_ids.is_empty() {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let azure_cfg = ctx.get_azure_config()?;
            let role_definition_ids = self
                .management_role_definition_ids(&azure_cfg.subscription_id, &resource_group_name);
            let client = ctx
                .service_provider
                .get_azure_authorization_client(azure_cfg)?;
            for (role_def_id, scope) in role_definition_ids {
                delete_management_role_definition(
                    &client,
                    &role_def_id,
                    &scope,
                    "on delete",
                    &config.id,
                )
                .await?;
            }
            self.role_definition_id = None;
            self.scoped_role_definition_ids.clear();
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

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        // The combined role earlier versions created is assigned at every binding scope, so it
        // can reach the whole subscription. Replace it on the next direct setup even when the
        // permissions themselves did not change.
        if self.role_definition_id.is_some()
            && ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            && ctx.deployment_config.setup_run
        {
            return Ok(true);
        }
        super::management_permissions_need_refresh(
            ctx,
            self.management_permissions_revision.as_deref(),
        )
    }

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
                legacy_remote_bindings_access: None,
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
        let err = AlienError::new(CloudClientErrorData::RemoteResourceConflict {
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

    /// A KubernetesCluster in another resource group binds its grants there, so the plan can have
    /// two resource-group scopes. Each gets its own role, with a name Azure accepts as unique in
    /// the tenant, created in the group it is assignable at.
    #[test]
    fn a_second_resource_group_gets_its_own_uniquely_named_role() {
        use alien_permissions::generators::{AzureCustomRole, AzureRoleDefinition};
        let role = |key: &str, scope: &str, action: &str| AzureCustomRole {
            key: key.to_string(),
            role_definition: AzureRoleDefinition {
                name: key.to_string(),
                id: None,
                is_custom: true,
                description: String::new(),
                actions: vec![action.to_string()],
                not_actions: vec![],
                data_actions: vec![],
                not_data_actions: vec![],
                assignable_scopes: vec![scope.to_string()],
            },
        };
        let plan = AzureGrantPlan {
            custom_roles: vec![
                role("own", "/subscriptions/sub-123/resourceGroups/rg-123", "a/write"),
                role("cluster", "/subscriptions/sub-123/resourceGroups/aks-rg", "b/read"),
                role("skus", "/subscriptions/sub-123", "c/read"),
            ],
            bindings: vec![],
        };
        let roles =
            management_role_definitions(&plan, "test", "Management role.", "sub-123", "rg-123")
                .unwrap();
        let names: BTreeSet<String> = roles
            .iter()
            .map(|r| r.properties.role_name.clone().unwrap())
            .collect();
        assert_eq!(names.len(), 3, "role names must be unique: {names:?}");
        let name_at = |scope: &str| {
            roles
                .iter()
                .find(|r| r.scope == scope)
                .and_then(|r| r.properties.role_name.clone())
                .unwrap()
        };
        assert_eq!(
            name_at("/subscriptions/sub-123/resourceGroups/rg-123"),
            "test-management-resource-group-role"
        );
        assert_eq!(name_at("/subscriptions/sub-123"), "test-management-subscription-role");
        assert!(name_at("/subscriptions/sub-123/resourceGroups/aks-rg")
            .starts_with("test-management-resource-group-role-"));
        assert!(matches!(
            role_definition_scope_for("/subscriptions/sub-123/resourceGroups/aks-rg", "sub-123"),
            Scope::ResourceGroup { resource_group_name } if resource_group_name == "aks-rg"
        ));
        assert!(matches!(
            role_definition_scope_for("/subscriptions/sub-123", "sub-123"),
            Scope::Subscription
        ));

        // The heartbeat reports the deployment group's role, and each role is deleted in the
        // group it is assignable at.
        let mut controller = AzureRemoteStackManagementController::mock_ready("test");
        controller.uami_resource_id = Some(
            "/subscriptions/sub-123/resourceGroups/rg-123/Providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-management-identity"
                .to_string(),
        );
        controller.scoped_role_definition_ids = BTreeMap::from([
            (
                "/subscriptions/sub-123/resourceGroups/aks-rg".to_string(),
                "cluster-role".to_string(),
            ),
            (
                "/subscriptions/sub-123/resourceGroups/rg-123".to_string(),
                "own-role".to_string(),
            ),
        ]);
        assert_eq!(controller.reported_role_definition_id().as_deref(), Some("own-role"));
        let cluster_delete_scope = controller
            .management_role_definition_ids("sub-123", "rg-123")
            .into_iter()
            .find(|(id, _)| id == "cluster-role")
            .map(|(_, scope)| scope);
        assert!(matches!(
            cluster_delete_scope,
            Some(Scope::ResourceGroup { resource_group_name }) if resource_group_name == "aks-rg"
        ));
    }

    /// A setup saved by an earlier version between creating its combined role and assigning it
    /// resumes in `CreatingRoleAssignments` with only `role_definition_id`. It must not fail on
    /// the missing per-scope role: it goes back to role creation, which deletes the assignments
    /// made with the combined role and the role itself before creating the per-scope roles.
    #[tokio::test]
    async fn setup_saved_with_the_combined_role_moves_to_per_scope_roles() {
        use crate::core::controller_test::SingleControllerExecutor;
        use crate::core::{MockPlatformServiceProvider, PlatformServiceProvider};
        use alien_azure_clients::authorization::{AuthorizationApi, MockAuthorizationApi};
        use std::sync::Arc;

        let legacy_role =
            "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/legacy-role";
        let legacy_assignment = "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleAssignments/legacy-assignment";
        let mut authorization = MockAuthorizationApi::new();
        authorization
            .expect_delete_role_assignment_by_id()
            .times(1)
            .withf(move |id| id == legacy_assignment)
            .returning(|_| Ok(None));
        authorization
            .expect_delete_role_definition()
            .times(1)
            .withf(|_, id| id == "legacy-role")
            .returning(|_, _| Ok(None));
        authorization
            .expect_create_or_update_role_definition()
            .returning(|_, id, definition| {
                Ok(RoleDefinition {
                    id: Some(format!(
                        "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/{id}"
                    )),
                    ..definition.clone()
                })
            });
        let authorization: Arc<dyn AuthorizationApi> = Arc::new(authorization);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        let provider: Arc<dyn PlatformServiceProvider> = Arc::new(provider);

        let mut controller = AzureRemoteStackManagementController::mock_ready("test");
        controller.state = AzureRemoteStackManagementState::CreatingRoleAssignments;
        controller.role_definition_id = Some(legacy_role.to_string());
        controller.scoped_role_definition_ids.clear();
        controller.role_assignment_ids = vec![legacy_assignment.to_string()];
        let mut executor = SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".to_string()).build())
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_dependency(
                alien_core::AzureResourceGroup::new("default-resource-group".to_string()).build(),
                crate::infra_requirements::AzureResourceGroupController::mock_ready(
                    "default-resource-group",
                ),
            )
            .build()
            .await
            .expect("executor builds");

        executor.step().await.expect("resuming the assignment step");
        let state = executor
            .internal_state::<AzureRemoteStackManagementController>()
            .expect("controller");
        assert_eq!(state.state, AzureRemoteStackManagementState::CreatingRoleDefinition);

        executor.step().await.expect("role creation replaces the combined role");
        let state = executor
            .internal_state::<AzureRemoteStackManagementController>()
            .expect("controller");
        assert_eq!(state.role_definition_id, None);
        assert!(state.role_assignment_ids.is_empty());
        assert_eq!(state.state, AzureRemoteStackManagementState::CreatingRoleAssignments);
    }

    /// A custom role grants its actions on everything under the scope it is assigned at, so each
    /// management role is assignable at one scope and holds only what is bound there. The
    /// subscription role carries SKU and usage discovery; the scale set and gateway writes stay
    /// in the resource-group role.
    #[test]
    fn management_roles_hold_only_the_grants_bound_at_their_scope() {
        let profile = PermissionProfile::new().global([
            PermissionSetReference::from_name("compute-cluster/management"),
            PermissionSetReference::from_name("container/provision"),
        ]);
        let grant_plan = generate_stack_management_grant_plan(
            &profile,
            std::iter::empty(),
            &permission_context(),
            &Default::default(),
        )
        .unwrap();
        let roles = management_role_definitions(
            &grant_plan,
            "test",
            "Management role.",
            "sub-123",
            "rg-123",
        )
        .unwrap();

        let custom_scopes: BTreeSet<&str> = grant_plan
            .bindings
            .iter()
            .filter(|b| matches!(b.role_definition, AzureRoleDefinitionRef::Custom { .. }))
            .map(|b| b.scope.as_str())
            .collect();
        let role_scopes: BTreeSet<&str> = roles.iter().map(|r| r.scope.as_str()).collect();
        assert_eq!(
            custom_scopes, role_scopes,
            "one role per custom binding scope"
        );
        for role in &roles {
            assert_eq!(role.properties.assignable_scopes, vec![role.scope.clone()]);
        }

        let actions_at = |scope: &str| -> Vec<String> {
            roles
                .iter()
                .find(|r| r.scope == scope)
                .unwrap_or_else(|| panic!("a role at {scope}"))
                .properties
                .permissions[0]
                .actions
                .clone()
        };
        assert_eq!(
            actions_at("/subscriptions/sub-123"),
            [
                "Microsoft.Compute/locations/usages/read",
                "Microsoft.Compute/skus/read"
            ]
        );
        let resource_group = actions_at("/subscriptions/sub-123/resourceGroups/rg-123");
        for action in [
            "Microsoft.Compute/virtualMachineScaleSets/write",
            "Microsoft.Network/applicationGateways/write",
        ] {
            assert!(resource_group.iter().any(|a| a == action), "{action}");
        }
        assert_ne!(
            roles[0].uuid, roles[1].uuid,
            "each scope gets its own role definition"
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

        let grant_plan = generate_stack_management_grant_plan(
            &profile,
            std::iter::empty(),
            &permission_context(),
            &Default::default(),
        )
        .unwrap();

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

    /// A remotely published sandbox puts `sandbox/management` under `*` by auto-derivation, and
    /// that set reaches a session. The setup template strips it so the remote caller is the only
    /// tenant; before this, the runtime rebuilt the role from the unfiltered profile and the first
    /// update handed the same reach back to the deployment's own identity.
    #[test]
    fn stack_management_grant_plan_drops_a_set_the_remote_caller_claims() {
        use alien_core::{ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress};

        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let stack = alien_core::Stack::new("byo-sandbox".to_string())
            .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
            .build();
        let profile = PermissionProfile::new()
            .global([PermissionSetReference::from_name("sandbox/management")]);

        let granted = generate_stack_management_grant_plan(
            &profile,
            stack.resources.values(),
            &permission_context(),
            &Default::default(),
        )
        .unwrap();
        assert!(
            granted.bindings.is_empty() && granted.custom_roles.is_empty(),
            "the remote caller claims sandbox/management, so the management identity keeps nothing"
        );

        // Without the remote binding the same set is this identity's, so the drop is not blanket.
        let own = alien_core::Stack::new("byo-sandbox".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                    })
                    .egress(SandboxEgress::Allow)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let kept = generate_stack_management_grant_plan(
            &profile,
            own.resources.values(),
            &permission_context(),
            &Default::default(),
        )
        .unwrap();
        assert!(
            !kept.bindings.is_empty() || !kept.custom_roles.is_empty(),
            "with no remote binding the management identity keeps sandbox/management"
        );
    }

    /// Azure authorizes every Application Gateway or load balancer PUT against the subnet and
    /// public IP it links to (`.../join/action`). Containers and daemons are Live, so the
    /// management identity creates their gateways, while the network is setup-owned and only
    /// contributes `network/heartbeat`. The join grants therefore have to come from the
    /// container and daemon sets, at a scope that covers the network's App Gateway subnet and
    /// the gateway's public IP and stops at the deployment resource group.
    #[test]
    fn management_role_can_link_gateways_to_the_app_gateway_subnet_and_public_ip() {
        let resource_group_scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        let app_gateway_subnet = format!(
            "{resource_group_scope}/providers/Microsoft.Network/virtualNetworks/e2e-01-azcr-default-network-vnet/subnets/e2e-01-azcr-default-network-appgw-subnet"
        );
        let public_ip = format!(
            "{resource_group_scope}/providers/Microsoft.Network/publicIPAddresses/e2e-01-azcr-web-pip"
        );

        // `container/management` PUTs the gateway again when it rotates the certificate.
        for gateway_set in [
            "container/provision",
            "daemon/provision",
            "container/management",
        ] {
            // What auto-derivation gives a stack with a public Live container or daemon on a
            // Frozen default network.
            let profile = PermissionProfile::new().global([
                PermissionSetReference::from_name(gateway_set),
                PermissionSetReference::from_name("network/heartbeat"),
            ]);
            let grant_plan = generate_stack_management_grant_plan(
                &profile,
                std::iter::empty(),
                &permission_context(),
                &Default::default(),
            )
            .unwrap();

            for (target, action) in [
                (
                    app_gateway_subnet.as_str(),
                    "Microsoft.Network/virtualNetworks/subnets/join/action",
                ),
                (
                    public_ip.as_str(),
                    "Microsoft.Network/publicIPAddresses/join/action",
                ),
            ] {
                // Runtime folds every custom role into one management role and assigns it at
                // each custom binding scope (`generate_management_role_definition`).
                let assignable_scopes: Vec<_> = grant_plan
                    .custom_roles
                    .iter()
                    .filter(|role| role.role_definition.actions.iter().any(|a| a == action))
                    .flat_map(|role| role.role_definition.assignable_scopes.iter())
                    .map(String::as_str)
                    .collect();
                assert_eq!(
                    assignable_scopes,
                    [resource_group_scope],
                    "{gateway_set}: {action} must be granted once, at the deployment resource group"
                );
                assert!(
                    grant_plan.bindings.iter().any(|binding| {
                        matches!(binding.role_definition, AzureRoleDefinitionRef::Custom { .. })
                            && target.starts_with(&format!("{}/", binding.scope))
                    }),
                    "{gateway_set}: the management role must be assigned at a scope covering {target}"
                );
            }
        }
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

        let live: std::collections::HashSet<String> = ["api".to_string(), "jobs".to_string()]
            .into_iter()
            .collect();
        let grant_plan = generate_stack_management_grant_plan(
            &profile,
            std::iter::empty(),
            &permission_context(),
            &live,
        )
        .unwrap();

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

    /// The management profile still names a resource the deployer declined —
    /// the strip removes it from the stack, never from the profile. Absence
    /// from the desired stack is the decline, and the grant must go with it,
    /// or the runtime would restore what the setup template withheld.
    #[test]
    fn stack_management_grant_plan_skips_a_declined_resource() {
        let profile = PermissionProfile::new().resource(
            "jobs",
            [PermissionSetReference::from_name("worker/dispatch-command")],
        );

        let grant_plan = generate_stack_management_grant_plan(
            &profile,
            std::iter::empty(),
            &permission_context(),
            &Default::default(),
        )
        .unwrap();

        assert!(
            grant_plan
                .bindings
                .iter()
                .all(|binding| binding.permission_set_id != "worker/dispatch-command"),
            "a declined worker's dispatch grant must not be issued at runtime"
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
                    role_definition_id: controller.reported_role_definition_id(),
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

/// `live_resource_ids` is the set of resources that still exist in the desired
/// stack. The management profile is authored before the declined-resource
/// strip runs and is never pruned by it, so a profile entry can name a
/// resource the deployer declined. Absence from the desired stack is that
/// decline, and the grant has to go with it — the setup template withholds it
/// the same way, and the two must agree.
fn generate_stack_management_grant_plan<'r, I>(
    management_profile: &PermissionProfile,
    resources: I,
    permission_context: &PermissionContext,
    live_resource_ids: &std::collections::HashSet<String>,
) -> Result<AzureGrantPlan>
where
    I: IntoIterator<Item = &'r alien_core::ResourceEntry> + Clone,
{
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();
    let generator = AzureRuntimePermissionsGenerator::new();

    // Setup and runtime have to answer the same way; see
    // `alien_permissions::management_identity_global_refs`.
    let global_refs =
        alien_permissions::management_identity_global_refs(resources, management_profile);
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
        // Skipped on the same condition the Terraform emitter uses, so push and runtime agree.
        // A set can decline the stack target deliberately — `sandbox/execute` does, because at
        // the only stack-level scope Azure RBAC can express, the resource group, its Data Owner
        // role would reach inside every session in every sibling sandbox group. Asking the
        // generator for a target a set does not declare is a hard error, so without this a
        // stack containing a sandbox fails management-role generation outright.
        let declines_stack_scope = permission_set.platforms.azure.as_ref().is_none_or(|azure| {
            azure.is_empty() || azure.iter().all(|entry| entry.binding.stack.is_none())
        });
        if declines_stack_scope {
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

    let mut seen_stack_management_refs = BTreeSet::new();
    for permission_set_ref in management_profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .filter(|(resource_id, _)| live_resource_ids.contains(resource_id.as_str()))
        .flat_map(|(_, refs)| refs.iter())
        // By registry name, never by `id()`: an inline set carries whatever id its author typed,
        // so deciding by id would let one named after this set compile at stack scope here.
        .filter(|reference| {
            matches!(
                reference,
                alien_core::permissions::PermissionSetReference::Name(name)
                    if name == "worker/dispatch-command"
            )
        })
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

/// One custom management role, assignable and assigned only at `scope`.
struct ManagementRoleDefinition {
    scope: String,
    scope_kind: AzureManagementScopeKind,
    uuid: String,
    properties: RoleDefinitionProperties,
}

/// Builds one custom role per scope the plan's custom grants bind at. Each role carries only the
/// actions bound at its scope, so a resource-group grant never reaches the subscription.
///
/// The roles at the deployment's own subscription and resource group get fixed names. A grant
/// bound elsewhere (a KubernetesCluster in another resource group) gets a name with a short hash
/// of its scope, since Azure requires custom role names to be unique in the tenant.
fn management_role_definitions(
    grant_plan: &AzureGrantPlan,
    resource_prefix: &str,
    description: &str,
    subscription_id: &str,
    resource_group_name: &str,
) -> Result<Vec<ManagementRoleDefinition>> {
    let subscription_scope = format!("/subscriptions/{subscription_id}");
    let resource_group_scope = format!("{subscription_scope}/resourceGroups/{resource_group_name}");
    let roles = custom_roles_by_scope(grant_plan).context(ErrorData::InfrastructureError {
        message: "Failed to group Azure management grants by scope".to_string(),
        operation: Some("generate_management_role_definitions".to_string()),
        resource_id: Some("management".to_string()),
    })?;
    Ok(roles
        .into_iter()
        .map(|role| {
            let kind = role.scope_kind.as_str();
            let uuid = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!("deployment:azure:mgmt-role-def:{resource_prefix}:{}", role.scope)
                    .as_bytes(),
            )
            .to_string();
            let own_scope = role.scope.eq_ignore_ascii_case(&subscription_scope)
                || role.scope.eq_ignore_ascii_case(&resource_group_scope);
            let role_name = if own_scope {
                format!("{resource_prefix}-management-{kind}-role")
            } else {
                format!("{resource_prefix}-management-{kind}-role-{}", &uuid[..8])
            };
            ManagementRoleDefinition {
                uuid,
                properties: RoleDefinitionProperties {
                    role_name: Some(role_name),
                    description: Some(format!("{description} Scope: {kind}.")),
                    type_: Some("CustomRole".to_string()),
                    permissions: vec![Permission {
                        actions: role.actions,
                        not_actions: vec![],
                        data_actions: role.data_actions,
                        not_data_actions: vec![],
                    }],
                    assignable_scopes: vec![role.scope.clone()],
                    ..Default::default()
                },
                scope: role.scope,
                scope_kind: role.scope_kind,
            }
        })
        .collect())
}

async fn delete_management_role_assignment(
    client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
    assignment_id: &str,
    config_id: &str,
) -> Result<()> {
    match client
        .delete_role_assignment_by_id(assignment_id.to_string())
        .await
    {
        Ok(_) => {
            info!(assignment_id = %assignment_id, "Management role assignment deleted");
            Ok(())
        }
        Err(e)
            if matches!(
                &e.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            info!(assignment_id = %assignment_id, "Management role assignment already absent");
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete management role assignment '{assignment_id}'"),
            resource_id: Some(config_id.to_string()),
        })),
    }
}

async fn delete_management_role_definition(
    client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
    role_def_id: &str,
    scope: &Scope,
    purpose: &str,
    config_id: &str,
) -> Result<()> {
    let role_def_uuid = role_def_id.split('/').last().unwrap_or(role_def_id);
    match client
        .delete_role_definition(scope, role_def_uuid.to_string())
        .await
    {
        Ok(_) => {
            info!(role_definition_id = %role_def_id, purpose, "Management role definition deleted");
            Ok(())
        }
        Err(e)
            if matches!(
                &e.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            info!(role_definition_id = %role_def_id, purpose, "Management role definition already absent");
            Ok(())
        }
        Err(e) => Err(e.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to delete management role definition '{role_def_id}' {purpose}"
            ),
            resource_id: Some(config_id.to_string()),
        })),
    }
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
    /// The custom management roles for this stack, one per scope its grants bind at.
    fn generate_management_role_definitions(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<ManagementRoleDefinition>> {
        let grant_plan = self.generate_management_grant_plan(ctx)?;
        let description = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => format!(
                "Management role for {deployment_name}. Resource prefix: {}.",
                ctx.resource_prefix
            ),
            None => format!("Management role. Resource prefix: {}.", ctx.resource_prefix),
        };
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        management_role_definitions(
            &grant_plan,
            ctx.resource_prefix,
            &description,
            &azure_cfg.subscription_id,
            &resource_group_name,
        )
    }

    /// The role reported in the heartbeat: a combined role still left from an earlier version,
    /// otherwise the role at the deployment resource group.
    fn reported_role_definition_id(&self) -> Option<String> {
        // The management identity lives in the deployment resource group.
        let deployment_group = self.uami_resource_id.as_deref().and_then(|id| {
            id.to_ascii_lowercase()
                .split("/providers/")
                .next()
                .map(str::to_string)
        });
        self.role_definition_id.clone().or_else(|| {
            self.scoped_role_definition_ids
                .iter()
                .find(|(scope, _)| Some(scope.to_ascii_lowercase()) == deployment_group)
                .map(|(_, id)| id.clone())
        })
    }

    /// Every custom management role this controller may own: the per-scope roles and the
    /// single combined role older controllers created.
    /// Each with the scope to delete it at: the group a per-scope role is assignable at, or the
    /// scope its ID names for the combined role.
    fn management_role_definition_ids(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
    ) -> Vec<(String, Scope)> {
        self.role_definition_id
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    role_definition_scope_from_id(id, resource_group_name),
                )
            })
            .chain(
                self.scoped_role_definition_ids
                    .iter()
                    .map(|(scope, id)| (id.clone(), role_definition_scope_for(scope, subscription_id))),
            )
            .collect()
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
        let grant_plan = generate_stack_management_grant_plan(
            management_profile,
            ctx.desired_stack.resources.values(),
            &permission_context,
            &ctx.desired_stack.resources.keys().cloned().collect(),
        )?;
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
        client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
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

        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: role_definition_id.to_string(),
                scope: Some(scope.to_string()),
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

        let create_result = client
            .create_or_update_role_assignment_by_id(full_assignment_id.clone(), &role_assignment)
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
            role_definition_id: None,
            scoped_role_definition_ids: BTreeMap::from([(
                "/subscriptions/sub-1234/resourceGroups/test-rg".to_string(),
                format!(
                    "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/{}-mgmt-role",
                    prefix
                ),
            )]),
            role_assignment_ids: vec![],
            role_assignment_wait_until_epoch_secs: None,
            management_permissions_revision: None,
            _internal_stay_count: None,
        }
    }
}

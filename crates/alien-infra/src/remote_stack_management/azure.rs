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
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::{
    generators::{
        azure_role_assignment_allowlist_conditions, azure_runtime_roles,
        dedupe_azure_role_bindings, management_roles_by_scope,
        protected_management_assignable_scopes, AzureGrantPlan, AzureManagementScopeKind,
        AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator, AzureRuntimeRoles,
        AzureScopedCustomRole,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[cfg(not(test))]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 300;
#[cfg(test)]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 0;

const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;

/// The layout of the management roles this version creates: role-assignment writes in their own
/// role, assigned with conditions that admit only the roles setup made for the runtime; no
/// role-definition writes; and every role also assignable at the subscription. A controller saved
/// with an older layout rebuilds its roles on the next direct setup, even when the permissions
/// themselves did not change.
pub(crate) const MANAGEMENT_ROLE_LAYOUT: u32 = 2;

/// Key under which a role-assignment role past the first condition shard is recorded.
fn role_assignment_role_key(scope: &str, shard: usize) -> String {
    if shard == 0 {
        scope.to_string()
    } else {
        format!("{scope}#shard-{shard}")
    }
}

/// The scope and condition shard a role-assignment role key names.
fn role_assignment_role_scope(key: &str) -> (&str, usize) {
    match key.rsplit_once("#shard-") {
        Some((scope, shard)) => (scope, shard.parse().unwrap_or(0)),
        None => (key, 0),
    }
}
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
    let segments: Vec<&str> = assignable_scope
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
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
    /// Full resource IDs of the roles that hold only the identity's role-assignment writes, keyed
    /// by scope. They are assigned with a condition that keeps the identity from assigning roles
    /// to itself.
    #[serde(default)]
    pub(crate) role_assignment_role_definition_ids: BTreeMap<String, String>,
    /// [`MANAGEMENT_ROLE_LAYOUT`] of the roles last created. 0 on controllers saved before the
    /// layout was recorded.
    #[serde(default)]
    pub(crate) role_layout: u32,
    /// Full resource IDs of the roles setup creates for the runtime to assign on resources after
    /// setup, keyed by role definition GUID. Updates keep them, since assignments on Live
    /// resources use them; delete removes them.
    #[serde(default)]
    pub(crate) runtime_role_definition_ids: BTreeMap<String, String>,
    /// Revision of the runtime roles and the conditions last applied. Missing on controllers
    /// from before setup created those roles.
    #[serde(default)]
    pub(crate) runtime_roles_revision: Option<String>,
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
        // The role-assignment conditions name the application's identities.
        if !runtime_roles(ctx)?.is_empty() && principals_ready(ctx)?.is_none() {
            info!("Waiting for the application's identities before creating management roles");
            return Ok(principal_wait());
        }
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
        self.role_assignment_role_definition_ids.clear();
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
            if role.role_assignments {
                self.role_assignment_role_definition_ids
                    .insert(role_assignment_role_key(&role.scope, role.shard), role_id);
            } else {
                self.scoped_role_definition_ids.insert(role.scope, role_id);
            }
        }
        self.role_layout = MANAGEMENT_ROLE_LAYOUT;

        // The roles the runtime assigns after setup. Setup creates them so management, which may
        // not write role definitions, finds each already made and only assigns it.
        let runtime_roles = runtime_roles(ctx)?;
        let resource_group_scope = Scope::ResourceGroup {
            resource_group_name: resource_group_name.clone(),
        };
        let subscription_scope = format!("/subscriptions/{}", azure_cfg.subscription_id);
        let resource_group_id =
            format!("{subscription_scope}/resourceGroups/{resource_group_name}");
        for role in &runtime_roles.custom_roles {
            let uuid = role.uuid(ctx.resource_prefix);
            let definition = &role.role_definition;
            let role_definition = RoleDefinition {
                properties: Some(RoleDefinitionProperties {
                    role_name: Some(role.role_name(ctx.resource_prefix)),
                    description: Some(format!(
                        "{} Assigned after setup by the management identity. Resource prefix: {}.",
                        definition.description, ctx.resource_prefix
                    )),
                    type_: Some("CustomRole".to_string()),
                    permissions: vec![Permission {
                        actions: definition.actions.clone(),
                        not_actions: definition.not_actions.clone(),
                        data_actions: definition.data_actions.clone(),
                        not_data_actions: definition.not_data_actions.clone(),
                    }],
                    assignable_scopes: protected_management_assignable_scopes(
                        &resource_group_id,
                        &subscription_scope,
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let created = client
                .create_or_update_role_definition(&resource_group_scope, uuid.clone(), &role_definition)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create role '{}' the runtime assigns",
                        role.role_name(ctx.resource_prefix)
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            let role_id = created.id.ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "Created role definition missing ID".to_string(),
                    operation: Some("create_runtime_role_definition".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            info!(role_definition_id = %role_id, "Runtime role definition created");
            self.runtime_role_definition_ids.insert(uuid, role_id);
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
            // A scope whose custom grants were only role-assignment writes has no management
            // role; those writes are assigned below.
            if matches!(
                binding.role_definition,
                AzureRoleDefinitionRef::Custom { .. }
            ) && !self.scoped_role_definition_ids.contains_key(&binding.scope)
                && self
                    .role_assignment_role_definition_ids
                    .contains_key(&binding.scope)
            {
                continue;
            }
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
                None,
                &config.id,
            )
            .await?;
        }
        // Role-assignment writes sit in their own role, assigned with conditions that admit only
        // the roles setup made for the runtime: management's own onto itself, the application's
        // onto any other principal. One role per condition shard.
        let runtime_roles = runtime_roles(ctx)?;
        let Some(principals) = principals_ready(ctx)? else {
            info!("Waiting for the application's identities before assigning management roles");
            return Ok(principal_wait());
        };
        let conditions =
            role_assignment_conditions(ctx, &runtime_roles, &uami_principal_id, &principals);
        for (key, role_definition_id) in self.role_assignment_role_definition_ids.clone() {
            let (scope, shard) = role_assignment_role_scope(&key);
            let condition = conditions.get(shard).cloned().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: format!(
                        "No role-assignment condition for shard {shard} at '{scope}'"
                    ),
                    operation: Some("create_role_assignments".to_string()),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let shard_suffix = if shard == 0 {
                String::new()
            } else {
                format!(":shard-{shard}")
            };
            let assignment_id = Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                format!(
                    "{}:role-assignments{shard_suffix}",
                    management_role_assignment_key(
                        ctx.resource_prefix,
                        &uami_principal_id,
                        &role_definition_id,
                        scope,
                    )
                )
                .as_bytes(),
            )
            .to_string();
            self.create_role_assignment_by_scope(
                &client,
                &assignment_id,
                &uami_principal_id,
                &role_definition_id,
                scope,
                &format!(
                    "management uami role assignments for the stack's roles ({})",
                    ctx.resource_prefix
                ),
                Some(condition),
                &config.id,
            )
            .await?;
        }
        self.runtime_roles_revision =
            Some(runtime_roles_revision(ctx, &runtime_roles, &principals)?);

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
                None,
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
        self.role_assignment_role_definition_ids.clear();
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

        if self.role_definition_id.is_some()
            || !self.scoped_role_definition_ids.is_empty()
            || !self.role_assignment_role_definition_ids.is_empty()
        {
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
            self.role_assignment_role_definition_ids.clear();
        }

        if !self.runtime_role_definition_ids.is_empty() {
            let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
            let azure_cfg = ctx.get_azure_config()?;
            let client = ctx
                .service_provider
                .get_azure_authorization_client(azure_cfg)?;
            let scope = Scope::ResourceGroup {
                resource_group_name,
            };
            for (uuid, role_def_id) in self.runtime_role_definition_ids.clone() {
                delete_management_role_definition(
                    &client,
                    &role_def_id,
                    &scope,
                    "the runtime assigned, on delete",
                    &config.id,
                )
                .await?;
                self.runtime_role_definition_ids.remove(&uuid);
            }
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
        if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            && ctx.deployment_config.setup_run
        {
            if self.role_definition_id.is_some() {
                return Ok(true);
            }
            // A setup from before role-assignment writes had their own conditioned role keeps
            // them, and its roles management can change, until the roles are rebuilt. The
            // permission sets did not change, so the revision alone would not rebuild them.
            if self.role_layout < MANAGEMENT_ROLE_LAYOUT {
                return Ok(true);
            }
            // A release that adds a grant on a resource needs its role made and allowed. The
            // runtime cannot do either, so its refused step waits for this setup run.
            let runtime_roles = runtime_roles(ctx)?;
            if !(runtime_roles.is_empty() && self.runtime_roles_revision.is_none()) {
                // An identity this setup run has yet to create is a change too.
                let Ok(principals) = workload_principal_ids(ctx) else {
                    return Ok(true);
                };
                if self.runtime_roles_revision.as_deref()
                    != Some(runtime_roles_revision(ctx, &runtime_roles, &principals)?.as_str())
                {
                    return Ok(true);
                }
            }
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
                role(
                    "own",
                    "/subscriptions/sub-123/resourceGroups/rg-123",
                    "a/write",
                ),
                role(
                    "cluster",
                    "/subscriptions/sub-123/resourceGroups/aks-rg",
                    "b/read",
                ),
                role("skus", "/subscriptions/sub-123", "c/read"),
            ],
            bindings: vec![],
        };
        let roles =
            management_role_definitions(&plan, "test", "Management role.", "sub-123", "rg-123", 0)
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
        assert_eq!(
            name_at("/subscriptions/sub-123"),
            "test-management-subscription-role"
        );
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
        assert_eq!(
            controller.reported_role_definition_id().as_deref(),
            Some("own-role")
        );
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
        assert_eq!(
            state.state,
            AzureRemoteStackManagementState::CreatingRoleDefinition
        );

        executor
            .step()
            .await
            .expect("role creation replaces the combined role");
        let state = executor
            .internal_state::<AzureRemoteStackManagementController>()
            .expect("controller");
        assert_eq!(state.role_definition_id, None);
        assert!(state.role_assignment_ids.is_empty());
        assert_eq!(
            state.state,
            AzureRemoteStackManagementState::CreatingRoleAssignments
        );
    }

    /// A custom role grants its actions on everything under the scope it is assigned at, so each
    /// management role is assignable at one scope and holds only what is bound there. The
    /// subscription role carries SKU and usage discovery; the scale set and gateway writes stay
    /// in the resource-group role.
    /// A setup that grants management no role-assignment writes of its own (one that is not
    /// automatic) still gets the conditioned role-assignment role when the runtime assigns roles
    /// after setup, one role per condition shard, each under its own id. Without runtime roles
    /// nothing is added.
    #[test]
    fn runtime_roles_give_even_a_manual_setup_its_conditioned_role_assignment_roles() {
        let plan = AzureGrantPlan {
            custom_roles: vec![],
            bindings: vec![],
        };
        let none =
            management_role_definitions(&plan, "test", "Management role.", "sub-123", "rg-123", 0)
                .unwrap();
        assert!(none.is_empty());

        let roles =
            management_role_definitions(&plan, "test", "Management role.", "sub-123", "rg-123", 3)
                .unwrap();
        assert_eq!(roles.len(), 3);
        assert!(roles.iter().all(|role| role.role_assignments
            && role.scope == "/subscriptions/sub-123/resourceGroups/rg-123"
            && role.properties.permissions[0].actions
                == vec![
                    "Microsoft.Authorization/roleAssignments/delete",
                    "Microsoft.Authorization/roleAssignments/write"
                ]
            && role.properties.assignable_scopes
                == vec![
                    "/subscriptions/sub-123/resourceGroups/rg-123",
                    "/subscriptions/sub-123"
                ]));
        let shards: Vec<usize> = roles.iter().map(|role| role.shard).collect();
        assert_eq!(shards, vec![0, 1, 2]);
        let ids: BTreeSet<&str> = roles.iter().map(|role| role.uuid.as_str()).collect();
        assert_eq!(ids.len(), 3, "each shard has its own role");
        let names: BTreeSet<String> = roles
            .iter()
            .map(|role| role.properties.role_name.clone().unwrap())
            .collect();
        assert_eq!(names.len(), 3, "each shard has its own name");
        // Shard 0 keeps the id the role had before shards, so an update finds it.
        assert_eq!(
            roles[0].uuid,
            Uuid::new_v5(
                &Uuid::NAMESPACE_OID,
                b"deployment:azure:mgmt-role-assignments-role-def:test:/subscriptions/sub-123/resourceGroups/rg-123"
            )
            .to_string()
        );
    }

    #[test]
    fn role_assignment_role_keys_round_trip_their_scope_and_shard() {
        let scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        for shard in [0, 1, 7] {
            assert_eq!(
                role_assignment_role_scope(&role_assignment_role_key(scope, shard)),
                (scope, shard)
            );
        }
    }

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
            0,
        )
        .unwrap();

        let custom_scopes: BTreeSet<&str> = grant_plan
            .bindings
            .iter()
            .filter(|b| matches!(b.role_definition, AzureRoleDefinitionRef::Custom { .. }))
            .map(|b| b.scope.as_str())
            .collect();
        let role_scopes: BTreeSet<&str> = roles
            .iter()
            .filter(|r| !r.role_assignments)
            .map(|r| r.scope.as_str())
            .collect();
        assert_eq!(
            custom_scopes, role_scopes,
            "one role per custom binding scope"
        );
        // Every role management is assigned is also assignable at the subscription, where
        // management cannot change role definitions, so it cannot widen its own roles.
        for role in &roles {
            let mut expected = vec![role.scope.clone()];
            if role.scope != "/subscriptions/sub-123" {
                expected.push("/subscriptions/sub-123".to_string());
            }
            assert_eq!(role.properties.assignable_scopes, expected);
        }
        // Role-assignment writes leave the management role for a role of their own.
        let writes = roles
            .iter()
            .filter(|r| r.role_assignments)
            .map(|r| {
                (
                    r.scope.as_str(),
                    r.properties.permissions[0].actions.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            writes,
            [(
                "/subscriptions/sub-123/resourceGroups/rg-123",
                vec![
                    "Microsoft.Authorization/roleAssignments/delete".to_string(),
                    "Microsoft.Authorization/roleAssignments/write".to_string(),
                ]
            )]
        );

        let actions_at = |scope: &str| -> Vec<String> {
            roles
                .iter()
                .find(|r| r.scope == scope && !r.role_assignments)
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
        for write in [
            "Microsoft.Authorization/roleAssignments/write",
            "Microsoft.Authorization/roleAssignments/delete",
        ] {
            assert!(
                !resource_group.iter().any(|a| a == write),
                "{write} must be in the conditioned role only: {resource_group:?}"
            );
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

    /// The ComputeCluster is setup-owned, but the manager PUTs its scale sets with the
    /// management role whenever it scales a capacity group, changes the machine size or rolls
    /// the nodes. Azure authorizes every scale set PUT against the resources its profile links
    /// to (`LinkedAuthorizationFailed` otherwise): the cluster NSG, the subnet and the node
    /// identity. All three live in the deployment resource group, and so must the grants.
    #[test]
    fn management_role_can_link_scale_sets_to_the_cluster_nsg_subnet_and_identity() {
        let resource_group_scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        let nsg = format!(
            "{resource_group_scope}/providers/Microsoft.Network/networkSecurityGroups/e2e-01-azcr-compute-nsg"
        );
        let subnet = format!(
            "{resource_group_scope}/providers/Microsoft.Network/virtualNetworks/e2e-01-azcr-default-network-vnet/subnets/e2e-01-azcr-default-network-private-subnet"
        );
        let node_identity = format!(
            "{resource_group_scope}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/e2e-01-azcr-compute-identity"
        );

        // A ComputeCluster runs Live containers or daemons, so auto-derivation pairs
        // `compute-cluster/management` with one of their provision sets.
        for workload_set in ["container/provision", "daemon/provision"] {
            let profile = PermissionProfile::new().global([
                PermissionSetReference::from_name("compute-cluster/management"),
                PermissionSetReference::from_name(workload_set),
                PermissionSetReference::from_name("network/heartbeat"),
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
                0,
            )
            .unwrap();

            for (target, action) in [
                (
                    nsg.as_str(),
                    "Microsoft.Network/networkSecurityGroups/join/action",
                ),
                (
                    subnet.as_str(),
                    "Microsoft.Network/virtualNetworks/subnets/join/action",
                ),
                (
                    node_identity.as_str(),
                    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
                ),
            ] {
                // The roles the controller creates: one per scope, each assigned at its scope.
                let holders: Vec<_> = roles
                    .iter()
                    .filter(|role| {
                        role.properties.permissions[0]
                            .actions
                            .iter()
                            .any(|a| a == action)
                    })
                    .collect();
                assert_eq!(
                    holders
                        .iter()
                        .map(|role| role.scope.as_str())
                        .collect::<Vec<_>>(),
                    [resource_group_scope],
                    "{workload_set}: {action} must be in the resource-group management role only"
                );
                // Also assignable at the subscription, so management cannot change it.
                assert_eq!(
                    holders[0].properties.assignable_scopes,
                    [resource_group_scope, "/subscriptions/sub-123"]
                );
                assert!(
                    target.starts_with(&format!("{}/", holders[0].scope)),
                    "{workload_set}: the role granting {action} must cover {target}"
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
    /// Holds only role-assignment writes, assigned with the allowlist conditions.
    role_assignments: bool,
    /// Which allowlist condition a role-assignment role is assigned with; 0 for other roles.
    shard: usize,
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
    role_assignment_shards: usize,
) -> Result<Vec<ManagementRoleDefinition>> {
    let subscription_scope = format!("/subscriptions/{subscription_id}");
    let resource_group_scope = format!("{subscription_scope}/resourceGroups/{resource_group_name}");
    let mut roles =
        management_roles_by_scope(grant_plan).context(ErrorData::InfrastructureError {
            message: "Failed to group Azure management grants by scope".to_string(),
            operation: Some("generate_management_role_definitions".to_string()),
            resource_id: Some("management".to_string()),
        })?;
    // The runtime assigns roles on resources it creates after setup even when the stack grants
    // management no role-assignment writes itself (a setup that is not automatic). The
    // conditions limit those writes to the stack's roles, so the role is safe to add.
    if role_assignment_shards > 0
        && !roles
            .role_assignment_roles
            .iter()
            .any(|role| role.scope.eq_ignore_ascii_case(&resource_group_scope))
    {
        roles.role_assignment_roles.push(AzureScopedCustomRole {
            scope: resource_group_scope.clone(),
            scope_kind: AzureManagementScopeKind::ResourceGroup,
            actions: vec![
                "Microsoft.Authorization/roleAssignments/delete".to_string(),
                "Microsoft.Authorization/roleAssignments/write".to_string(),
            ],
            data_actions: Vec::new(),
            not_actions: Vec::new(),
        });
    }
    let definition = |role: &AzureScopedCustomRole, role_assignments: bool, shard: usize| {
        let kind = role.scope_kind.as_str();
        let (id_part, name_part) = if role_assignments {
            ("mgmt-role-assignments-role-def", "-role-assignments")
        } else {
            ("mgmt-role-def", "")
        };
        let shard_part = if shard == 0 {
            String::new()
        } else {
            format!(":shard-{shard}")
        };
        let uuid = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "deployment:azure:{id_part}:{resource_prefix}:{}{shard_part}",
                role.scope
            )
            .as_bytes(),
        )
        .to_string();
        let own_scope = role.scope.eq_ignore_ascii_case(&subscription_scope)
            || role.scope.eq_ignore_ascii_case(&resource_group_scope);
        let shard_name = if shard == 0 {
            String::new()
        } else {
            format!("-{shard}")
        };
        let role_name = if own_scope {
            format!("{resource_prefix}-management{name_part}-{kind}-role{shard_name}")
        } else {
            format!(
                "{resource_prefix}-management{name_part}-{kind}-role-{}{shard_name}",
                &uuid[..8]
            )
        };
        ManagementRoleDefinition {
            uuid,
            properties: RoleDefinitionProperties {
                role_name: Some(role_name),
                description: Some(format!("{description} Scope: {kind}.")),
                type_: Some("CustomRole".to_string()),
                permissions: vec![Permission {
                    actions: role.actions.clone(),
                    not_actions: role.not_actions.clone(),
                    data_actions: role.data_actions.clone(),
                    not_data_actions: vec![],
                }],
                assignable_scopes: protected_management_assignable_scopes(
                    &role.scope,
                    &subscription_scope,
                ),
                ..Default::default()
            },
            scope: role.scope.clone(),
            scope_kind: role.scope_kind,
            role_assignments,
            shard,
        }
    };
    let shards = role_assignment_shards.max(1);
    Ok(roles
        .roles
        .iter()
        .map(|role| definition(role, false, 0))
        .chain(roles.role_assignment_roles.iter().flat_map(|role| {
            (0..shards).map(move |shard| (role, shard))
        }).map(|(role, shard)| definition(role, true, shard)))
        .collect())
}

/// The runtime-roles revision a setup run would record for this stack now.
#[cfg(test)]
pub(crate) fn current_runtime_roles_revision(
    ctx: &ResourceControllerContext<'_>,
) -> Result<String> {
    let runtime_roles = runtime_roles(ctx)?;
    let principals = workload_principal_ids(ctx)?;
    runtime_roles_revision(ctx, &runtime_roles, &principals)
}

/// Every role the runtime assigns after setup for this stack.
fn runtime_roles(ctx: &ResourceControllerContext<'_>) -> Result<AzureRuntimeRoles> {
    azure_runtime_roles(ctx.desired_stack).context(ErrorData::InfrastructureError {
        message: "Failed to list the roles the runtime assigns".to_string(),
        operation: Some("runtime_roles".to_string()),
        resource_id: Some("management".to_string()),
    })
}

/// The conditions management's role-assignment roles are assigned with, one per shard.
fn role_assignment_conditions(
    ctx: &ResourceControllerContext<'_>,
    runtime_roles: &AzureRuntimeRoles,
    management_principal_id: &str,
    workload_principal_ids: &BTreeSet<String>,
) -> Vec<String> {
    azure_role_assignment_allowlist_conditions(
        management_principal_id,
        workload_principal_ids,
        &runtime_roles.management_role_ids(ctx.resource_prefix),
        &runtime_roles.workload_role_ids(ctx.resource_prefix),
    )
}

/// The principal ids of the application's identities, which the runtime assigns roles to: every
/// service account's managed identity and the remote-bindings identity.
///
/// Setup creates them alongside this controller. They are not its declared dependencies: adding
/// one would change this Frozen resource's entry on every existing installation, and the runtime
/// would then run its update with credentials that may not rebuild management's own roles. So the
/// steps that need them wait for them instead ([`principals_ready`]).
fn workload_principal_ids(ctx: &ResourceControllerContext<'_>) -> Result<BTreeSet<String>> {
    let mut principals = BTreeSet::new();
    for (resource_id, entry) in &ctx.desired_stack.resources {
        let resource_type = entry.config.resource_type();
        let principal = if resource_type == alien_core::ServiceAccount::RESOURCE_TYPE {
            ctx.require_dependency::<crate::service_account::AzureServiceAccountController>(
                &(&entry.config).into(),
            )?
            .identity_principal_id
        } else if resource_type == alien_core::RemoteBindings::RESOURCE_TYPE {
            ctx.require_dependency::<crate::remote_bindings::AzureRemoteBindingsController>(
                &(&entry.config).into(),
            )?
            .principal_id
        } else {
            continue;
        };
        let principal = principal.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: "management".to_string(),
                dependency_id: resource_id.clone(),
            })
        })?;
        principals.insert(principal.to_ascii_lowercase());
    }
    Ok(principals)
}

/// The application's principal ids, or `None` while an identity setup is creating is not ready.
fn principals_ready(ctx: &ResourceControllerContext<'_>) -> Result<Option<BTreeSet<String>>> {
    match workload_principal_ids(ctx) {
        Ok(principals) => Ok(Some(principals)),
        Err(error)
            if matches!(
                error.error,
                Some(ErrorData::DependencyNotReady { .. } | ErrorData::DependencyNotFound { .. })
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Waits for the application's identities: tries again every 10 seconds, up to 30 minutes.
fn principal_wait() -> AzureRemoteStackManagementHandlerAction {
    AzureRemoteStackManagementHandlerAction::Stay {
        max_times: Some(180),
        suggested_delay: Some(Duration::from_secs(10)),
    }
}

/// A digest of the runtime roles and what they grant, so a setup run notices a release that
/// adds or changes one.
fn runtime_roles_revision(
    ctx: &ResourceControllerContext<'_>,
    runtime_roles: &AzureRuntimeRoles,
    workload_principal_ids: &BTreeSet<String>,
) -> Result<String> {
    let mut entries: Vec<_> = runtime_roles
        .custom_roles
        .iter()
        .map(|role| {
            let definition = &role.role_definition;
            serde_json::json!([
                role.uuid(ctx.resource_prefix),
                definition.actions,
                definition.not_actions,
                definition.data_actions,
                definition.not_data_actions,
            ])
        })
        .collect();
    entries.sort_by_key(|entry| entry.to_string());
    let digest = serde_json::to_vec(&serde_json::json!({
        "custom": entries,
        "management": runtime_roles.management_predefined_role_ids,
        "workload": runtime_roles.workload_predefined_role_ids,
        "principals": workload_principal_ids,
    }))
    .into_alien_error()
    .context(ErrorData::InfrastructureError {
        message: "Failed to serialize the runtime roles".to_string(),
        operation: Some("runtime_roles_revision".to_string()),
        resource_id: Some("management".to_string()),
    })?;
    Ok(format!("{:x}", Sha256::digest(&digest)))
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
        // How many conditions the allowlist needs. A principal id is a fixed-length GUID, so the
        // count does not depend on which one.
        let runtime_roles = runtime_roles(ctx)?;
        let shards = if runtime_roles.is_empty() {
            0
        } else {
            let principals = workload_principal_ids(ctx)?;
            role_assignment_conditions(ctx, &runtime_roles, &Uuid::nil().to_string(), &principals)
                .len()
        };
        management_role_definitions(
            &grant_plan,
            ctx.resource_prefix,
            &description,
            &azure_cfg.subscription_id,
            &resource_group_name,
            shards,
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
                    .chain(&self.role_assignment_role_definition_ids)
                    .map(|(key, id)| {
                        let (scope, _) = role_assignment_role_scope(key);
                        (
                            id.clone(),
                            role_definition_scope_for(scope, subscription_id),
                        )
                    }),
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
        condition: Option<String>,
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
                condition_version: condition.as_ref().map(|_| "2.0".to_string()),
                condition,
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
            role_assignment_role_definition_ids: BTreeMap::new(),
            role_layout: MANAGEMENT_ROLE_LAYOUT,
            runtime_role_definition_ids: BTreeMap::new(),
            runtime_roles_revision: None,
            role_assignment_ids: vec![],
            role_assignment_wait_until_epoch_secs: None,
            management_permissions_revision: None,
            _internal_stay_count: None,
        }
    }
}

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::info;
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_azure_clients::authorization::Scope;
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_azure_clients::models::managed_identity::Identity;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureManagedIdentityServiceAccountHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, ServiceAccount, ServiceAccountHeartbeatData, ServiceAccountHeartbeatStatus,
    ServiceAccountOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::{
    generators::{
        dedupe_azure_role_bindings, AzureGrantPlan, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use chrono::Utc;
use std::collections::HashMap;

/// Generates the Azure managed identity name.
fn get_azure_managed_identity_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

/// Generates the Azure custom role name.
fn get_azure_custom_role_name(prefix: &str, name: &str) -> String {
    format!("{}-{}-role", prefix, name)
}

fn azure_custom_role_segment(key: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = true;
    for ch in key.rsplit(':').next().unwrap_or(key).chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "custom".to_string()
    } else {
        out
    }
}

#[cfg(not(test))]
const AZURE_MANAGED_IDENTITY_WAIT_SECS: u64 = 10;
#[cfg(test)]
const AZURE_MANAGED_IDENTITY_WAIT_SECS: u64 = 0;

#[cfg(not(test))]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 300;
#[cfg(test)]
const AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS: u64 = 0;

const AZURE_RBAC_WAIT_POLL_SECS: u64 = 10;
// The absolute wait deadline controls Azure RBAC propagation. Terraform-imported
// setup can drive this state quickly, so keep the Stay guard comfortably above
// the number of reconciles that can happen inside the deadline.
const AZURE_RBAC_WAIT_MAX_ATTEMPTS: u32 = 100_000;

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

fn role_definition_scope_for_assignable_scopes(
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

#[controller]
pub struct AzureServiceAccountController {
    /// The resource ID of the created user-assigned managed identity.
    pub identity_resource_id: Option<String>,
    /// The client ID of the created user-assigned managed identity.
    pub identity_client_id: Option<String>,
    /// The principal ID of the created user-assigned managed identity.
    pub(crate) identity_principal_id: Option<String>,
    /// Resource IDs of created custom role definitions.
    pub(crate) custom_role_definition_ids: Vec<String>,
    /// Resource IDs of created role assignments.
    pub(crate) role_assignment_ids: Vec<String>,
    /// Whether stack-level permissions have been applied
    pub(crate) stack_permissions_applied: bool,
    /// Deadline before using the newly created managed identity in role assignments.
    #[serde(default)]
    pub(crate) managed_identity_wait_until_epoch_secs: Option<u64>,
    /// Deadline before marking role assignments consumer-visible.
    #[serde(default)]
    pub(crate) role_assignment_wait_until_epoch_secs: Option<u64>,
}

#[controller]
impl AzureServiceAccountController {
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

        self.managed_identity_wait_until_epoch_secs = None;
        self.role_assignment_wait_until_epoch_secs = None;

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
        let created_identity = client
            .create_or_update_user_assigned_identity(
                &resource_group_name,
                &identity_name,
                &identity,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create managed identity '{}'", identity_name),
                resource_id: Some(config.id.clone()),
            })?;

        let identity_id = created_identity.id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let properties = created_identity.properties.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing properties".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let principal_id = properties.principal_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing principal ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let client_id = properties.client_id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created managed identity missing client ID".to_string(),
                operation: Some("create_managed_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            identity_name = %identity_name,
            identity_id = %identity_id,
            principal_id = %principal_id,
            client_id = %client_id,
            "Managed identity created successfully"
        );

        self.identity_resource_id = Some(identity_id);
        self.identity_client_id = Some(client_id.to_string());
        self.identity_principal_id = Some(principal_id.to_string());

        info!("Waiting for managed identity to propagate across Azure tenants");

        Ok(HandlerAction::Continue {
            state: WaitingForManagedIdentityPropagation,
            suggested_delay: None,
        })
    }

    #[handler(
        state = WaitingForManagedIdentityPropagation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_managed_identity_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let deadline = ensure_wait_deadline(
            &mut self.managed_identity_wait_until_epoch_secs,
            AZURE_MANAGED_IDENTITY_WAIT_SECS,
        );

        if let Some(delay) = wait_delay(deadline) {
            info!(
                config_id = %config.id,
                remaining_secs = deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure managed identity propagation"
            );
            return Ok(HandlerAction::Stay {
                max_times: Some(AZURE_RBAC_WAIT_MAX_ATTEMPTS),
                suggested_delay: Some(delay),
            });
        }

        self.managed_identity_wait_until_epoch_secs = None;
        Ok(HandlerAction::Continue {
            state: CreatingRoleDefinitions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingRoleDefinitions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role_definitions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            stack_permission_sets_count = config.stack_permission_sets.len(),
            "Creating Azure role definitions for stack-level permission sets"
        );

        let grant_plan = self.generate_stack_grant_plan(config, ctx)?;

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        // Parallelize role definition creation — each uses a unique
        // deterministic UUID, so there are no conflicts between them.
        let config_id = config.id.clone();
        let futures = grant_plan.custom_roles.iter().map(|custom_role| {
            let client = client.clone();
            let config_id = config_id.clone();
            let role_name = format!(
                "{}-{}",
                get_azure_custom_role_name(ctx.resource_prefix, &config.id),
                azure_custom_role_segment(&custom_role.key)
            );
            let role_def = custom_role.role_definition.clone();
            let scope = role_definition_scope_for_assignable_scopes(
                &role_def.assignable_scopes,
                &azure_cfg.subscription_id,
                &resource_group_name,
            );

            async move {
                // Deterministic UUID so re-running the same deployment updates
                // the existing role definition instead of creating a duplicate.
                let role_definition_id = Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    format!("deployment:azure:stack-role-def:{}", role_name).as_bytes(),
                )
                .to_string();

                let role_definition_props = RoleDefinitionProperties {
                    role_name: Some(role_name.clone()),
                    description: Some(role_def.description.clone()),
                    type_: Some("CustomRole".to_string()),
                    permissions: vec![Permission {
                        actions: role_def.actions.clone(),
                        not_actions: Vec::new(),
                        data_actions: role_def.data_actions.clone(),
                        not_data_actions: Vec::new(),
                    }],
                    assignable_scopes: role_def.assignable_scopes.clone(),
                    ..Default::default()
                };

                let role_definition = RoleDefinition {
                    properties: Some(role_definition_props),
                    ..Default::default()
                };

                let created_role = client
                    .create_or_update_role_definition(
                        &scope,
                        role_definition_id.clone(),
                        &role_definition,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create custom role definition '{}'", role_name),
                        resource_id: Some(config_id.clone()),
                    })?;

                let role_id = created_role.id.ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: "Created role definition missing ID".to_string(),
                        operation: Some("create_role_definition".to_string()),
                        resource_id: Some(config_id.clone()),
                    })
                })?;

                info!(
                    role_name = %role_name,
                    role_id = %role_id,
                    actions_count = role_def.actions.len(),
                    data_actions_count = role_def.data_actions.len(),
                    "Role definition created successfully"
                );

                Ok::<_, AlienError<ErrorData>>(role_id)
            }
        });

        let role_ids = futures::future::try_join_all(futures).await?;
        self.custom_role_definition_ids = role_ids;

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        let principal_id = self.identity_principal_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Managed identity principal ID not available for role assignment"
                    .to_string(),
                operation: Some("assign_role_to_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            config_id = %config.id,
            principal_id = %principal_id,
            role_definitions_count = self.custom_role_definition_ids.len(),
            "Creating role assignments for managed identity"
        );

        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        let grant_plan = self.generate_stack_grant_plan(config, ctx)?;

        // Parallelize all role assignments. Each uses a unique deterministic UUID,
        // so there are no conflicts.
        let config_id = config.id.clone();
        let principal_id = principal_id.clone();

        let custom_role_ids_by_key: HashMap<_, _> = grant_plan
            .custom_roles
            .iter()
            .map(|custom_role| custom_role.key.clone())
            .zip(self.custom_role_definition_ids.iter().cloned())
            .collect();

        let futures = grant_plan
            .bindings
            .iter()
            .enumerate()
            .map(|(binding_index, binding)| {
                let client = client.clone();
                let config_id = config_id.clone();
                let principal_id = principal_id.clone();
                let binding = binding.clone();
                let custom_role_ids_by_key = custom_role_ids_by_key.clone();

                async move {
                    let role_definition_id = match &binding.role_definition {
                        AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                            role_definition_id.clone()
                        }
                        AzureRoleDefinitionRef::Custom { key } => {
                            custom_role_ids_by_key.get(key).cloned().ok_or_else(|| {
                                AlienError::new(ErrorData::InfrastructureError {
                                    message: format!(
                                        "Custom Azure role definition '{}' was not created",
                                        key
                                    ),
                                    operation: Some("create_role_assignment".to_string()),
                                    resource_id: Some(config_id.clone()),
                                })
                            })?
                        }
                    };
                    let assignment_id = Uuid::new_v5(
                        &Uuid::NAMESPACE_OID,
                        format!(
                            "deployment:azure:stack-role-assign:{}:{}:{}",
                            binding.role_name, binding_index, principal_id
                        )
                        .as_bytes(),
                    )
                    .to_string();

                    let role_assignment = RoleAssignment {
                        id: None,
                        name: None,
                        properties: Some(RoleAssignmentProperties {
                            condition: None,
                            condition_version: None,
                            created_by: None,
                            created_on: None,
                            delegated_managed_identity_resource_id: None,
                            description: Some(format!(
                                "{} role for runtime service account {}",
                                binding.role_name, config_id
                            )),
                            principal_id: principal_id.clone(),
                            principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                            role_definition_id: role_definition_id.clone(),
                            scope: Some(binding.scope.clone()),
                            updated_by: None,
                            updated_on: None,
                        }),
                        type_: None,
                    };

                    let full_assignment_id = format!(
                        "{}/providers/Microsoft.Authorization/roleAssignments/{}",
                        binding.scope, assignment_id
                    );

                    client
                        .create_or_update_role_assignment_by_id(
                            full_assignment_id.clone(),
                            &role_assignment,
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to assign role to managed identity '{}'",
                                principal_id
                            ),
                            resource_id: Some(config_id.clone()),
                        })?;

                    info!(
                        assignment_id = %full_assignment_id,
                        role_definition_id = %role_definition_id,
                        "Role assignment created successfully"
                    );

                    Ok::<_, AlienError<ErrorData>>(full_assignment_id)
                }
            });

        self.role_assignment_ids = futures::future::try_join_all(futures).await?;

        self.stack_permissions_applied = true;

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let deadline = ensure_wait_deadline(
            &mut self.role_assignment_wait_until_epoch_secs,
            AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS,
        );

        if let Some(delay) = wait_delay(deadline) {
            info!(
                config_id = %config.id,
                remaining_secs = deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure role assignment propagation"
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
        let azure_cfg = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Heartbeat check: verify managed identity still exists
        if let Some(identity_id) = &self.identity_resource_id {
            let resource_group_name =
                crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
            let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

            let managed_identity_client = ctx
                .service_provider
                .get_azure_managed_identity_client(azure_cfg)?;
            let identity = managed_identity_client
                .get_user_assigned_identity(&resource_group_name, &identity_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get managed identity during heartbeat check".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            // Check if identity ID matches what we expect
            if let Some(fetched_id) = &identity.id {
                if !crate::infra_requirements::azure_utils::azure_resource_ids_equal(
                    identity_id,
                    fetched_id,
                ) {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "Managed identity ID changed from {} to {}",
                            identity_id, fetched_id
                        ),
                    }));
                }
            }

            emit_azure_service_account_heartbeat(
                ctx,
                &config.id,
                &resource_group_name,
                &identity_name,
                identity_id,
                &identity,
                &self.role_assignment_ids,
                &self.custom_role_definition_ids,
                self.stack_permissions_applied,
            );
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
            "Updating Azure managed identity permissions"
        );

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
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
                    info!(assignment_id = %assignment_id, "Role assignment deleted for update");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(assignment_id = %assignment_id, "Role assignment already absent for update");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete role assignment '{}' for update",
                            assignment_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        self.role_assignment_ids.clear();

        for role_definition_id in &self.custom_role_definition_ids {
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);
            let scope = role_definition_scope_from_id(role_definition_id, &resource_group_name);

            match client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_definition_id, "Role definition deleted for update");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_definition_id, "Role definition already absent for update");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete role definition '{}' for update",
                            role_definition_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        self.custom_role_definition_ids.clear();
        self.stack_permissions_applied = false;
        self.role_assignment_wait_until_epoch_secs = None;

        Ok(HandlerAction::Continue {
            state: CreatingRoleDefinitions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateWaitingForRbacPropagation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let deadline = ensure_wait_deadline(
            &mut self.role_assignment_wait_until_epoch_secs,
            AZURE_ROLE_ASSIGNMENT_RBAC_WAIT_SECS,
        );

        if let Some(delay) = wait_delay(deadline) {
            info!(
                config_id = %config.id,
                remaining_secs = deadline.saturating_sub(current_unix_timestamp_secs()),
                "Waiting for Azure role assignment propagation after update"
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
            role_assignments_count = self.role_assignment_ids.len(),
            "Starting deletion of role assignments"
        );

        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        // Delete all role assignments
        for assignment_id in &self.role_assignment_ids {
            match client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => {
                    info!(assignment_id = %assignment_id, "Role assignment deleted successfully");
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
            state: DeletingRoleDefinitions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRoleDefinitions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role_definitions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            config_id = %config.id,
            role_definitions_count = self.custom_role_definition_ids.len(),
            "Deleting custom role definitions"
        );

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;

        // Delete all role definitions
        for role_definition_id in &self.custom_role_definition_ids {
            // Extract the role definition UUID from the full ID
            let role_def_uuid = role_definition_id
                .split('/')
                .last()
                .unwrap_or(role_definition_id);
            let scope = role_definition_scope_from_id(role_definition_id, &resource_group_name);

            match client
                .delete_role_definition(&scope, role_def_uuid.to_string())
                .await
            {
                Ok(_) => {
                    info!(role_definition_id = %role_definition_id, "Role definition deleted successfully");
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_definition_id = %role_definition_id, "Role definition already deleted");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete role definition '{}'",
                            role_definition_id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
        }
        self.custom_role_definition_ids.clear();

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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let identity_name = get_azure_managed_identity_name(ctx.resource_prefix, &config.id);

        if self.identity_resource_id.is_none() {
            info!(config_id = %config.id, "No managed identity was created, nothing to delete");
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }

        let resource_group_name =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
        let azure_cfg = ctx.get_azure_config()?;
        let client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        match client
            .delete_user_assigned_identity(&resource_group_name, &identity_name)
            .await
        {
            Ok(_) => {
                info!(config_id = %config.id, identity_name = %identity_name, "Managed identity deleted successfully");
            }
            Err(e)
                if matches!(
                    &e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(config_id = %config.id, identity_name = %identity_name, "Managed identity already deleted");
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete managed identity '{}'", identity_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.identity_resource_id = None;
        self.identity_client_id = None;
        self.identity_principal_id = None;
        self.stack_permissions_applied = false;
        self.managed_identity_wait_until_epoch_secs = None;
        self.role_assignment_wait_until_epoch_secs = None;

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
        if let (Some(client_id), Some(resource_id)) =
            (&self.identity_client_id, &self.identity_resource_id)
        {
            Some(ResourceOutputs::new(ServiceAccountOutputs {
                identity: client_id.clone(),
                resource_id: resource_id.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        if let (Some(client_id), Some(resource_id), Some(principal_id)) = (
            &self.identity_client_id,
            &self.identity_resource_id,
            &self.identity_principal_id,
        ) {
            let binding = ServiceAccountBinding::azure_managed_identity(
                BindingValue::Value(client_id.clone()),
                BindingValue::Value(resource_id.clone()),
                BindingValue::Value(principal_id.clone()),
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
impl AzureServiceAccountController {
    /// Generate role definitions for all stack-level permission sets
    fn generate_stack_grant_plan(
        &self,
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<AzureGrantPlan> {
        if service_account.stack_permission_sets.is_empty() {
            return Ok(AzureGrantPlan {
                custom_roles: Vec::new(),
                bindings: Vec::new(),
            });
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let azure_config = ctx.get_azure_config()?;
        let resource_group =
            crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;

        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_subscription_id(azure_config.subscription_id.clone())
            .with_resource_group(resource_group.clone());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }

        // Compute storage account name deterministically (needed by kv/* and storage/* permission sets).
        // We can't read from state because the storage account may still be provisioning concurrently.
        let storage_account_name = crate::infra_requirements::generate_storage_account_name(
            ctx.resource_prefix,
            "default-storage-account",
        );
        permission_context = permission_context.with_storage_account_name(storage_account_name);

        // Managing subscription/resource group: used by worker/execute and compute-cluster/execute
        // permission sets for cross-tenant management. In single-subscription mode,
        // these are the same as the current subscription/resource group.
        permission_context = permission_context
            .with_managing_subscription_id(azure_config.subscription_id.clone())
            .with_managing_resource_group(resource_group);

        let mut all_custom_roles = Vec::new();
        let mut all_bindings = Vec::new();

        for permission_set in &service_account.stack_permission_sets {
            let grant_plan = generator
                .generate_grant_plan(permission_set, BindingTarget::Stack, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate grant plan for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_stack_grant_plan".to_string()),
                    resource_id: Some(service_account.id.clone()),
                })?;

            all_custom_roles.extend(grant_plan.custom_roles);
            all_bindings.extend(grant_plan.bindings);
        }

        Ok(AzureGrantPlan {
            custom_roles: all_custom_roles,
            bindings: dedupe_azure_role_bindings(all_bindings),
        })
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(identity_name: &str) -> Self {
        Self {
            state: AzureServiceAccountState::Ready,
            identity_resource_id: Some(format!("/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}", identity_name)),
            identity_client_id: Some("12345678-1234-1234-1234-123456789012".to_string()),
            identity_principal_id: Some("87654321-4321-4321-4321-210987654321".to_string()),
            custom_role_definition_ids: vec!["/subscriptions/12345678-1234-1234-1234-123456789012/providers/Microsoft.Authorization/roleDefinitions/test-role".to_string()],
            role_assignment_ids: vec!["/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.Authorization/roleAssignments/test-assignment".to_string()],
            stack_permissions_applied: true,
            managed_identity_wait_until_epoch_secs: None,
            role_assignment_wait_until_epoch_secs: None,
            _internal_stay_count: None,
        }
    }
}

fn emit_azure_service_account_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    resource_group_name: &str,
    identity_name: &str,
    expected_identity_id: &str,
    identity: &Identity,
    role_assignment_ids: &[String],
    custom_role_definition_ids: &[String],
    stack_permissions_applied: bool,
) {
    let managed_tag_count = identity
        .tags
        .keys()
        .filter(|key| key.starts_with("alien"))
        .count() as u32;
    let properties = identity.properties.as_ref();
    let message = format!("Azure managed identity '{identity_name}' is reachable");

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ServiceAccount::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ServiceAccount(
            ServiceAccountHeartbeatData::AzureManagedIdentity(
                AzureManagedIdentityServiceAccountHeartbeatData {
                    status: ServiceAccountHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: Some(message),
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    name: identity
                        .name
                        .clone()
                        .unwrap_or_else(|| identity_name.to_string()),
                    resource_id: identity
                        .id
                        .clone()
                        .unwrap_or_else(|| expected_identity_id.to_string()),
                    resource_group: resource_group_name.to_string(),
                    location: identity.location.clone(),
                    type_: identity.type_.clone(),
                    client_id: properties
                        .and_then(|properties| properties.client_id.as_ref())
                        .map(ToString::to_string),
                    principal_id: properties
                        .and_then(|properties| properties.principal_id.as_ref())
                        .map(ToString::to_string),
                    tenant_id: properties
                        .and_then(|properties| properties.tenant_id.as_ref())
                        .map(ToString::to_string),
                    isolation_scope: properties
                        .and_then(|properties| properties.isolation_scope.as_ref())
                        .map(ToString::to_string),
                    managed_tag_count,
                    role_assignment_count: role_assignment_ids.len() as u32,
                    role_assignment_ids: role_assignment_ids.to_vec(),
                    custom_role_definition_count: custom_role_definition_ids.len() as u32,
                    custom_role_definition_ids: custom_role_definition_ids.to_vec(),
                    stack_permissions_applied,
                },
            ),
        ),
        raw: vec![],
    });
}

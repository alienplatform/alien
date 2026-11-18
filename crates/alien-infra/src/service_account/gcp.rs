use std::time::Duration;
use tracing::info;

use crate::core::{GcpCustomRoleNaming, ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{
    GcpServiceAccountHeartbeatData, HeartbeatBackend, ObservedHealth, PermissionSet, Platform,
    ProviderLifecycleState, RemoteStackManagement, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceRef, ResourceStatus, ServiceAccount, ServiceAccountHeartbeatData,
    ServiceAccountHeartbeatStatus, ServiceAccountOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::iam::{
    Binding, CreateServiceAccountRequest, IamPolicy, ServiceAccount as GcpServiceAccount,
};
use alien_macros::controller;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpIamBinding, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use chrono::Utc;

/// Generates the GCP service account ID from the ServiceAccount name.
pub(crate) fn get_gcp_service_account_id(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpServiceAccountController {
    /// The email of the created service account.
    pub service_account_email: Option<String>,
    /// The unique ID of the created service account.
    pub(crate) service_account_unique_id: Option<String>,
    /// How the deployment names its custom roles, recorded with the service
    /// account. `None` for a service account created before it was recorded.
    pub(crate) custom_role_naming: Option<GcpCustomRoleNaming>,
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
        let custom_role_naming = GcpCustomRoleNaming::for_deployment(ctx.state)?;
        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;

        let service_account_id = match &config.extension {
            Some(_) => {
                alien_core::extension_identity_gcp_account_id(ctx.resource_prefix, &config.id)
            }
            None => get_gcp_service_account_id(ctx.resource_prefix, &config.id),
        };

        info!(
            service_account_id = %service_account_id,
            config_id = %config.id,
            "Creating GCP service account"
        );

        let display_name = match (&config.extension, ctx.deployment_name_for_metadata()) {
            (Some(extension), _) => alien_core::extension_identity_display_name(extension),
            (None, Some(deployment_name)) => format!("{deployment_name}: Runtime service account"),
            (None, None) => "Runtime service account".to_string(),
        };
        let description = match (&config.extension, ctx.deployment_name_for_metadata()) {
            (Some(extension), _) => format!(
                "{}. Resource prefix: {}. Resource: {}.",
                alien_core::extension_identity_display_name(extension),
                ctx.resource_prefix,
                config.id
            ),
            (None, Some(deployment_name)) => format!(
                "Runtime cloud identity for {deployment_name}. Resource prefix: {}. Resource: {}.",
                ctx.resource_prefix, config.id
            ),
            (None, None) => format!(
                "Runtime cloud identity. Resource prefix: {}. Resource: {}.",
                ctx.resource_prefix, config.id
            ),
        };
        let service_account = GcpServiceAccount::builder()
            .display_name(display_name)
            .description(description)
            .build();

        let request = CreateServiceAccountRequest::builder()
            .service_account(service_account)
            .build();

        let created_sa = client
            .create_service_account(service_account_id.clone(), request)
            .await
            .map_err(|error| {
                ResourcePermissionsHelper::gcp_service_account_creation_error(
                    error,
                    &gcp_config.project_id,
                )
            })
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
        self.custom_role_naming = Some(custom_role_naming);

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

            let service_account_policy = client
                .get_service_account_iam_policy(service_account_email.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get service account IAM policy during heartbeat check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let rm_client = ctx
                .service_provider
                .get_gcp_resource_manager_client(gcp_config)?;
            let project_policy = rm_client
                .get_project_iam_policy(
                    gcp_config.project_id.clone(),
                    Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                        requested_policy_version: Some(3),
                    }),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to get project IAM policy during service account heartbeat"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            emit_gcp_service_account_heartbeat(
                ctx,
                &config.id,
                service_account_email,
                sa,
                service_account_policy,
                project_policy,
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

        match self.custom_role_cleanup(ctx, &config.id) {
            CustomRoleCleanup::Mine => {
                ResourcePermissionsHelper::delete_gcp_custom_roles(ctx).await?;
            }
            // The cleanup owner waits for the accounts deleting beside it, which read the same
            // state in this step and so each saw another account still alive.
            CustomRoleCleanup::WaitForPeers => {
                return Ok(HandlerAction::Stay {
                    max_times: Some(150),
                    suggested_delay: Some(std::time::Duration::from_secs(2)),
                });
            }
            CustomRoleCleanup::NotMine => {}
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

#[derive(Debug, PartialEq, Eq)]
enum CustomRoleCleanup {
    Mine,
    WaitForPeers,
    NotMine,
}

/// Pure decision behind [`GcpServiceAccountController::custom_role_cleanup`].
fn decide_custom_role_cleanup(
    this_id: &str,
    peers: &[(&str, ResourceStatus)],
    has_remote_stack_management: bool,
) -> CustomRoleCleanup {
    if has_remote_stack_management {
        return CustomRoleCleanup::NotMine;
    }
    let alive: Vec<&(&str, ResourceStatus)> = peers
        .iter()
        .filter(|(_, status)| *status != ResourceStatus::Deleted)
        .collect();
    if alive.is_empty() {
        return CustomRoleCleanup::Mine;
    }
    if alive
        .iter()
        .any(|(_, status)| *status != ResourceStatus::Deleting)
    {
        return CustomRoleCleanup::NotMine;
    }
    if alive.iter().all(|(id, _)| *id < this_id) {
        CustomRoleCleanup::WaitForPeers
    } else {
        CustomRoleCleanup::NotMine
    }
}

// Separate impl block for helper methods
impl GcpServiceAccountController {
    /// Whether this deletion is the one that removes the deployment's custom roles. Setup that
    /// Alien ran itself has no remote stack management resource whose delete does it, and a
    /// setup package removes its own roles. The roles are shared by every profile, so the last
    /// service account to go removes them, after its own bindings.
    ///
    /// Accounts deleted in the same step all read the others as still alive. When every other
    /// account is being deleted too, the one with the greatest id owns the cleanup and waits
    /// until the rest are Deleted. An account that is neither Deleting nor Deleted (still
    /// running, or failed to delete) keeps the roles in place.
    fn custom_role_cleanup(
        &self,
        ctx: &ResourceControllerContext<'_>,
        this_id: &str,
    ) -> CustomRoleCleanup {
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup {
            return CustomRoleCleanup::NotMine;
        }
        let peers: Vec<(&str, ResourceStatus)> = ctx
            .state
            .resources
            .iter()
            .filter(|(id, resource)| {
                id.as_str() != this_id
                    && resource.resource_type == ServiceAccount::RESOURCE_TYPE.as_ref()
            })
            .map(|(id, resource)| (id.as_str(), resource.status))
            .collect();
        let has_remote_stack_management =
            ctx.state.resources.values().any(|r| {
                r.resource_type == alien_core::RemoteStackManagement::RESOURCE_TYPE.as_ref()
            });
        decide_custom_role_cleanup(this_id, &peers, has_remote_stack_management)
    }

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

        let permission_context = ResourcePermissionsHelper::gcp_permission_context(ctx)?
            .with_service_account_name(service_account_id.to_string());

        let mut new_bindings = Vec::new();

        for permission_set in &config.stack_permission_sets {
            self.collect_project_bindings_for_permission_set(
                ctx,
                &generator,
                permission_set,
                BindingTarget::Stack,
                &permission_context,
                &mut new_bindings,
            )
            .await?;
        }

        let legacy_profile = config
            .id
            .strip_suffix("-sa")
            .and_then(|name| ctx.desired_stack.permissions.profiles.get(name));
        let concrete = config
            .concrete_permission_sets(legacy_profile, |name| {
                alien_permissions::get_stack_permission_set(ctx.desired_stack, name).cloned()
            })
            .context(ErrorData::ResourceConfigInvalid {
                message: "Cannot resolve concrete account grants".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        for (resource_id, sets) in concrete.iter() {
            let resource_context = permission_context
                .clone()
                .with_resource_name(format!("{}-{}", ctx.resource_prefix, resource_id));
            for set in sets {
                self.collect_project_bindings_for_permission_set(
                    ctx,
                    &generator,
                    set,
                    BindingTarget::Resource,
                    &resource_context,
                    &mut new_bindings,
                )
                .await?;
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
        let owned_exact_roles = ResourcePermissionsHelper::gcp_predefined_role_names(&new_bindings);
        let mut all_bindings = current_policy.bindings;
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut all_bindings,
            new_bindings,
            &member,
            &owned_role_prefixes,
            &owned_exact_roles,
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

    async fn collect_project_bindings_for_permission_set(
        &self,
        ctx: &ResourceControllerContext<'_>,
        generator: &GcpRuntimePermissionsGenerator,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        permission_context: &PermissionContext,
        new_bindings: &mut Vec<Binding>,
    ) -> Result<()> {
        if permission_set.platforms.gcp.is_none() {
            return Ok(());
        }

        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let grant_plan = generator
            .generate_grant_plan(permission_set, binding_target, permission_context)
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to generate IAM grant plan for permission set '{}'",
                    permission_set.id
                ),
                operation: Some("sync_stack_role_bindings".to_string()),
                resource_id: Some(config.id.clone()),
            })?;
        ResourcePermissionsHelper::ensure_all_gcp_custom_roles(
            ctx,
            &permission_set.id,
            &grant_plan,
        )
        .await?;

        let project_bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
        if project_bindings.is_empty() {
            return Ok(());
        }

        for binding in project_bindings {
            new_bindings
                .push(ResourcePermissionsHelper::gcp_policy_binding_from_iam_binding(binding));
        }

        Ok(())
    }

    fn own_service_account_bindings(
        &self,
        ctx: &ResourceControllerContext<'_>,
        email: &str,
    ) -> Result<Vec<GcpIamBinding>> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let context = ResourcePermissionsHelper::gcp_permission_context(ctx)?
            .with_service_account_name(email.split('@').next().unwrap_or(email).to_string());
        let generator = GcpRuntimePermissionsGenerator::new();
        let mut bindings = Vec::new();
        let mut collect =
            |set: &PermissionSet, target, context: &PermissionContext| -> Result<()> {
                if set.platforms.gcp.is_none() {
                    return Ok(());
                }
                let plan = generator
                    .generate_grant_plan(set, target, context)
                    .context(ErrorData::ResourceConfigInvalid {
                        message: "Failed to resolve executing service-account grants".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
                for binding in plan.bindings_for_target(GcpBindingTargetScope::ServiceAccount) {
                    if !bindings.contains(&binding) {
                        bindings.push(binding);
                    }
                }
                Ok(())
            };
        for set in &config.stack_permission_sets {
            collect(set, BindingTarget::Stack, &context)?;
        }
        let legacy_profile = config
            .id
            .strip_suffix("-sa")
            .and_then(|name| ctx.desired_stack.permissions.profiles.get(name));
        let concrete = config
            .concrete_permission_sets(legacy_profile, |name| {
                alien_permissions::get_stack_permission_set(ctx.desired_stack, name).cloned()
            })
            .context(ErrorData::ResourceConfigInvalid {
                message: "Cannot resolve concrete account grants".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        for (resource_id, sets) in concrete.iter() {
            let resource_context = context
                .clone()
                .with_resource_name(format!("{}-{resource_id}", ctx.resource_prefix));
            for set in sets {
                collect(set, BindingTarget::Resource, &resource_context)?;
            }
        }
        Ok(bindings)
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
            let own_bindings = self.own_service_account_bindings(ctx, service_account_email)?;
            let explicit_target = own_bindings
                .first()
                .and_then(|binding| binding.target_resource_name.clone());
            if own_bindings.iter().any(|binding| {
                binding.target_resource_name.is_none()
                    || binding.target_resource_name != explicit_target
            }) {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Executing service-account grants must share one explicit target"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
            let manager_trust = manager_trust_binding(ctx)?;
            // The manager's trust must land on the account this controller created, never on a
            // name derived from the grants, and the grants written with it must address that
            // same account.
            let target = match (&manager_trust, explicit_target) {
                (None, Some(explicit_target)) => explicit_target,
                (Some(_), Some(explicit_target))
                    if explicit_target != *service_account_email
                        && explicit_target
                            .rsplit_once("/serviceAccounts/")
                            .is_none_or(|(_, account)| account != service_account_email) =>
                {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "The extension identity's grants target '{explicit_target}', not its own service account '{service_account_email}'"
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                _ => service_account_email.clone(),
            };
            let sa_email_owned = service_account_email.clone();
            let config_id_owned = config.id.clone();

            ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "Service account",
                "service-account",
                client,
                |client, mut iam_policy| async move {
                    iam_policy.bindings.extend(
                        own_bindings
                            .into_iter()
                            .map(ResourcePermissionsHelper::gcp_policy_binding_from_iam_binding),
                    );
                    iam_policy.bindings.extend(manager_trust);
                    client
                        .set_service_account_iam_policy(target, iam_policy)
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
            custom_role_naming: Some(GcpCustomRoleNaming::HashedLongPrefix),
            _internal_stay_count: None,
        }
    }
}

fn emit_gcp_service_account_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    service_account_email: &str,
    service_account: GcpServiceAccount,
    service_account_policy: IamPolicy,
    project_policy: IamPolicy,
) {
    let project_member = format!("serviceAccount:{service_account_email}");
    let project_roles = project_policy
        .bindings
        .iter()
        .filter(|binding| {
            binding
                .members
                .iter()
                .any(|member| member == &project_member)
        })
        .map(|binding| binding.role.clone())
        .collect::<Vec<_>>();
    let service_account_roles = service_account_policy
        .bindings
        .iter()
        .map(|binding| binding.role.clone())
        .collect::<Vec<_>>();
    let disabled = service_account.disabled;
    let health = if disabled == Some(true) {
        ObservedHealth::Unhealthy
    } else {
        ObservedHealth::Healthy
    };
    let lifecycle = if disabled == Some(true) {
        ProviderLifecycleState::Stopped
    } else {
        ProviderLifecycleState::Running
    };
    let message = if disabled == Some(true) {
        format!("GCP service account '{service_account_email}' is disabled")
    } else {
        format!("GCP service account '{service_account_email}' is reachable")
    };

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ServiceAccount::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ServiceAccount(
            ServiceAccountHeartbeatData::GcpServiceAccount(GcpServiceAccountHeartbeatData {
                status: ServiceAccountHeartbeatStatus {
                    health,
                    lifecycle,
                    message: Some(message),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: service_account.name,
                project_id: service_account.project_id,
                unique_id: service_account.unique_id,
                email: service_account
                    .email
                    .unwrap_or_else(|| service_account_email.to_string()),
                display_name: service_account.display_name,
                description: service_account.description,
                oauth2_client_id: service_account.oauth2_client_id,
                disabled,
                etag: service_account.etag,
                project_binding_count: project_roles.len() as u32,
                project_roles,
                service_account_binding_count: service_account_roles.len() as u32,
                service_account_roles,
            }),
        ),
        raw: vec![],
    });
}

/// An extension's identity lets the manager impersonate it through the management identity's
/// federation, the way the remote bindings identity does through its own.
fn manager_trust_binding(ctx: &ResourceControllerContext<'_>) -> Result<Option<Binding>> {
    let config = ctx.desired_resource_config::<ServiceAccount>()?;
    let Some(extension) = &config.extension else {
        return Ok(None);
    };
    let missing = |message: &str| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!("{message}; the identity of the extension '{extension}' trusts the manager through it"),
            resource_id: Some(config.id.clone()),
        })
    };
    let (management_id, _) = ctx
        .state
        .resources
        .iter()
        .find(|(_, state)| state.resource_type == RemoteStackManagement::RESOURCE_TYPE.as_ref())
        .ok_or_else(|| missing("The stack has no management identity"))?;
    let management = ctx
        .require_dependency::<crate::remote_stack_management::GcpRemoteStackManagementController>(
            &ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, management_id.clone()),
        )?;
    let member = management
        .federation_access
        .as_ref()
        .ok_or_else(|| missing("The management identity has no federation"))?
        .principal()
        .context(ErrorData::ResourceConfigInvalid {
            message: "Invalid management federation principal".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    Ok(Some(
        Binding::builder()
            .role("roles/iam.workloadIdentityUser".to_string())
            .members(vec![member])
            .build(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{decide_custom_role_cleanup, CustomRoleCleanup};

    #[test]
    fn accounts_deleting_together_have_one_cleanup_owner() {
        use alien_core::ResourceStatus::{Deleted, Deleting, Running};
        // Same step: both read the other as Deleting. Exactly one owns the cleanup.
        let a = decide_custom_role_cleanup("a-sa", &[("b-sa", Deleting)], false);
        let b = decide_custom_role_cleanup("b-sa", &[("a-sa", Deleting)], false);
        assert_eq!(a, CustomRoleCleanup::NotMine);
        assert_eq!(b, CustomRoleCleanup::WaitForPeers);
        // Once the peers are gone, the owner cleans up.
        assert_eq!(
            decide_custom_role_cleanup("b-sa", &[("a-sa", Deleted)], false),
            CustomRoleCleanup::Mine
        );
        // A running account keeps the roles; so does a remote management resource.
        assert_eq!(
            decide_custom_role_cleanup("b-sa", &[("a-sa", Running)], false),
            CustomRoleCleanup::NotMine
        );
        assert_eq!(
            decide_custom_role_cleanup("b-sa", &[], true),
            CustomRoleCleanup::NotMine
        );
        assert_eq!(
            decide_custom_role_cleanup("only-sa", &[], false),
            CustomRoleCleanup::Mine
        );
    }

    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_gcp_clients::iam::MockIamApi;
    use alien_gcp_clients::resource_manager::MockResourceManagerApi;
    use std::sync::Arc;

    /// A stack-wide grant of an extension's permission set resolves from the stack: Alien's own
    /// registry has no set by that id.
    #[tokio::test]
    async fn a_stack_wide_extension_grant_resolves_from_the_stack() {
        let (extension, profile) = crate::service_account::extension_with_a_stack_wide_set();
        let mut resource_manager = MockResourceManagerApi::new();
        resource_manager
            .expect_get_project_iam_policy()
            .returning(|_, _| Ok(IamPolicy::builder().version(3).bindings(vec![]).build()));
        let resource_manager = Arc::new(resource_manager);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_resource_manager_client()
            .returning(move |_| Ok(resource_manager.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(ServiceAccount::new("app-sa".to_string()).build())
            .controller(GcpServiceAccountController {
                state: GcpServiceAccountState::BindingStackRoles,
                service_account_email: Some(
                    "test-app-sa@test-project.iam.gserviceaccount.com".to_string(),
                ),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .permission_profile("app", profile)
            .extension(extension)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor
            .step()
            .await
            .unwrap_or_else(|error| panic!("the extension's set did not resolve: {error}"));
    }

    /// Direct setup lets the manager, federated through the management identity, impersonate an
    /// extension's identity, as the setup template does.
    #[tokio::test]
    async fn direct_setup_lets_the_federated_manager_impersonate_the_extension_identity() {
        let management =
            crate::remote_stack_management::GcpRemoteStackManagementController::mock_ready(
                "test-management",
            );
        let principal = management
            .federation_access
            .as_ref()
            .unwrap()
            .principal()
            .unwrap();
        let mut iam = MockIamApi::new();
        iam.expect_set_service_account_iam_policy()
            .times(1)
            .returning(move |email, policy| {
                assert_eq!(email, "test-acme-ext@mock-project.iam.gserviceaccount.com");
                let trust = policy
                    .bindings
                    .iter()
                    .filter(|binding| binding.role == "roles/iam.workloadIdentityUser")
                    .map(|binding| binding.members.clone())
                    .collect::<Vec<_>>();
                assert_eq!(trust, [vec![principal.clone()]]);
                Ok(policy)
            });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                alien_core::ServiceAccount::new("acme-ext".to_string())
                    .extension("acme".to_string())
                    .build(),
            )
            .controller(GcpServiceAccountController {
                state: GcpServiceAccountState::ApplyingResourcePermissions,
                service_account_email: Some(
                    "test-acme-ext@mock-project.iam.gserviceaccount.com".to_string(),
                ),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .with_dependency(
                RemoteStackManagement::new("management".to_string()).build(),
                management,
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<GcpServiceAccountController>()
                .unwrap()
                .state,
            GcpServiceAccountState::Ready
        );
    }

    /// The manager's trust is written on the account the controller recorded, not on a name its
    /// grants derive from the permission context.
    #[tokio::test]
    async fn the_manager_trust_is_written_on_the_extension_identitys_own_account() {
        let mut iam = MockIamApi::new();
        iam.expect_set_service_account_iam_policy()
            .times(1)
            .returning(|email, policy| {
                assert_eq!(
                    email,
                    "test-acme-ext@test-project-123.iam.gserviceaccount.com"
                );
                Ok(policy)
            });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                alien_core::ServiceAccount::new("acme-ext".to_string())
                    .stack_permission_set(
                        alien_permissions::get_permission_set("storage/data-read")
                            .unwrap()
                            .clone(),
                    )
                    .extension("acme".to_string())
                    .build(),
            )
            .controller(GcpServiceAccountController {
                state: GcpServiceAccountState::ApplyingResourcePermissions,
                service_account_email: Some(
                    "test-acme-ext@test-project-123.iam.gserviceaccount.com".to_string(),
                ),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .with_dependency(
                RemoteStackManagement::new("management".to_string()).build(),
                crate::remote_stack_management::GcpRemoteStackManagementController::mock_ready(
                    "test-management",
                ),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
    }

    /// An extension identity's grants that address another service account, here the same name
    /// in the deployment's project while the identity lives in another, are refused rather than
    /// written onto the identity's own account.
    #[tokio::test]
    async fn an_extension_identitys_grant_on_another_account_is_refused() {
        let mut iam = MockIamApi::new();
        iam.expect_set_service_account_iam_policy().times(0);
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                alien_core::ServiceAccount::new("acme-ext".to_string())
                    .stack_permission_set(
                        alien_permissions::get_permission_set("storage/data-read")
                            .unwrap()
                            .clone(),
                    )
                    .extension("acme".to_string())
                    .build(),
            )
            .controller(GcpServiceAccountController {
                state: GcpServiceAccountState::ApplyingResourcePermissions,
                service_account_email: Some(
                    "test-acme-ext@other-project.iam.gserviceaccount.com".to_string(),
                ),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .with_dependency(
                RemoteStackManagement::new("management".to_string()).build(),
                crate::remote_stack_management::GcpRemoteStackManagementController::mock_ready(
                    "test-management",
                ),
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        let error = executor.step().await.unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(
            error.message.contains(
                "projects/test-project-123/serviceAccounts/test-acme-ext@test-project-123"
            ),
            "{error:?}"
        );
    }

    /// An extension identity in a stack with no management identity has no manager to trust; a
    /// retry cannot add one.
    #[tokio::test]
    async fn an_extension_identity_without_a_management_identity_is_not_retried() {
        let mut iam = MockIamApi::new();
        iam.expect_set_service_account_iam_policy().times(0);
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                alien_core::ServiceAccount::new("acme-ext".to_string())
                    .extension("acme".to_string())
                    .build(),
            )
            .controller(GcpServiceAccountController {
                state: GcpServiceAccountState::ApplyingResourcePermissions,
                service_account_email: Some(
                    "test-acme-ext@mock-project.iam.gserviceaccount.com".to_string(),
                ),
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        let error = executor.step().await.unwrap_err();
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(!error.retryable && !error.internal, "{error:?}");
    }

    /// Direct setup names and describes an extension's identity as the setup template does.
    #[tokio::test]
    async fn direct_setup_names_the_extension_identity_as_the_template_does() {
        let mut iam = MockIamApi::new();
        iam.expect_create_service_account()
            .times(1)
            .returning(|account_id, request| {
                assert_eq!(
                    account_id,
                    alien_core::extension_identity_gcp_account_id("test", "acme-ext")
                );
                let account = request.service_account;
                assert_eq!(
                    account.display_name.as_deref(),
                    Some("Extension identity (acme)")
                );
                Ok(GcpServiceAccount {
                    email: Some(format!("{account_id}@mock-project.iam.gserviceaccount.com")),
                    unique_id: Some("123".to_string()),
                    ..Default::default()
                })
            });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                alien_core::ServiceAccount::new("acme-ext".to_string())
                    .extension("acme".to_string())
                    .build(),
            )
            .controller(GcpServiceAccountController::default())
            .platform(Platform::Gcp)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        assert_eq!(
            executor
                .internal_state::<GcpServiceAccountController>()
                .unwrap()
                .state,
            GcpServiceAccountState::BindingStackRoles
        );
    }

    /// Deleting the last service account of a direct Google Cloud setup removes the custom
    /// roles of the deployment; nothing else does, so they stayed behind after every destroy.
    /// A service account that is not the last leaves them to the one that is.
    mod custom_role_cleanup {
        use std::sync::{Arc, Mutex};

        use alien_core::{InitialSetupAuthority, Platform, ResourceLifecycle};
        use alien_gcp_clients::iam::{MockIamApi, Role};
        use alien_gcp_clients::resource_manager::MockResourceManagerApi;

        use super::*;
        use crate::core::controller_test::SingleControllerExecutor;
        use crate::core::MockPlatformServiceProvider;

        async fn delete(second_account: bool, authority: InitialSetupAuthority) -> Vec<String> {
            let namespace = GcpCustomRoleNaming::HashedLongPrefix.namespace("test");
            let own = format!("projects/test-project-123/roles/role_{namespace}_sandbox_execute");
            let foreign = format!("projects/test-project-123/roles/role_{namespace}_other");
            let deleted = Arc::new(Mutex::new(Vec::new()));
            let mut iam = MockIamApi::new();
            iam.expect_delete_service_account().returning(|_| Ok(()));
            iam.expect_list_roles().returning(move |_, _, _| {
                Ok(alien_gcp_clients::iam::ListRolesResponse::builder()
                    .roles(vec![
                        Role::builder()
                            .name(own.clone())
                            .description("Sandbox execute. Resource prefix: test.".to_string())
                            .build(),
                        // Another deployment whose prefix starts the same.
                        Role::builder()
                            .name(foreign.clone())
                            .description("Other. Resource prefix: test-prod.".to_string())
                            .build(),
                    ])
                    .build())
            });
            let sink = deleted.clone();
            iam.expect_delete_role().returning(move |name| {
                sink.lock().unwrap().push(name);
                Ok(Role::builder().build())
            });
            let mut manager = MockResourceManagerApi::new();
            manager
                .expect_get_project_iam_policy()
                .returning(|_, _| Ok(alien_gcp_clients::iam::IamPolicy::default()));
            let (iam, manager) = (Arc::new(iam), Arc::new(manager));
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_gcp_iam_client()
                .returning(move |_| Ok(iam.clone()));
            provider
                .expect_get_gcp_resource_manager_client()
                .returning(move |_| Ok(manager.clone()));

            let mut builder = SingleControllerExecutor::builder()
                // The harness's own stack already holds this profile's account, so it is the only one.
                .resource(ServiceAccount::new("default-profile-sa".to_string()).build())
                .resource_prefix("test")
                .resource_lifecycle(ResourceLifecycle::Frozen)
                .controller(GcpServiceAccountController {
                    service_account_email: Some(
                        "test-default-profile-sa@test-project-123.iam.gserviceaccount.com"
                            .to_string(),
                    ),
                    state: GcpServiceAccountState::Ready,
                    ..Default::default()
                })
                .platform(Platform::Gcp)
                .initial_setup_authority(authority)
                .service_provider(Arc::new(provider));
            if second_account {
                builder = builder.with_dependency(
                    ServiceAccount::new("other-sa".to_string()).build(),
                    GcpServiceAccountController {
                        service_account_email: Some(
                            "test-other-sa@test-project-123.iam.gserviceaccount.com".to_string(),
                        ),
                        state: GcpServiceAccountState::Ready,
                        ..Default::default()
                    },
                );
            }
            let mut executor = builder.build().await.unwrap();
            executor.delete().expect("delete is accepted");
            executor.run_until_terminal().await.expect("delete runs");
            let deleted = deleted.lock().unwrap().clone();
            deleted
        }

        #[tokio::test]
        async fn the_last_service_account_of_a_direct_setup_removes_only_its_deployments_roles() {
            let removed = delete(false, InitialSetupAuthority::DirectSetup).await;
            assert_eq!(removed.len(), 1, "{removed:?}");
            assert!(removed[0].ends_with("_sandbox_execute"), "{removed:?}");
        }

        #[tokio::test]
        async fn another_service_account_still_present_keeps_the_roles() {
            assert!(delete(true, InitialSetupAuthority::DirectSetup)
                .await
                .is_empty());
        }

        #[tokio::test]
        async fn a_setup_package_removes_its_own_roles() {
            assert!(delete(false, InitialSetupAuthority::ImportedHandoff)
                .await
                .is_empty());
        }
    }

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

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::core::ResourcePermissionsHelper;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    GcpSecretManagerVaultHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Vault, VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use chrono::Utc;

/// Attempts at the project IAM read-modify-write when another writer (another
/// vault, or the management identity) commits between our read and write.
const PROJECT_POLICY_WRITE_MAX_ATTEMPTS: u32 = 5;
const PROJECT_POLICY_WRITE_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Outcome of one project IAM read-modify-write.
enum ProjectPolicyWrite {
    /// The policy holds this vault's grants.
    Reconciled,
    /// The etag was stale: another writer committed after our read.
    ConcurrentChange(AlienError<CloudClientErrorData>),
}

/// GCP Vault controller.
///
/// GCP Secret Manager implicitly exists in every GCP project and location.
/// This controller simply sets up the vault reference without creating any infrastructure.
/// The vault represents a namespace prefix for secrets in GCP Secret Manager.
#[controller]
pub struct GcpVaultController {
    /// Revision of the vault grants successfully applied by this controller.
    #[serde(default)]
    pub(crate) permissions_revision: Option<String>,

    /// GCP project ID for the vault
    pub(crate) project_id: Option<String>,
    /// The GCP region/location for this vault
    pub(crate) location: Option<String>,
    /// The vault prefix (resource id)
    pub(crate) vault_prefix: Option<String>,
}

#[controller]
impl GcpVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            project_id = %gcp_cfg.project_id,
            location = %gcp_cfg.region,
            "Setting up GCP Secret Manager vault reference"
        );

        let vault_prefix = format!("{}-{}", ctx.resource_prefix, config.id);

        if let ProjectPolicyWrite::ConcurrentChange(error) = self
            .apply_management_permissions(ctx, &config.id, &vault_prefix)
            .await?
        {
            self.check_policy_write_attempts(&config.id, error)?;
            return Ok(HandlerAction::Stay {
                max_times: Some(PROJECT_POLICY_WRITE_MAX_ATTEMPTS),
                suggested_delay: Some(PROJECT_POLICY_WRITE_RETRY_DELAY),
            });
        }
        if ResourcePermissionsHelper::resource_is_setup_owned(ctx, &config.id)? {
            self.permissions_revision = Some(super::permissions_revision(ctx)?);
        }

        // The Secret Manager API should be enabled via infra requirements
        // Here we set up the vault reference
        self.project_id = Some(gcp_cfg.project_id.clone());
        self.location = Some(gcp_cfg.region.clone());
        self.vault_prefix = Some(vault_prefix);

        info!(
            vault_id = %config.id,
            project_id = %gcp_cfg.project_id,
            location = %gcp_cfg.region,
            vault_prefix = %self.vault_prefix.as_deref().unwrap_or("unknown"),
            "GCP Secret Manager vault is ready (implicitly exists)"
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "Reconciling GCP Secret Manager vault management permissions"
        );

        let vault_prefix = self.vault_prefix.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Vault prefix is missing from saved controller state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        if let ProjectPolicyWrite::ConcurrentChange(error) = self
            .apply_management_permissions(ctx, &config.id, vault_prefix)
            .await?
        {
            self.check_policy_write_attempts(&config.id, error)?;
            return Ok(HandlerAction::Stay {
                max_times: Some(PROJECT_POLICY_WRITE_MAX_ATTEMPTS),
                suggested_delay: Some(PROJECT_POLICY_WRITE_RETRY_DELAY),
            });
        }
        if ResourcePermissionsHelper::resource_is_setup_owned(ctx, &config.id)? {
            self.permissions_revision = Some(super::permissions_revision(ctx)?);
        }

        // No infrastructure to update - Secret Manager exists implicitly
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
        let config = ctx.desired_resource_config::<Vault>()?;

        info!(
            vault_id = %config.id,
            "Deleting GCP Secret Manager vault reference (no infrastructure to delete)"
        );

        // Clear stored values
        self.project_id = None;
        self.location = None;
        self.vault_prefix = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Vault>()?;

        // Heartbeat check: verify stored project/region haven't drifted
        if let (Some(stored_project_id), Some(stored_location)) = (&self.project_id, &self.location)
        {
            // Check for configuration drift
            if stored_project_id != &gcp_cfg.project_id {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "GCP project ID changed from {} to {}",
                        stored_project_id, gcp_cfg.project_id
                    ),
                }));
            }

            if stored_location != &gcp_cfg.region {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "GCP region changed from {} to {}",
                        stored_location, gcp_cfg.region
                    ),
                }));
            }

            debug!(project_id=%stored_project_id, location=%stored_location, "GCP Secret Manager vault heartbeat check passed");
        }

        if let (Some(project_id), Some(location), Some(vault_prefix)) =
            (&self.project_id, &self.location, &self.vault_prefix)
        {
            emit_gcp_secret_manager_vault_heartbeat(
                ctx,
                &config.id,
                project_id,
                location,
                vault_prefix,
            );
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
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

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
            || !ResourcePermissionsHelper::resource_is_setup_owned(ctx, ctx.desired_config.id())?
        {
            return Ok(false);
        }
        Ok(
            self.permissions_revision.as_deref()
                != Some(super::permissions_revision(ctx)?.as_str()),
        )
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(project_id), Some(location)) = (&self.project_id, &self.location) {
            let vault_id = format!("projects/{}/locations/{}", project_id, location);
            Some(ResourceOutputs::new(VaultOutputs { vault_id }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::VaultBinding;

        if let Some(vault_prefix) = &self.vault_prefix {
            let binding = VaultBinding::secret_manager(vault_prefix.clone());

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

fn emit_gcp_secret_manager_vault_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project_id: &str,
    location: &str,
    prefix: &str,
) {
    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Vault::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Vault(VaultHeartbeatData::GcpSecretManager(
            GcpSecretManagerVaultHeartbeatData {
                status: VaultHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(
                        "GCP Secret Manager namespace prefix is configured; secret metadata was not listed"
                            .to_string(),
                    ),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                project_id: project_id.to_string(),
                location: location.to_string(),
                prefix: prefix.to_string(),
                secret_metadata_listed: false,
            },
        )),
        raw: vec![],
    });
}

/// The condition fragment that scopes a project IAM binding to one vault's
/// secrets. Vault controllers own the management identity's bindings whose
/// condition contains it; other project-policy writers must leave them alone.
fn gcp_vault_namespace_condition(project_number: &str, vault_prefix: &str) -> String {
    format!("resource.name.startsWith(\"projects/{project_number}/secrets/{vault_prefix}-\")")
}

/// Namespace conditions of every vault in this stack, desired or still in state.
pub(crate) fn gcp_stack_vault_namespace_conditions(
    ctx: &ResourceControllerContext<'_>,
    project_number: &str,
) -> Vec<String> {
    let desired = ctx
        .desired_stack
        .resources
        .iter()
        .filter(|(_, entry)| entry.config.resource_type() == Vault::RESOURCE_TYPE)
        .map(|(id, _)| id.as_str());
    let saved = ctx
        .state
        .resources
        .iter()
        .filter(|(_, state)| state.resource_type == Vault::RESOURCE_TYPE.as_ref())
        .map(|(id, _)| id.as_str());
    let mut ids: Vec<&str> = desired.chain(saved).collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            gcp_vault_namespace_condition(project_number, &format!("{}-{id}", ctx.resource_prefix))
        })
        .collect()
}

/// Fully-qualified names of the custom roles every registered `vault/`
/// permission set binds in this project.
fn vault_custom_role_names(
    generator: &GcpRuntimePermissionsGenerator,
    permission_context: &PermissionContext,
) -> Result<std::collections::HashSet<String>> {
    let mut roles = std::collections::HashSet::new();
    for id in alien_permissions::list_permission_set_ids()
        .into_iter()
        .filter(|id| id.starts_with("vault/"))
    {
        let Some(set) = alien_permissions::get_permission_set(id) else {
            continue;
        };
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let plan = generator
                .generate_grant_plan(set, target, permission_context)
                .context(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to resolve grants of vault permission set '{id}'"),
                    resource_id: None,
                })?;
            roles.extend(plan.custom_roles.into_iter().map(|role| role.name));
        }
    }
    Ok(roles)
}

/// Roles of the unconditional project bindings that some vault grant in the
/// desired stack still produces for the management identity: resource grants
/// on any vault, and `*` grants both stack-wide and per vault.
fn unconditional_vault_roles_granted(
    ctx: &ResourceControllerContext<'_>,
    generator: &GcpRuntimePermissionsGenerator,
    permission_context: &PermissionContext,
) -> Result<std::collections::HashSet<String>> {
    let mut roles = std::collections::HashSet::new();
    let Some(profile) = ctx.desired_stack.management().profile() else {
        return Ok(roles);
    };
    for (scope, references) in &profile.0 {
        let (targets, context): (&[BindingTarget], PermissionContext) = if scope == "*" {
            (
                &[BindingTarget::Stack, BindingTarget::Resource],
                permission_context.clone(),
            )
        } else if ctx
            .desired_stack
            .resources
            .get(scope)
            .is_some_and(|entry| entry.config.resource_type() == Vault::RESOURCE_TYPE)
        {
            (
                &[BindingTarget::Resource],
                permission_context
                    .clone()
                    .with_resource_name(format!("{}-{scope}", ctx.resource_prefix)),
            )
        } else {
            continue;
        };
        for reference in references.iter().filter(|r| r.id().starts_with("vault/")) {
            let set = reference
                .resolve(|id| alien_permissions::get_permission_set(id).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Vault permission set '{}' not found", reference.id()),
                        resource_id: Some(scope.clone()),
                    })
                })?;
            for target in targets {
                let plan = generator
                    .generate_grant_plan(&set, *target, &context)
                    .context(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Failed to resolve grants of vault permission set '{}'",
                            set.id
                        ),
                        resource_id: Some(scope.clone()),
                    })?;
                roles.extend(
                    plan.bindings_for_target(GcpBindingTargetScope::Project)
                        .into_iter()
                        .filter(|binding| binding.condition.is_none())
                        .map(|binding| binding.role),
                );
            }
        }
    }
    Ok(roles)
}

pub(crate) fn binding_targets_vault_namespace(binding: &Binding, namespaces: &[String]) -> bool {
    binding.condition.as_ref().is_some_and(|condition| {
        namespaces
            .iter()
            .any(|namespace| condition.expression.contains(namespace))
    })
}

// IAM bindings and members are sets; provider ordering does not change access.
fn normalized_bindings(
    bindings: &[alien_gcp_clients::iam::Binding],
) -> std::result::Result<Vec<String>, AlienError> {
    let mut normalized = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let mut binding = binding.clone();
        binding.members.sort();
        binding.members.dedup();
        normalized.push(serde_json::to_string(&binding).into_alien_error()?);
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

impl GcpVaultController {
    /// A stale etag means another writer committed between our read and
    /// write. Re-reading and re-merging is the only correct response, so the
    /// handler stays in its state for a bounded number of attempts instead of
    /// failing the step. Any other error fails the step as usual.
    fn check_policy_write_attempts(
        &self,
        vault_id: &str,
        error: AlienError<CloudClientErrorData>,
    ) -> Result<()> {
        let attempt = self._internal_stay_count.unwrap_or_default() + 1;
        if attempt >= PROJECT_POLICY_WRITE_MAX_ATTEMPTS {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Project IAM policy kept changing concurrently; gave up binding vault management roles after {attempt} attempts"
                ),
                resource_id: Some(vault_id.to_string()),
            }));
        }
        warn!(
            vault_id = %vault_id,
            attempt,
            error = %error,
            "Project IAM policy changed concurrently; re-reading before binding vault management roles"
        );
        Ok(())
    }

    async fn apply_management_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vault_id: &str,
        vault_prefix: &str,
    ) -> Result<ProjectPolicyWrite> {
        // Project IAM grants are setup-owned, even when the vault is Live.
        if !ResourcePermissionsHelper::resource_is_setup_owned(ctx, vault_id)? {
            return Ok(ProjectPolicyWrite::Reconciled);
        }

        let mut seen_ids = std::collections::HashSet::new();
        let mut management_refs = Vec::new();
        if let Some(management_profile) = ctx.desired_stack.management().profile() {
            if let Some(permission_set_refs) = management_profile.0.get(vault_id) {
                for permission_set_ref in permission_set_refs {
                    if seen_ids.insert(permission_set_ref.id().to_string()) {
                        management_refs.push(permission_set_ref.clone());
                    }
                }
            }
            if let Some(wildcard_refs) = management_profile.0.get("*") {
                for permission_set_ref in wildcard_refs
                    .iter()
                    .filter(|r| r.id().starts_with("vault/"))
                {
                    if seen_ids.insert(permission_set_ref.id().to_string()) {
                        management_refs.push(permission_set_ref.clone());
                    }
                }
            }
        }

        let gcp_config = ctx.get_gcp_config()?;
        if management_refs.is_empty() && gcp_config.project_number.is_none() {
            let revision = super::permissions_revision(ctx)?;
            if self.permissions_revision.as_deref() == Some(revision.as_str())
                || self.state == GcpVaultState::CreateStart
            {
                // A new or unchanged empty profile has no namespace IAM work.
                return Ok(ProjectPolicyWrite::Reconciled);
            }
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "GCP project number is required to remove previous vault grants"
                    .to_string(),
                resource_id: Some(vault_id.to_string()),
            }));
        }

        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_gcp_custom_role_namespace(ResourcePermissionsHelper::gcp_custom_role_namespace(
                ctx,
            )?)
            .with_resource_name(vault_prefix.to_string());
        if let Some(deployment_name) = ctx.deployment_name_for_metadata() {
            permission_context =
                permission_context.with_deployment_name(deployment_name.to_string());
        }
        if !management_refs.is_empty() {
            let project_number = gcp_config.project_number.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "GCP project number is required to scope vault management permissions"
                        .to_string(),
                    resource_id: Some(vault_id.to_string()),
                })
            })?;
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = GcpRuntimePermissionsGenerator::new();
        let mut new_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_management_bindings_for(
            ctx,
            vault_id,
            vault_prefix,
            &management_refs,
            &generator,
            &permission_context,
            GcpBindingTargetScope::Project,
            &mut new_bindings,
        )
        .await?;

        let Some(management_sa_email) =
            ResourcePermissionsHelper::get_gcp_management_service_account_email(ctx)?
        else {
            return Ok(ProjectPolicyWrite::Reconciled);
        };

        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)?;
        let current_policy = rm_client
            .get_project_iam_policy(
                gcp_config.project_id.clone(),
                Some(GetPolicyOptions {
                    requested_policy_version: Some(3),
                }),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get project IAM policy before binding vault management roles"
                    .to_string(),
                resource_id: Some(vault_id.to_string()),
            })?;

        let member = format!("serviceAccount:{management_sa_email}");
        let owned_role_prefixes =
            ResourcePermissionsHelper::gcp_permission_set_custom_role_name_prefixes(
                &permission_context,
                std::iter::once("vault/"),
            );
        // Include predefined vault roles that a removed grant used to own.
        let mut owned_exact_roles =
            ResourcePermissionsHelper::gcp_predefined_role_names(&new_bindings);
        for id in alien_permissions::list_permission_set_ids()
            .into_iter()
            .filter(|id| id.starts_with("vault/"))
        {
            if let Some(set) = alien_permissions::get_permission_set(id) {
                for permission in set.platforms.gcp.as_deref().unwrap_or_default() {
                    owned_exact_roles
                        .extend(permission.grant.predefined_roles.iter().flatten().cloned());
                }
            }
        }
        // Different vaults share predefined roles and the management identity.
        // Reconcile only bindings whose condition targets this vault namespace.
        let project_number = gcp_config.project_number.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "GCP project number is required to reconcile vault management permissions"
                    .to_string(),
                resource_id: Some(vault_id.to_string()),
            })
        })?;
        let namespace = [gcp_vault_namespace_condition(project_number, vault_prefix)];
        let role_context = permission_context
            .clone()
            .with_project_number(project_number.to_string())
            .with_service_account_name(
                management_sa_email
                    .split('@')
                    .next()
                    .unwrap_or(&management_sa_email)
                    .to_string(),
            );
        // Vault custom role IDs come from grant labels, not the permission set
        // ID, so the prefixes above do not match them. Name them exactly.
        let vault_custom_roles = vault_custom_role_names(&generator, &role_context)?;
        owned_exact_roles.extend(vault_custom_roles.iter().cloned());
        let current_bindings = normalized_bindings(&current_policy.bindings).context(
            ErrorData::InfrastructureError {
                message: "Failed to serialize current vault IAM bindings".to_string(),
                operation: Some("compare_vault_management_bindings".to_string()),
                resource_id: Some(vault_id.to_string()),
            },
        )?;
        let (mut vault_bindings, mut all_bindings): (Vec<_>, Vec<_>) = current_policy
            .bindings
            .into_iter()
            .partition(|binding| binding_targets_vault_namespace(binding, &namespace));
        ResourcePermissionsHelper::remove_gcp_project_member_bindings(
            &mut vault_bindings,
            &member,
            Some(&owned_role_prefixes),
            Some(&owned_exact_roles),
        );
        all_bindings.extend(vault_bindings);
        // Unconditional roles can be shared by several vaults. Upsert them
        // against the full policy instead of adding a duplicate on each retry.
        ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut all_bindings,
            new_bindings,
            &member,
            &[],
            &[],
        );
        // Some vault grants cannot be scoped to a namespace (e.g. the
        // `secretmanager.secrets.create` role from `vault/data-write`), and
        // their custom roles are per stack, so every vault and the stack-wide
        // grant share one unconditional binding. Drop the member from it only
        // when no vault grant in the desired stack still produces it.
        let still_granted = unconditional_vault_roles_granted(ctx, &generator, &role_context)?;
        for binding in all_bindings.iter_mut().filter(|binding| {
            binding.condition.is_none()
                && vault_custom_roles.contains(&binding.role)
                && !still_granted.contains(&binding.role)
        }) {
            binding
                .members
                .retain(|binding_member| binding_member != &member);
        }
        all_bindings.retain(|binding| !binding.members.is_empty());
        let proposed_bindings =
            normalized_bindings(&all_bindings).context(ErrorData::InfrastructureError {
                message: "Failed to serialize desired vault IAM bindings".to_string(),
                operation: Some("compare_vault_management_bindings".to_string()),
                resource_id: Some(vault_id.to_string()),
            })?;
        if proposed_bindings == current_bindings {
            info!(vault_id = %vault_id, "GCP vault management permissions already reconciled");
            return Ok(ProjectPolicyWrite::Reconciled);
        }

        let new_policy = IamPolicy::builder()
            .version(3)
            .bindings(all_bindings)
            .maybe_etag(current_policy.etag)
            .maybe_kind(current_policy.kind)
            .maybe_resource_id(current_policy.resource_id)
            .build();

        match rm_client
            .set_project_iam_policy(gcp_config.project_id.clone(), new_policy, None)
            .await
        {
            Ok(_) => {}
            // GCP rejects a stale etag with 409 ABORTED.
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                return Ok(ProjectPolicyWrite::ConcurrentChange(error));
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to bind vault management roles at project level".to_string(),
                    resource_id: Some(vault_id.to_string()),
                }));
            }
        }

        info!(
            vault_id = %vault_id,
            vault_prefix = %vault_prefix,
            "GCP vault management permissions applied"
        );

        Ok(ProjectPolicyWrite::Reconciled)
    }
}

#[cfg(test)]
mod permission_update_tests {
    use super::*;
    use crate::core::{
        MockPlatformServiceProvider, ResourceController, StackExecutor, StackResourceStateExt,
    };
    use crate::remote_stack_management::GcpRemoteStackManagementController;
    use alien_client_core::ErrorData as CloudError;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{
        ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
        GcpClientConfig, InitialSetupAuthority, RemoteStackManagement, Resource, ResourceLifecycle,
        ResourceRef, Stack, StackResourceState, StackSettings, StackState,
    };
    use alien_gcp_clients::{resource_manager::MockResourceManagerApi, GcpClientConfigExt as _};
    use sha2::Digest;
    use std::sync::{Arc, Mutex};

    // Simulate a committed project policy whose response is lost.
    // Unexpected provider calls fail the mock.
    fn fixture(
        lifecycle: ResourceLifecycle,
        authority: InitialSetupAuthority,
        writes: usize,
        lose_first_response: bool,
    ) -> (StackExecutor, StackState, Arc<Mutex<Vec<String>>>) {
        let policies = Arc::new(Mutex::new(Vec::new()));
        let saved = policies.clone();
        let mut manager = MockResourceManagerApi::new();
        let remote_policy = Arc::new(Mutex::new(
            IamPolicy::builder()
                .bindings(vec![alien_gcp_clients::iam::Binding {
                    role: "roles/secretmanager.secretAccessor".to_string(),
                    members: vec!["serviceAccount:manager@mock-project.iam.gserviceaccount.com".to_string()],
                    condition: Some(alien_gcp_clients::iam::Expr {
                        expression: "resource.name.startsWith(\"projects/123456789012/secrets/test-other-\")".to_string(),
                        title: Some("ResourceVaultSecretsRead".to_string()),
                        description: None, location: None,
                    }),
                }])
                .etag("test-etag".to_string())
                .build(),
        ));
        let read = remote_policy.clone();
        manager
            .expect_get_project_iam_policy()
            .times(writes)
            .returning(move |_, options| {
                assert_eq!(options.unwrap().requested_policy_version, Some(3));
                Ok(read.lock().unwrap().clone())
            });
        manager
            .expect_set_project_iam_policy()
            .times(if lose_first_response && writes > 0 {
                1
            } else {
                writes
            })
            .returning(move |_, policy, _| {
                assert_eq!(policy.etag.as_deref(), Some("test-etag"));
                assert_eq!(policy.bindings.len(), 3);
                assert_eq!(
                    policy.bindings[0].condition.as_ref().unwrap().expression,
                    "resource.name.startsWith(\"projects/123456789012/secrets/test-other-\")"
                );
                for binding in &policy.bindings[1..] {
                    assert!(matches!(
                        binding.role.as_str(),
                        "roles/secretmanager.viewer" | "roles/secretmanager.secretAccessor"
                    ));
                    assert_eq!(
                        binding.members,
                        vec!["serviceAccount:manager@mock-project.iam.gserviceaccount.com"]
                    );
                    assert!(binding
                        .condition
                        .as_ref()
                        .unwrap()
                        .expression
                        .contains("projects/123456789012/secrets/test-secrets-"));
                }
                let mut committed = policy.clone();
                if lose_first_response {
                    // A concurrent writer reorders the policy and shares a binding.
                    committed.bindings[1].members.push(
                        "serviceAccount:other@mock-project.iam.gserviceaccount.com".to_string(),
                    );
                    committed.bindings.reverse();
                }
                *remote_policy.lock().unwrap() = committed;
                saved
                    .lock()
                    .unwrap()
                    .push(serde_json::to_string(&policy).unwrap());
                if lose_first_response && saved.lock().unwrap().len() == 1 {
                    return Err(AlienError::new(CloudError::HttpRequestFailed {
                        message: "Connection closed after the policy write".to_string(),
                    }));
                }
                Ok(policy)
            });
        let manager = Arc::new(manager);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_resource_manager_client()
            .times(writes)
            .returning(move |_| Ok(manager.clone()));
        let vault = Vault::new("secrets".to_string()).build();
        let account = RemoteStackManagement::new("manager".to_string()).build();
        let stack = Stack::new("test".to_string())
            .add_with_dependencies(
                vault.clone(),
                lifecycle,
                vec![ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    "manager",
                )],
            )
            .add(account.clone(), ResourceLifecycle::Frozen)
            .management(alien_core::ManagementPermissions::Extend(
                PermissionProfile::new().resource("secrets", ["vault/data-read"]),
            ))
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .build();
        let mut client = GcpClientConfig::mock();
        client.project_number = Some("123456789012".to_string());
        let provider: Arc<dyn crate::core::PlatformServiceProvider> = Arc::new(provider);
        let executor = StackExecutor::builder(&stack, ClientConfig::Gcp(Box::new(client)))
            .deployment_config(&config)
            .service_provider(provider.clone())
            .initial_setup_authority(authority)
            .step_running_resources(false)
            .build()
            .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Gcp, "test".to_string());
        let controller = GcpVaultController {
            state: GcpVaultState::Ready,
            project_id: Some("mock-project".to_string()),
            location: Some("us-central1".to_string()),
            vault_prefix: Some("test-secrets".to_string()),
            ..Default::default()
        };
        let mut vault_state = StackResourceState::new_pending(
            Vault::RESOURCE_TYPE.to_string(),
            Resource::new(vault),
            Some(lifecycle),
            vec![],
        );
        vault_state.status = ResourceStatus::Running;
        vault_state.outputs = controller.get_outputs();
        vault_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("secrets".to_string(), vault_state);
        // Management is ready, but the vault has not recorded its new
        // dependency or applied its explicit secret-read grant.
        let mut controller = GcpRemoteStackManagementController::mock_ready("manager");
        // Setup has already converged the management identity. This update is
        // specifically the vault's new dependency, not a stale identity revision.
        let registry = Arc::new(crate::core::ResourceRegistry::default());
        let desired_config = Resource::new(account.clone());
        controller.management_permissions_revision =
            crate::remote_stack_management::management_permissions_revision(
                &ResourceControllerContext {
                    desired_config: &desired_config,
                    platform: Platform::Gcp,
                    client_config: ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
                    state: &state,
                    resource_prefix: "test",
                    registry: &registry,
                    desired_stack: &stack,
                    service_provider: &provider,
                    deployment_config: &config,
                    initial_setup_authority: authority,
                    heartbeat_collector: crate::core::HeartbeatCollector::default(),
                },
            )
            .unwrap();
        let mut account_state = StackResourceState::new_pending(
            RemoteStackManagement::RESOURCE_TYPE.to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        account_state.status = ResourceStatus::Running;
        account_state.outputs = controller.get_outputs();
        account_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("manager".to_string(), account_state);
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("manager"));
        (executor, state, policies)
    }

    #[tokio::test]
    async fn setup_update_grants_existing_vault_access_to_consumer() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            1,
            false,
        );
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(policies.lock().unwrap().len(), 1);
        // Repeating setup does not schedule another update after convergence.
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn empty_management_grants_do_not_require_a_project_number() {
        for (operation, known_empty) in [
            (GcpVaultState::CreateStart, false),
            (GcpVaultState::UpdateStart, true),
            (GcpVaultState::UpdateStart, false),
        ] {
            let vault = Vault::new("secrets".to_string()).build();
            let manager = RemoteStackManagement::new("manager".to_string()).build();
            let stack = Stack::new("test".to_string())
                .add_with_dependencies(
                    vault.clone(),
                    ResourceLifecycle::Frozen,
                    vec![ResourceRef::new(
                        RemoteStackManagement::RESOURCE_TYPE,
                        "manager",
                    )],
                )
                .add(manager.clone(), ResourceLifecycle::Frozen)
                .build();
            // Test the empty Auto profile before any setup preflight adds grants.
            let config = DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(ExternalBindings::default())
                .allow_frozen_changes(true)
                .build();
            let executor = StackExecutor::builder(
                &stack,
                ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
            )
            .deployment_config(&config)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .step_running_resources(false)
            .build()
            .unwrap();
            let mut state = StackState::with_resource_prefix(Platform::Gcp, "test".to_string());
            let mut vault_state = StackResourceState::new_pending(
                Vault::RESOURCE_TYPE.to_string(),
                Resource::new(vault),
                Some(ResourceLifecycle::Frozen),
                vec![ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    "manager",
                )],
            );
            vault_state.status = if operation == GcpVaultState::CreateStart {
                ResourceStatus::Provisioning
            } else {
                ResourceStatus::Updating
            };
            vault_state
                .set_internal_controller(Some(Box::new(GcpVaultController {
                    state: operation.clone(),
                    permissions_revision: known_empty
                        .then(|| format!("{:x}", sha2::Sha256::digest(b"[]"))),
                    project_id: Some("mock-project".to_string()),
                    location: Some("us-central1".to_string()),
                    vault_prefix: Some("test-secrets".to_string()),
                    ..Default::default()
                })))
                .unwrap();
            state.resources.insert("secrets".to_string(), vault_state);
            let controller = GcpRemoteStackManagementController::mock_ready("manager");
            let mut manager_state = StackResourceState::new_pending(
                RemoteStackManagement::RESOURCE_TYPE.to_string(),
                Resource::new(manager),
                Some(ResourceLifecycle::Frozen),
                vec![],
            );
            manager_state.status = ResourceStatus::Running;
            manager_state.outputs = controller.get_outputs();
            manager_state
                .set_internal_controller(Some(Box::new(controller)))
                .unwrap();
            state.resources.insert("manager".to_string(), manager_state);
            let state = executor.step(state).await.unwrap().next_state;
            if operation == GcpVaultState::UpdateStart && !known_empty {
                // Old checkpoints may have grants even without a saved revision.
                assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
                assert!(state.resources["secrets"]
                    .error
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("required to remove previous vault grants"));
                let controller = state.resources["secrets"]
                    .get_internal_controller_typed::<GcpVaultController>()
                    .unwrap();
                assert!(controller.permissions_revision.is_none());
                continue;
            }
            assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
            assert!(!executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("secrets"));
        }
    }

    #[tokio::test]
    async fn permission_only_update_reconciles_and_then_converges() {
        let (executor, mut state, writes) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            1,
            false,
        );
        let resource = state.resources.get_mut("secrets").unwrap();
        resource.dependencies = vec![ResourceRef::new(
            RemoteStackManagement::RESOURCE_TYPE,
            "manager",
        )];
        let mut controller = resource
            .get_internal_controller_typed::<GcpVaultController>()
            .unwrap();
        controller.permissions_revision = Some("previous-grants".to_string());
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn previous_checkpoint_without_revision_reconciles_once() {
        let (executor, mut state, writes) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            1,
            false,
        );
        assert!(state
            .resources
            .get_mut("secrets")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("permissionsRevision")
            .is_some());
        let state: StackState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("secrets"));
    }

    #[tokio::test]
    async fn imported_vault_update_refuses_permission_writes() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::ImportedHandoff,
            0,
            false,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"]
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("rerun setup"));
        assert!(policies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn live_vault_update_leaves_setup_owned_iam_untouched() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Live,
            InitialSetupAuthority::ImportedHandoff,
            0,
            false,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(policies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_refuses_missing_saved_vault_prefix() {
        let (executor, mut state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            0,
            false,
        );
        let resource = state.resources.get_mut("secrets").unwrap();
        let mut controller = resource
            .get_internal_controller_typed::<GcpVaultController>()
            .unwrap();
        controller.vault_prefix = None;
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"]
            .error
            .as_ref()
            .unwrap()
            .to_string()
            .contains("Vault prefix is missing"));
        assert!(policies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn lost_response_resumes_the_saved_update_and_adopts_the_committed_policy() {
        let (executor, state, policies) = fixture(
            ResourceLifecycle::Frozen,
            InitialSetupAuthority::DirectSetup,
            2,
            true,
        );
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(state.resources["secrets"].status, ResourceStatus::Running);
        assert!(state.resources["secrets"].error.is_some());
        // Reload the durable checkpoint and drive the executor's actual retry.
        let state: StackState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(state.resources["secrets"].status, ResourceStatus::Running);
        let policies = policies.lock().unwrap();
        assert_eq!(
            policies.len(),
            1,
            "retry adopts the committed policy without a second write"
        );
    }
}

/// Vaults and the management identity all read-modify-write the same project
/// IAM policy during one setup run. These tests drive the real controllers
/// against an in-memory policy with GCP's etag semantics.
#[cfg(test)]
mod project_policy_writer_tests {
    use super::*;
    use crate::core::{
        MockPlatformServiceProvider, ResourceController, StackExecutor, StackResourceStateExt,
    };
    use crate::remote_stack_management::GcpRemoteStackManagementController;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{
        ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
        GcpClientConfig, GcpManagementConfig, InitialSetupAuthority, ManagementConfig,
        RemoteStackManagement, Resource, ResourceLifecycle, ResourceRef, Stack, StackResourceState,
        StackSettings, StackState,
    };
    use alien_gcp_clients::iam::{Expr, MockIamApi};
    use alien_gcp_clients::{resource_manager::MockResourceManagerApi, GcpClientConfigExt as _};
    use std::sync::{Arc, Mutex};

    const PROJECT_NUMBER: &str = "123456789012";
    const MANAGER: &str = "serviceAccount:manager@test-project-123.iam.gserviceaccount.com";
    const OTHER_WRITER: &str = "serviceAccount:other@mock-project.iam.gserviceaccount.com";

    /// Project IAM policy with etags. `conflicts` makes that many writes lose
    /// a race: another writer commits a binding first, so the write's etag is
    /// stale and GCP answers 409.
    #[derive(Default)]
    struct Project {
        bindings: Vec<Binding>,
        version: u64,
        conflicts: usize,
        writes: usize,
    }

    impl Project {
        fn etag(&self) -> String {
            format!("etag-{}", self.version)
        }

        fn has(&self, role: &str, member: &str, condition: Option<&str>) -> bool {
            self.bindings.iter().any(|binding| {
                binding.role == role
                    && binding.members.iter().any(|m| m == member)
                    && match (condition, &binding.condition) {
                        (None, None) => true,
                        (Some(fragment), Some(expr)) => expr.expression.contains(fragment),
                        _ => false,
                    }
            })
        }

        /// Custom roles bound to `member` without a condition. Here these are
        /// the stack's `vault/data-write` create role.
        fn unconditional_custom_roles(&self, member: &str) -> Vec<String> {
            self.bindings
                .iter()
                .filter(|binding| {
                    binding.condition.is_none()
                        && binding.role.starts_with("projects/")
                        && binding.members.iter().any(|m| m == member)
                })
                .map(|binding| binding.role.clone())
                .collect()
        }

        /// The management member's bindings scoped to `vault_prefix`'s secrets.
        fn vault_grants(&self, vault_prefix: &str) -> Vec<String> {
            let namespace = gcp_vault_namespace_condition(PROJECT_NUMBER, vault_prefix);
            let mut grants: Vec<String> = self
                .bindings
                .iter()
                .filter(|binding| binding.members.iter().any(|m| m == MANAGER))
                .filter_map(|binding| {
                    let condition = binding.condition.as_ref()?;
                    condition.expression.contains(&namespace).then(|| {
                        format!(
                            "{} {}",
                            binding.role,
                            condition.title.clone().unwrap_or_default()
                        )
                    })
                })
                .collect();
            grants.sort();
            grants
        }
    }

    fn other_vault_binding() -> Binding {
        Binding {
            role: "roles/secretmanager.secretAccessor".to_string(),
            members: vec![OTHER_WRITER.to_string()],
            condition: Some(Expr {
                expression: gcp_vault_namespace_condition(PROJECT_NUMBER, "test-other"),
                title: Some("ResourceVaultSecretsRead".to_string()),
                description: None,
                location: None,
            }),
        }
    }

    fn resource_manager(project: Arc<Mutex<Project>>) -> MockResourceManagerApi {
        let mut manager = MockResourceManagerApi::new();
        let read = project.clone();
        manager
            .expect_get_project_iam_policy()
            .returning(move |_, _| {
                let project = read.lock().unwrap();
                Ok(IamPolicy::builder()
                    .version(3)
                    .bindings(project.bindings.clone())
                    .etag(project.etag())
                    .build())
            });
        manager
            .expect_set_project_iam_policy()
            .returning(move |_, policy, _| {
                let mut project = project.lock().unwrap();
                project.writes += 1;
                if project.conflicts > 0 {
                    project.conflicts -= 1;
                    project.bindings.push(other_vault_binding());
                    project.version += 1;
                }
                if policy.etag.as_deref() != Some(project.etag().as_str()) {
                    return Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceConflict {
                            resource_type: "Project IAM policy".to_string(),
                            resource_name: "mock-project".to_string(),
                            message: "There were concurrent policy changes".to_string(),
                        },
                    ));
                }
                project.bindings = policy.bindings.clone();
                project.version += 1;
                Ok(IamPolicy {
                    etag: Some(project.etag()),
                    ..policy
                })
            });
        manager
    }

    fn fixture(project: Arc<Mutex<Project>>) -> (StackExecutor, StackState) {
        // `vault/heartbeat` on `*` makes the management identity hold
        // `roles/secretmanager.viewer` both project-wide (its own grant) and
        // on this vault's namespace (the vault's grant).
        fixture_with(
            project,
            PermissionProfile::new()
                .global(["vault/heartbeat"])
                .resource("app-secrets", ["vault/data-read"]),
            &[],
        )
    }

    /// `vaults` lists extra Frozen vaults, saved as Ready like `app-secrets`.
    fn fixture_with(
        project: Arc<Mutex<Project>>,
        management: PermissionProfile,
        vaults: &[&str],
    ) -> (StackExecutor, StackState) {
        let manager = Arc::new(resource_manager(project));
        let mut iam = MockIamApi::new();
        // Setup repairs the stack's vault custom roles before binding them.
        iam.expect_get_role().returning(|name| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "IAM role".to_string(),
                    resource_name: name,
                },
            ))
        });
        iam.expect_create_role()
            .returning(|_, request| Ok(request.role));
        iam.expect_get_service_account_iam_policy()
            .returning(|_| Ok(IamPolicy::builder().etag("sa-etag".to_string()).build()));
        iam.expect_set_service_account_iam_policy()
            .returning(|_, policy| Ok(policy));
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_resource_manager_client()
            .returning(move |_| Ok(manager.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        // Ids sort the vault before the management identity, so in a step
        // that runs both, the identity's policy write lands last, as in the
        // setup run where this was found.
        let ids: Vec<&str> = std::iter::once("app-secrets")
            .chain(vaults.iter().copied())
            .collect();
        let account = RemoteStackManagement::new("manager".to_string()).build();
        let mut stack = Stack::new("test".to_string());
        for id in &ids {
            stack = stack.add_with_dependencies(
                Vault::new(id.to_string()).build(),
                ResourceLifecycle::Frozen,
                vec![ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    "manager",
                )],
            );
        }
        let stack = stack
            .add(account.clone(), ResourceLifecycle::Frozen)
            .management(alien_core::ManagementPermissions::Extend(management))
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(true)
            .management_config(ManagementConfig::Gcp(GcpManagementConfig {
                service_account_email: "control-plane@vendor.iam.gserviceaccount.com".to_string(),
            }))
            .build();
        let mut client = GcpClientConfig::mock();
        client.project_number = Some(PROJECT_NUMBER.to_string());
        let provider: Arc<dyn crate::core::PlatformServiceProvider> = Arc::new(provider);
        let executor = StackExecutor::builder(&stack, ClientConfig::Gcp(Box::new(client.clone())))
            .deployment_config(&config)
            .service_provider(provider.clone())
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .step_running_resources(false)
            .build()
            .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Gcp, "test".to_string());
        for id in &ids {
            let controller = GcpVaultController {
                state: GcpVaultState::Ready,
                project_id: Some("mock-project".to_string()),
                location: Some("us-central1".to_string()),
                vault_prefix: Some(format!("test-{id}")),
                ..Default::default()
            };
            let mut vault_state = StackResourceState::new_pending(
                Vault::RESOURCE_TYPE.to_string(),
                Resource::new(Vault::new(id.to_string()).build()),
                Some(ResourceLifecycle::Frozen),
                vec![ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    "manager",
                )],
            );
            vault_state.status = ResourceStatus::Running;
            vault_state.outputs = controller.get_outputs();
            vault_state
                .set_internal_controller(Some(Box::new(controller)))
                .unwrap();
            state.resources.insert(id.to_string(), vault_state);
        }
        // The management identity starts converged with the desired stack.
        let mut controller = GcpRemoteStackManagementController::mock_ready("manager");
        // The identity's own grants name it by the mock client's project.
        controller.service_account_email =
            Some("manager@test-project-123.iam.gserviceaccount.com".to_string());
        let registry = Arc::new(crate::core::ResourceRegistry::default());
        let desired_config = Resource::new(account.clone());
        controller.management_permissions_revision =
            crate::remote_stack_management::management_permissions_revision(
                &ResourceControllerContext {
                    desired_config: &desired_config,
                    platform: Platform::Gcp,
                    client_config: ClientConfig::Gcp(Box::new(client)),
                    state: &state,
                    resource_prefix: "test",
                    registry: &registry,
                    desired_stack: &stack,
                    service_provider: &provider,
                    deployment_config: &config,
                    initial_setup_authority: InitialSetupAuthority::DirectSetup,
                    heartbeat_collector: crate::core::HeartbeatCollector::default(),
                },
            )
            .unwrap();
        let mut account_state = StackResourceState::new_pending(
            RemoteStackManagement::RESOURCE_TYPE.to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        account_state.status = ResourceStatus::Running;
        account_state.outputs = controller.get_outputs();
        account_state
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        state.resources.insert("manager".to_string(), account_state);
        (executor, state)
    }

    /// Saved grants that differ from the desired stack, so setup schedules
    /// the management identity's update.
    fn force_management_update(state: &mut StackState) {
        let resource = state.resources.get_mut("manager").unwrap();
        let mut controller = resource
            .get_internal_controller_typed::<GcpRemoteStackManagementController>()
            .unwrap();
        controller.management_permissions_revision = Some("previous-grants".to_string());
        resource
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
    }

    async fn step_until_settled(executor: &StackExecutor, mut state: StackState) -> StackState {
        for _ in 0..10 {
            let plan = executor.plan(&state).unwrap();
            let busy = state.resources.values().any(|resource| {
                !matches!(
                    resource.status,
                    ResourceStatus::Running
                        | ResourceStatus::UpdateFailed
                        | ResourceStatus::ProvisionFailed
                )
            });
            if !busy && plan.updates.is_empty() {
                return state;
            }
            state = executor.step(state).await.unwrap().next_state;
        }
        panic!("stack did not settle in 10 steps");
    }

    #[tokio::test]
    async fn concurrent_policy_change_rereads_and_keeps_the_other_writers_grant() {
        let project = Arc::new(Mutex::new(Project {
            conflicts: 1,
            ..Default::default()
        }));
        let (executor, state) = fixture(project.clone());
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("app-secrets"));

        // The first write loses the race. The step stays in its state instead
        // of failing, and records nothing.
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(
            state.resources["app-secrets"].status,
            ResourceStatus::Updating
        );
        assert!(state.resources["app-secrets"].error.is_none());
        let controller = state.resources["app-secrets"]
            .get_internal_controller_typed::<GcpVaultController>()
            .unwrap();
        assert_eq!(controller.state, GcpVaultState::UpdateStart);
        assert!(controller.permissions_revision.is_none());

        // The checkpoint resumes: it re-reads, merges, and writes once more.
        let state: StackState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let state = executor.step(state).await.unwrap().next_state;
        assert_eq!(
            state.resources["app-secrets"].status,
            ResourceStatus::Running
        );
        let project = project.lock().unwrap();
        assert_eq!(project.writes, 2);
        assert!(
            project
                .bindings
                .iter()
                .any(|b| b.members.contains(&OTHER_WRITER.to_string())),
            "the concurrent writer's binding survives the retry"
        );
        assert!(project.has(
            "roles/secretmanager.secretAccessor",
            MANAGER,
            Some(&gcp_vault_namespace_condition(
                PROJECT_NUMBER,
                "test-app-secrets"
            )),
        ));
        assert!(!executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("app-secrets"));
    }

    #[tokio::test]
    async fn policy_that_keeps_changing_fails_the_step_after_bounded_attempts() {
        let project = Arc::new(Mutex::new(Project {
            conflicts: usize::MAX,
            ..Default::default()
        }));
        let (executor, mut state) = fixture(project.clone());
        for attempt in 1..PROJECT_POLICY_WRITE_MAX_ATTEMPTS {
            state = executor.step(state).await.unwrap().next_state;
            assert_eq!(
                state.resources["app-secrets"].status,
                ResourceStatus::Updating,
                "attempt {attempt} stays"
            );
        }
        // The last attempt fails the step; the executor's own retry policy
        // takes over from here.
        let state = executor.step(state).await.unwrap().next_state;
        assert_ne!(
            state.resources["app-secrets"].status,
            ResourceStatus::Running
        );
        let error = state.resources["app-secrets"]
            .error
            .as_ref()
            .unwrap()
            .to_string();
        assert!(error.contains("kept changing concurrently"), "{error}");
        assert_eq!(
            project.lock().unwrap().writes,
            PROJECT_POLICY_WRITE_MAX_ATTEMPTS as usize
        );
        let controller = state.resources["app-secrets"]
            .get_internal_controller_typed::<GcpVaultController>()
            .unwrap();
        assert!(controller.permissions_revision.is_none());
    }

    #[tokio::test]
    async fn management_update_after_the_vault_keeps_the_vaults_grants() {
        let project = Arc::new(Mutex::new(Project::default()));
        let (executor, state) = fixture(project.clone());

        // The vault reconciles first, as in a setup run where its update
        // finishes before the management identity's.
        let state = step_until_settled(&executor, state).await;
        assert_eq!(
            state.resources["app-secrets"].status,
            ResourceStatus::Running
        );
        let vault_grants = project.lock().unwrap().vault_grants("test-app-secrets");
        // The conditional viewer is the binding a management reconcile would
        // strip, because the identity also holds viewer project-wide.
        assert!(
            vault_grants
                .iter()
                .any(|grant| grant.starts_with("roles/secretmanager.viewer ")),
            "{vault_grants:?}"
        );

        // Then the management identity's update commits last.
        let mut state = state;
        force_management_update(&mut state);
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("manager"));
        let state = step_until_settled(&executor, state).await;
        assert_eq!(state.resources["manager"].status, ResourceStatus::Running);

        let project = project.lock().unwrap();
        assert!(project.has("roles/secretmanager.viewer", MANAGER, None));
        assert_eq!(project.vault_grants("test-app-secrets"), vault_grants);
        // Both writers' revisions are truthful: nothing is rescheduled.
        let plan = executor.plan(&state).unwrap();
        assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    }

    fn management(app_secrets: &[&'static str], accounts: &[&'static str]) -> PermissionProfile {
        let mut profile = PermissionProfile::new()
            .global(["vault/heartbeat"])
            .resource("app-secrets", app_secrets.to_vec());
        if !accounts.is_empty() {
            profile = profile.resource("accounts", accounts.to_vec());
        }
        profile
    }

    #[tokio::test]
    async fn removed_write_grant_drops_the_shared_unconditional_binding() {
        let project = Arc::new(Mutex::new(Project::default()));
        let granted = management(&["vault/data-read", "vault/data-write"], &[]);
        let (executor, state) = fixture_with(project.clone(), granted, &[]);
        let state = step_until_settled(&executor, state).await;
        let write_role = {
            let mut project = project.lock().unwrap();
            let roles = project.unconditional_custom_roles(MANAGER);
            assert_eq!(roles.len(), 1, "{:#?}", project.bindings);
            // Another principal on the same role is not this vault's to remove.
            let binding = project
                .bindings
                .iter_mut()
                .find(|binding| binding.role == roles[0] && binding.condition.is_none())
                .unwrap();
            binding.members.push(OTHER_WRITER.to_string());
            roles[0].clone()
        };

        // The next release drops the write grant.
        let (executor, _) =
            fixture_with(project.clone(), management(&["vault/data-read"], &[]), &[]);
        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("app-secrets"));
        let state = step_until_settled(&executor, state).await;
        assert_eq!(
            state.resources["app-secrets"].status,
            ResourceStatus::Running
        );

        let project = project.lock().unwrap();
        assert!(project.unconditional_custom_roles(MANAGER).is_empty());
        assert!(project.has(&write_role, OTHER_WRITER, None));
        let grants = project.vault_grants("test-app-secrets");
        // The namespace-scoped half of the write grant (a predefined role and a
        // label-named custom role) goes too; the read grant stays.
        assert!(
            !grants
                .iter()
                .any(|grant| grant.contains("secretVersionAdder")
                    || grant.contains("write_secret_manager_values")),
            "{grants:?}"
        );
        assert!(grants
            .iter()
            .any(|grant| grant.starts_with("roles/secretmanager.secretAccessor ")));
        let plan = executor.plan(&state).unwrap();
        assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    }

    #[tokio::test]
    async fn write_grant_another_vault_still_needs_keeps_the_shared_binding() {
        let project = Arc::new(Mutex::new(Project::default()));
        let granted = management(
            &["vault/data-read", "vault/data-write"],
            &["vault/data-write"],
        );
        let (executor, state) = fixture_with(project.clone(), granted, &["accounts"]);
        let state = step_until_settled(&executor, state).await;
        let roles = project.lock().unwrap().unconditional_custom_roles(MANAGER);
        assert_eq!(roles.len(), 1);

        // `app-secrets` drops its write grant and, sorted after `accounts`,
        // writes last; `accounts` still needs the shared create role.
        let (executor, _) = fixture_with(
            project.clone(),
            management(&["vault/data-read"], &["vault/data-write"]),
            &["accounts"],
        );
        let state = step_until_settled(&executor, state).await;
        assert_eq!(
            state.resources["app-secrets"].status,
            ResourceStatus::Running
        );
        assert_eq!(
            project.lock().unwrap().unconditional_custom_roles(MANAGER),
            roles
        );
        let plan = executor.plan(&state).unwrap();
        assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    }
}

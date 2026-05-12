use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::core::ResourcePermissionsHelper;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Vault, VaultOutputs};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_permissions::{generators::GcpRuntimePermissionsGenerator, PermissionContext};

/// GCP Vault controller.
///
/// GCP Secret Manager implicitly exists in every GCP project and location.
/// This controller simply sets up the vault reference without creating any infrastructure.
/// The vault represents a namespace prefix for secrets in GCP Secret Manager.
#[controller]
pub struct GcpVaultController {
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

        ResourcePermissionsHelper::ensure_gcp_resource_custom_roles(
            ctx,
            &config.id,
            &vault_prefix,
            "vault",
        )
        .await?;
        self.apply_management_permissions(ctx, &config.id, &vault_prefix)
            .await?;

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
            "GCP Secret Manager vault update complete (no infrastructure to update)"
        );

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

impl GcpVaultController {
    async fn apply_management_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vault_id: &str,
        vault_prefix: &str,
    ) -> Result<()> {
        let Some(management_profile) = ctx.desired_stack.management().profile() else {
            return Ok(());
        };

        let mut seen_ids = std::collections::HashSet::new();
        let mut management_refs = Vec::new();
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

        if management_refs.is_empty() {
            return Ok(());
        }

        let gcp_config = ctx.get_gcp_config()?;
        let project_number = gcp_config.project_number.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "GCP project number is required to scope vault management permissions"
                    .to_string(),
                resource_id: Some(vault_id.to_string()),
            })
        })?;
        let permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_project_number(project_number.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(vault_prefix.to_string());

        let generator = GcpRuntimePermissionsGenerator::new();
        let mut new_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_management_bindings_for(
            ctx,
            vault_id,
            vault_prefix,
            &management_refs,
            &generator,
            &permission_context,
            &mut new_bindings,
        )
        .await?;

        if new_bindings.is_empty() {
            return Ok(());
        }

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

        let mut all_bindings = current_policy.bindings;
        merge_iam_bindings(&mut all_bindings, new_bindings);

        let new_policy = IamPolicy::builder()
            .version(3)
            .bindings(all_bindings)
            .maybe_etag(current_policy.etag)
            .maybe_kind(current_policy.kind)
            .maybe_resource_id(current_policy.resource_id)
            .build();

        rm_client
            .set_project_iam_policy(gcp_config.project_id.clone(), new_policy, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to bind vault management roles at project level".to_string(),
                resource_id: Some(vault_id.to_string()),
            })?;

        info!(
            vault_id = %vault_id,
            vault_prefix = %vault_prefix,
            "GCP vault management permissions applied"
        );

        Ok(())
    }
}

fn merge_iam_bindings(existing_bindings: &mut Vec<Binding>, new_bindings: Vec<Binding>) {
    for binding in existing_bindings.iter_mut() {
        binding.members.retain(|m| !m.starts_with("deleted:"));
    }
    existing_bindings.retain(|binding| !binding.members.is_empty());

    for new_binding in new_bindings {
        let existing = existing_bindings.iter_mut().find(|binding| {
            binding.role == new_binding.role
                && match (&binding.condition, &new_binding.condition) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.expression == b.expression,
                    _ => false,
                }
        });

        if let Some(existing) = existing {
            for member in new_binding.members {
                if !existing.members.contains(&member) {
                    existing.members.push(member);
                }
            }
        } else {
            existing_bindings.push(new_binding);
        }
    }
}

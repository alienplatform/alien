use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::core::ResourcePermissionsHelper;
use crate::error::{ErrorData, Result};
use alien_core::{
    GcpSecretManagerVaultHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Vault, VaultHeartbeatData, VaultHeartbeatStatus, VaultOutputs,
};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    PermissionContext,
};
use chrono::Utc;

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

        self.apply_management_permissions(ctx, &config.id, &vault_prefix)
            .await?;
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
        self.apply_management_permissions(ctx, &config.id, vault_prefix)
            .await?;
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
    async fn apply_management_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vault_id: &str,
        vault_prefix: &str,
    ) -> Result<()> {
        // Project IAM grants are setup-owned, even when the vault is Live.
        if !ResourcePermissionsHelper::resource_is_setup_owned(ctx, vault_id)? {
            return Ok(());
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
                return Ok(());
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
            return Ok(());
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
        let namespace = format!(
            "resource.name.startsWith(\"projects/{}/secrets/{vault_prefix}-\")",
            gcp_config.project_number.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message:
                        "GCP project number is required to reconcile vault management permissions"
                            .to_string(),
                    resource_id: Some(vault_id.to_string()),
                })
            })?,
        );
        let current_bindings = normalized_bindings(&current_policy.bindings).context(
            ErrorData::InfrastructureError {
                message: "Failed to serialize current vault IAM bindings".to_string(),
                operation: Some("compare_vault_management_bindings".to_string()),
                resource_id: Some(vault_id.to_string()),
            },
        )?;
        let (mut vault_bindings, mut all_bindings): (Vec<_>, Vec<_>) =
            current_policy.bindings.into_iter().partition(|binding| {
                binding
                    .condition
                    .as_ref()
                    .is_some_and(|condition| condition.expression.contains(&namespace))
            });
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
        let proposed_bindings =
            normalized_bindings(&all_bindings).context(ErrorData::InfrastructureError {
                message: "Failed to serialize desired vault IAM bindings".to_string(),
                operation: Some("compare_vault_management_bindings".to_string()),
                resource_id: Some(vault_id.to_string()),
            })?;
        if proposed_bindings == current_bindings {
            info!(vault_id = %vault_id, "GCP vault management permissions already reconciled");
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

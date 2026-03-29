use alien_error::{AlienError, Context, ContextError, IntoAlienErrorDirect};
use alien_macros::controller;
use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, BindingTarget, PermissionContext,
};
use tracing::{debug, info};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{ArtifactRegistry, ArtifactRegistryOutputs, ResourceOutputs, ResourceStatus};
use alien_gcp_clients::iam::{CreateServiceAccountRequest, ServiceAccount};

/// Generates the service account ID for pull operations
pub fn get_gcp_artifact_registry_pull_service_account_id(
    prefix: &str,
    resource_id: &str,
) -> String {
    let raw_name = format!("{}-{}-pull", prefix, resource_id);
    if raw_name.len() <= 30 {
        raw_name.replace("-", "")
    } else {
        // Use a hash-based approach for longer names to ensure uniqueness and stay within limits
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        raw_name.hash(&mut hasher);
        let hash = hasher.finish();

        format!("{}-pull-{:x}", prefix.replace("-", ""), hash % 0xFFFF)
    }
}

/// Generates the service account ID for push operations
pub fn get_gcp_artifact_registry_push_service_account_id(
    prefix: &str,
    resource_id: &str,
) -> String {
    let raw_name = format!("{}-{}-push", prefix, resource_id);
    if raw_name.len() <= 30 {
        raw_name.replace("-", "")
    } else {
        // Use a hash-based approach for longer names to ensure uniqueness and stay within limits
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        raw_name.hash(&mut hasher);
        let hash = hasher.finish();

        format!("{}-push-{:x}", prefix.replace("-", ""), hash % 0xFFFF)
    }
}

/// GCP Artifact Registry controller.
///
/// GCP Artifact Registry is enabled per project and location via the API service.
/// This controller creates two service accounts to manage access: one for pull permissions and one for push+pull permissions.
#[controller]
pub struct GcpArtifactRegistryController {
    /// GCP project ID for the registry
    pub(crate) project_id: Option<String>,
    /// The GCP region/location for this registry
    pub(crate) location: Option<String>,
    /// The email of the pull service account
    pub(crate) pull_service_account_email: Option<String>,
    /// The email of the push+pull service account
    pub(crate) push_service_account_email: Option<String>,
}

#[controller]
impl GcpArtifactRegistryController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            project_id = %gcp_cfg.project_id,
            location = %gcp_cfg.region,
            "Setting up GCP Artifact Registry with service accounts"
        );

        // The Artifact Registry API should be enabled via infra requirements
        // Here we set up the registry reference and create service accounts
        self.project_id = Some(gcp_cfg.project_id.clone());
        self.location = Some(gcp_cfg.region.clone());

        info!(
            registry_id = %config.id,
            project_id = %gcp_cfg.project_id,
            location = %gcp_cfg.region,
            "GCP Artifact Registry reference is ready"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPullServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPullServiceAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_pull_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

        let pull_account_id =
            get_gcp_artifact_registry_pull_service_account_id(ctx.resource_prefix, &config.id);

        info!(
            account_id = %pull_account_id,
            "Creating pull service account for artifact registry"
        );

        let service_account = ServiceAccount::builder()
            .display_name(format!(
                "Alien Artifact Registry pull SA for registry {}",
                config.id
            ))
            .description(format!(
                "Service account for pulling from artifact registry {}",
                config.id
            ))
            .build();

        let request = CreateServiceAccountRequest::builder()
            .service_account(service_account)
            .build();

        let response = iam_client
            .create_service_account(pull_account_id.clone(), request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create pull service account '{}'",
                    pull_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.pull_service_account_email = response.email.clone();

        info!(
            account_id = %pull_account_id,
            email = %self.pull_service_account_email.as_deref().unwrap_or("unknown"),
            "Pull service account created successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPushServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPushServiceAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

        let push_account_id =
            get_gcp_artifact_registry_push_service_account_id(ctx.resource_prefix, &config.id);

        info!(
            account_id = %push_account_id,
            "Creating push service account for artifact registry"
        );

        let service_account = ServiceAccount::builder()
            .display_name(format!(
                "Alien Artifact Registry push SA for registry {}",
                config.id
            ))
            .description(format!(
                "Service account for pushing to artifact registry {}",
                config.id
            ))
            .build();

        let request = CreateServiceAccountRequest::builder()
            .service_account(service_account)
            .build();

        let response = iam_client
            .create_service_account(push_account_id.clone(), request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create push service account '{}'",
                    push_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.push_service_account_email = response.email.clone();

        info!(
            account_id = %push_account_id,
            email = %self.push_service_account_email.as_deref().unwrap_or("unknown"),
            "Push service account created successfully"
        );

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Applying resource-scoped permissions for artifact registry"
        );

        // Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx).await?;

        info!(
            registry_id = %config.id,
            "Resource-scoped permissions applied successfully"
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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "GCP Artifact Registry update (no-op - nothing to update)"
        );

        // GCP Artifact Registry service accounts don't need updates - just transition back to ready
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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Deleting GCP Artifact Registry service accounts"
        );

        Ok(HandlerAction::Continue {
            state: DeletingPullServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPullServiceAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_pull_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

        if let Some(ref email) = self.pull_service_account_email {
            // Delete pull service account - treat NotFound as success for idempotent deletion
            match iam_client.delete_service_account(email.clone()).await {
                Ok(_) => {
                    info!(email = %email, "Pull service account deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(email = %email, "Pull service account already deleted");
                }
                Err(e) => {
                    return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete pull service account '{}'", email),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }

            self.pull_service_account_email = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingPushServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPushServiceAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

        if let Some(ref email) = self.push_service_account_email {
            // Delete push service account - treat NotFound as success for idempotent deletion
            match iam_client.delete_service_account(email.clone()).await {
                Ok(_) => {
                    info!(email = %email, "Push service account deleted successfully");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(email = %email, "Push service account already deleted");
                }
                Err(e) => {
                    return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete push service account '{}'", email),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }

            self.push_service_account_email = None;
        }

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Heartbeat check: verify stored project/region haven't drifted and service accounts exist
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

            // Verify service accounts still exist
            let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

            // Check pull service account
            if let Some(ref email) = self.pull_service_account_email {
                match iam_client.get_service_account(email.clone()).await {
                    Ok(_) => {
                        debug!(email = %email, "Pull service account verified successfully");
                    }
                    Err(e) => {
                        return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to verify pull service account '{}' during heartbeat check",
                                email
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            }

            // Check push service account
            if let Some(ref email) = self.push_service_account_email {
                match iam_client.get_service_account(email.clone()).await {
                    Ok(_) => {
                        debug!(email = %email, "Push service account verified successfully");
                    }
                    Err(e) => {
                        return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to verify push service account '{}' during heartbeat check",
                                email
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            }

            debug!(project_id=%stored_project_id, location=%stored_location, "GCP Artifact Registry heartbeat check passed");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(30)), // Check again in 30 seconds
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
            let registry_id = format!("projects/{}/locations/{}", project_id, location);
            let registry_endpoint = format!("{}-docker.pkg.dev/{}", location, project_id);
            Some(ResourceOutputs::new(ArtifactRegistryOutputs {
                registry_id,
                registry_endpoint,
                pull_role: self.pull_service_account_email.clone(),
                push_role: self.push_service_account_email.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{ArtifactRegistryBinding, BindingValue};

        if let (Some(_project_id), Some(_location)) = (&self.project_id, &self.location) {
            let binding = ArtifactRegistryBinding::gar(
                self.pull_service_account_email.clone(),
                self.push_service_account_email.clone(),
            );

            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

impl GcpArtifactRegistryController {
    /// Applies resource-scoped permissions to the artifact registry from stack permission profiles
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let gcp_config = ctx.get_gcp_config()?;

        // Ensure all GCP custom roles referenced by the permission sets exist
        // before trying to apply IAM bindings that reference them
        ResourcePermissionsHelper::ensure_gcp_resource_custom_roles(ctx, &config.id, &config.id)
            .await?;

        // Build permission context for this specific artifact registry resource
        let mut permission_context = PermissionContext::new()
            .with_project_name(gcp_config.project_id.clone())
            .with_region(gcp_config.region.clone())
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_resource_name(config.id.clone());
        if let Some(ref project_number) = gcp_config.project_number {
            permission_context = permission_context.with_project_number(project_number.clone());
        }

        let generator = GcpRuntimePermissionsGenerator::new();

        // Process each permission profile in the stack
        for (profile_name, profile) in &ctx.desired_stack.permissions.profiles {
            if let Some(permission_set_refs) = profile.0.get(&config.id) {
                info!(
                    registry_id = %config.id,
                    profile = %profile_name,
                    permission_sets = ?permission_set_refs,
                    "Processing resource-scoped permissions for artifact registry"
                );

                self.process_profile_permissions(
                    ctx,
                    profile_name,
                    permission_set_refs,
                    &generator,
                    &permission_context,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to process permissions for profile '{}' on artifact registry '{}'",
                        profile_name, config.id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        Ok(())
    }

    /// Process permissions for a specific profile
    async fn process_profile_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
        permission_set_refs: &[alien_core::permissions::PermissionSetReference],
        generator: &GcpRuntimePermissionsGenerator,
        permission_context: &PermissionContext,
    ) -> Result<()> {
        // Get the service account email for this profile
        let service_account_email =
            self.get_service_account_email_for_profile(ctx, profile_name)?;

        // Get clients
        let gcp_config = ctx.get_gcp_config()?;
        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)?;

        // Get current project IAM policy (version 3 required for conditional bindings)
        let current_policy = rm_client
            .get_project_iam_policy(
                gcp_config.project_id.clone(),
                Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                    requested_policy_version: Some(3),
                }),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get current project IAM policy".to_string(),
                resource_id: Some(profile_name.to_string()),
            })?;

        let mut updated_policy = current_policy;
        // Ensure policy version is 3 for conditional bindings
        updated_policy.version = Some(3);

        // Process each permission set for this resource
        for permission_set_ref in permission_set_refs {
            let permission_set = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Permission set '{}' not found", permission_set_ref.id()),
                        resource_id: Some(profile_name.to_string()),
                    })
                })?;

            // Generate IAM bindings for resource-scoped permissions
            let bindings_result = generator
                .generate_bindings(&permission_set, BindingTarget::Resource, permission_context)
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to generate bindings for permission set '{}'",
                        permission_set.id
                    ),
                    resource_id: Some(profile_name.to_string()),
                })?;

            info!(
                profile = %profile_name,
                service_account = %service_account_email,
                permission_set = %permission_set.id,
                bindings_count = bindings_result.bindings.len(),
                "Applying IAM bindings for artifact registry permissions"
            );

            // Convert and merge bindings into the policy, deduplicating by (role, condition)
            for binding in bindings_result.bindings {
                let iam_condition = binding.condition.map(|cond| {
                    alien_gcp_clients::iam::Expr::builder()
                        .expression(cond.expression)
                        .title(cond.title)
                        .description(cond.description)
                        .build()
                });

                let member = format!("serviceAccount:{}", service_account_email);

                let existing = updated_policy.bindings.iter_mut().find(|b| {
                    b.role == binding.role
                        && match (&b.condition, &iam_condition) {
                            (None, None) => true,
                            (Some(a), Some(b)) => a.expression == b.expression,
                            _ => false,
                        }
                });

                if let Some(existing) = existing {
                    if !existing.members.contains(&member) {
                        existing.members.push(member);
                    }
                } else {
                    let mut iam_binding = alien_gcp_clients::iam::Binding::builder()
                        .role(binding.role.clone())
                        .members(vec![member])
                        .build();
                    iam_binding.condition = iam_condition;
                    updated_policy.bindings.push(iam_binding);
                }
            }
        }

        // Apply the updated policy
        rm_client
            .set_project_iam_policy(gcp_config.project_id.clone(), updated_policy, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to apply IAM bindings".to_string(),
                resource_id: Some(profile_name.to_string()),
            })?;

        info!(
            profile = %profile_name,
            service_account = %service_account_email,
            "Successfully applied IAM bindings for artifact registry permissions"
        );

        Ok(())
    }

    /// Get the service account email for a permission profile
    fn get_service_account_email_for_profile(
        &self,
        ctx: &ResourceControllerContext<'_>,
        profile_name: &str,
    ) -> Result<String> {
        let service_account_id = format!("{}-sa", profile_name);
        let service_account_resource = ctx
            .desired_stack
            .resources
            .get(&service_account_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Service account resource '{}' not found for profile '{}'",
                        service_account_id, profile_name
                    ),
                    resource_id: Some(profile_name.to_string()),
                })
            })?;

        let service_account_controller = ctx
            .require_dependency::<crate::service_account::GcpServiceAccountController>(
                &(&service_account_resource.config).into(),
            )?;

        service_account_controller
            .service_account_email
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: "artifact_registry".to_string(),
                    dependency_id: profile_name.to_string(),
                })
            })
    }

    /// Create a mock controller for testing
    #[cfg(test)]
    pub fn mock_ready(project_id: &str, location: &str) -> Self {
        Self {
            state: GcpArtifactRegistryState::Ready,
            project_id: Some(project_id.to_string()),
            location: Some(location.to_string()),
            pull_service_account_email: Some(format!(
                "test-pull@{}.iam.gserviceaccount.com",
                project_id
            )),
            push_service_account_email: Some(format!(
                "test-push@{}.iam.gserviceaccount.com",
                project_id
            )),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use alien_gcp_clients::iam::{MockIamApi, ServiceAccount};
    use std::sync::Arc;

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn create_successful_service_account_response(account_id: &str) -> ServiceAccount {
        ServiceAccount {
            name: Some(format!(
                "projects/test-project-123/serviceAccounts/{}",
                account_id
            )),
            project_id: Some("test-project-123".to_string()),
            unique_id: Some("123456789012".to_string()),
            email: Some(format!(
                "{}@test-project-123.iam.gserviceaccount.com",
                account_id
            )),
            display_name: Some(format!("Test service account {}", account_id)),
            etag: Some("etag123".to_string()),
            description: None,
            oauth2_client_id: None,
            disabled: None,
        }
    }

    fn setup_mock_client_for_creation_and_deletion() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful service account creation (for both pull and push)
        mock_iam
            .expect_create_service_account()
            .returning(|account_id, _| Ok(create_successful_service_account_response(&account_id)));

        // Mock successful service account deletion (for both pull and push)
        mock_iam
            .expect_delete_service_account()
            .returning(|_| Ok(()));

        Arc::new(mock_iam)
    }

    fn setup_mock_client_for_creation_and_update() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful service account creation (for both pull and push)
        mock_iam
            .expect_create_service_account()
            .returning(|account_id, _| Ok(create_successful_service_account_response(&account_id)));

        // Mock successful service account retrieval for heartbeat checks
        mock_iam
            .expect_get_service_account()
            .returning(|service_account_name| {
                // Extract account ID from the service account name
                let account_id = service_account_name.split('/').last().unwrap_or("unknown");
                Ok(create_successful_service_account_response(account_id))
            });

        Arc::new(mock_iam)
    }

    fn setup_mock_service_provider(mock_iam: Arc<MockIamApi>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as GcpClientConfig::mock()
        let project_id = "test-project-123";
        let location = "us-central1";

        let mock_iam = setup_mock_client_for_creation_and_deletion();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(GcpArtifactRegistryController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Test create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs
        let outputs = executor.outputs().unwrap();
        let registry_outputs = outputs.downcast_ref::<ArtifactRegistryOutputs>().unwrap();

        assert_eq!(
            registry_outputs.registry_id,
            format!("projects/{}/locations/{}", project_id, location)
        );
        assert_eq!(
            registry_outputs.registry_endpoint,
            format!("{}-docker.pkg.dev/{}", location, project_id)
        );
        assert!(registry_outputs.pull_role.is_some());
        assert!(registry_outputs.push_role.is_some());

        // Test delete flow
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as GcpClientConfig::mock()
        let project_id = "test-project-123";
        let location = "us-central1";

        let mock_iam = setup_mock_client_for_creation_and_update();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry.clone())
            .controller(GcpArtifactRegistryController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Initial creation
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Test update flow (should be no-op)
        executor.update(registry).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}

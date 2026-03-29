use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use tracing::{debug, info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{ArtifactRegistry, ArtifactRegistryOutputs, ResourceOutputs, ResourceStatus};
use alien_gcp_clients::artifactregistry::{Repository, RepositoryFormat};
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
    /// LRO operation name for repository creation (used while waiting)
    pub(crate) repository_operation_name: Option<String>,
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
            "Setting up GCP Artifact Registry"
        );

        self.project_id = Some(gcp_cfg.project_id.clone());
        self.location = Some(gcp_cfg.region.clone());

        let ar_client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_cfg)?;

        // Check if the repository already exists
        match ar_client
            .get_repository(
                gcp_cfg.project_id.clone(),
                gcp_cfg.region.clone(),
                config.id.clone(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    registry_id = %config.id,
                    "GCP Artifact Registry repository already exists"
                );
                Ok(HandlerAction::Continue {
                    state: CreatingPullServiceAccount,
                    suggested_delay: None,
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(
                    registry_id = %config.id,
                    "Repository not found, creating it"
                );

                let repository = Repository {
                    format: Some(RepositoryFormat::Docker),
                    description: Some(format!(
                        "Alien Artifact Registry for {}",
                        config.id
                    )),
                    ..Default::default()
                };

                let operation = ar_client
                    .create_repository(
                        gcp_cfg.project_id.clone(),
                        gcp_cfg.region.clone(),
                        config.id.clone(),
                        repository,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to create Artifact Registry repository '{}'",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                if operation.done == Some(true) {
                    info!(
                        registry_id = %config.id,
                        "Repository created successfully"
                    );
                    Ok(HandlerAction::Continue {
                        state: CreatingPullServiceAccount,
                        suggested_delay: None,
                    })
                } else {
                    self.repository_operation_name =
                        operation.name.clone();
                    Ok(HandlerAction::Continue {
                        state: CreatingRepository,
                        suggested_delay: Some(std::time::Duration::from_secs(2)),
                    })
                }
            }
            Err(e) => Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check Artifact Registry repository '{}'",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = CreatingRepository,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_repository(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        let operation_name = self.repository_operation_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Missing repository operation name".to_string(),
                operation: Some("creating_repository".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let ar_client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_cfg)?;

        let operation = ar_client
            .get_operation(
                gcp_cfg.project_id.clone(),
                gcp_cfg.region.clone(),
                operation_name.clone(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check repository creation status for '{}'",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        if operation.done == Some(true) {
            info!(
                registry_id = %config.id,
                "Repository created successfully"
            );
            self.repository_operation_name = None;
            Ok(HandlerAction::Continue {
                state: CreatingPullServiceAccount,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingRepository,
                suggested_delay: Some(std::time::Duration::from_secs(2)),
            })
        }
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
            state: DeletingRepository,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRepository,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_repository(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        let ar_client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_cfg)?;

        match ar_client
            .delete_repository(
                gcp_cfg.project_id.clone(),
                gcp_cfg.region.clone(),
                config.id.clone(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    registry_id = %config.id,
                    "Repository deleted successfully"
                );
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(
                    registry_id = %config.id,
                    "Repository already deleted"
                );
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete Artifact Registry repository '{}'",
                        config.id
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
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

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{ArtifactRegistryBinding, BindingValue};

        if let (Some(_project_id), Some(_location)) = (&self.project_id, &self.location) {
            let binding = ArtifactRegistryBinding::gar(
                self.pull_service_account_email.clone(),
                self.push_service_account_email.clone(),
            );

            Ok(Some(serde_json::to_value(binding).into_alien_error().context(
                ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize binding parameters".to_string(),
                },
            )?))
        } else {
            Ok(None)
        }
    }
}

impl GcpArtifactRegistryController {
    /// Applies resource-scoped permissions to the artifact registry using repository-level IAM
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let gcp_config = ctx.get_gcp_config()?;

        let project_id = gcp_config.project_id.clone();
        let location = gcp_config.region.clone();
        let repository_id = config.id.clone();

        // Use ResourcePermissionsHelper to apply repository-level IAM
        // (instead of project-level IAM which is overly broad)
        let client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_config)?;
        let project_id_owned = project_id.clone();
        let location_owned = location.clone();
        let repository_id_owned = repository_id.clone();
        let config_id_owned = config.id.clone();

        ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
            ctx,
            &config.id,
            &config.id,
            "Artifact Registry repository",
            "artifact-registry",
            client,
            |client, iam_policy| async move {
                client
                    .set_repository_iam_policy(
                        project_id_owned,
                        location_owned,
                        repository_id_owned.clone(),
                        iam_policy,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to apply IAM policy to Artifact Registry repository '{}'",
                            repository_id_owned
                        ),
                        resource_id: Some(config_id_owned),
                    })?;
                info!(
                    repository_id = %repository_id_owned,
                    "Applied IAM policy to Artifact Registry repository"
                );
                Ok(())
            },
        )
        .await?;

        Ok(())
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
            repository_operation_name: None,
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
    use alien_gcp_clients::artifactregistry::MockArtifactRegistryApi;
    use alien_gcp_clients::iam::{MockIamApi, Role, ServiceAccount};
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

    /// Adds common IAM mock expectations needed for resource-scoped permissions
    /// (custom role ensure + patch flow triggered by management permission mutations).
    fn add_resource_permission_mocks(mock_iam: &mut MockIamApi) {
        mock_iam
            .expect_get_role()
            .returning(|_| Ok(Role::default()));
        mock_iam
            .expect_patch_role()
            .returning(|_, _, _| Ok(Role::default()));
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

        add_resource_permission_mocks(&mut mock_iam);

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

        add_resource_permission_mocks(&mut mock_iam);

        Arc::new(mock_iam)
    }

    fn setup_mock_ar_client_existing_repo() -> Arc<MockArtifactRegistryApi> {
        let mut mock_ar = MockArtifactRegistryApi::new();

        // Repository already exists — CreateStart will skip creation
        mock_ar
            .expect_get_repository()
            .returning(|_, _, _| Ok(Repository::default()));

        // Delete flow calls delete_repository
        mock_ar.expect_delete_repository().returning(|_, _, _| {
            Ok(alien_gcp_clients::longrunning::Operation {
                name: None,
                metadata: None,
                done: Some(true),
                result: None,
            })
        });

        Arc::new(mock_ar)
    }

    fn setup_mock_service_provider(mock_iam: Arc<MockIamApi>) -> Arc<MockPlatformServiceProvider> {
        let mock_ar = setup_mock_ar_client_existing_repo();
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        mock_provider
            .expect_get_gcp_artifact_registry_client()
            .returning(move |_| Ok(mock_ar.clone()));

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

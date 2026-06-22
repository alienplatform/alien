use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use google_cloud_artifactregistry_v1::{
    client::ArtifactRegistry as ArtifactRegistryClient,
    model::{repository::Format as RepositoryFormat, Repository},
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_admin_v1::model::{CreateServiceAccountRequest, ServiceAccount};
use google_cloud_iam_v1::model::Policy;
use google_cloud_longrunning::model::Operation;
use tracing::{debug, info};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::gcp_iam_admin::{create_service_account, delete_service_account, get_service_account};
use alien_core::{
    bindings::ArtifactRegistryBinding, ArtifactRegistry, ArtifactRegistryHeartbeatData,
    ArtifactRegistryHeartbeatStatus, ArtifactRegistryOutputs, GcpArtifactRegistryHeartbeatData,
    HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use chrono::Utc;

/// Generates the prefixed GAR repository name for a given resource.
pub fn get_gcp_artifact_registry_repository_name(prefix: &str, resource_id: &str) -> String {
    format!("{}-{}", prefix, resource_id)
}

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

fn is_gcp_not_found<T>(error: &AlienError<T>) -> bool
where
    T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize,
{
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_NOT_FOUND" | "CLOUD_RESOURCE_NOT_FOUND"
    )
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
    /// Repository name (the config.id, used for binding params)
    pub(crate) repository_name: Option<String>,
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

        let repository_name =
            get_gcp_artifact_registry_repository_name(ctx.resource_prefix, &config.id);

        self.project_id = Some(gcp_cfg.project_id.clone());
        self.location = Some(gcp_cfg.region.clone());
        self.repository_name = Some(repository_name.clone());

        let ar_client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_cfg)
            .await?;

        // Check if the repository already exists
        match get_artifact_registry_repository(
            &ar_client,
            &gcp_cfg.project_id,
            &gcp_cfg.region,
            &repository_name,
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
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
                info!(
                    registry_id = %config.id,
                    "Repository not found, creating it"
                );

                let repository = Repository::new()
                    .set_format(RepositoryFormat::Docker)
                    .set_description(format!("Runtime Artifact Registry for {}", config.id));

                let operation = create_artifact_registry_repository(
                    &ar_client,
                    &gcp_cfg.project_id,
                    &gcp_cfg.region,
                    &repository_name,
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

                if operation.done {
                    info!(
                        registry_id = %config.id,
                        "Repository created successfully"
                    );
                    Ok(HandlerAction::Continue {
                        state: CreatingPullServiceAccount,
                        suggested_delay: None,
                    })
                } else {
                    self.repository_operation_name = Some(operation.name.clone());
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
            .get_gcp_artifact_registry_client(gcp_cfg)
            .await?;

        let operation = get_artifact_registry_operation(&ar_client, operation_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to check repository creation status for '{}'",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        if operation.done {
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
        let iam_client = ctx
            .service_provider
            .get_gcp_iam_admin_client(gcp_cfg)
            .await?;

        let pull_account_id =
            get_gcp_artifact_registry_pull_service_account_id(ctx.resource_prefix, &config.id);

        info!(
            account_id = %pull_account_id,
            "Creating pull service account for artifact registry"
        );

        let service_account = ServiceAccount::new()
            .set_display_name(format!(
                "Runtime Artifact Registry pull SA for registry {}",
                config.id
            ))
            .set_description(format!(
                "Service account for pulling from artifact registry {}",
                config.id
            ));

        let request = CreateServiceAccountRequest::new()
            .set_name(format!("projects/{}", gcp_cfg.project_id))
            .set_account_id(pull_account_id.clone())
            .set_service_account(service_account);

        let response = create_service_account(&iam_client, &gcp_cfg.project_id, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create pull service account '{}'",
                    pull_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.pull_service_account_email = if response.email.is_empty() {
            None
        } else {
            Some(response.email.clone())
        };

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
        let iam_client = ctx
            .service_provider
            .get_gcp_iam_admin_client(gcp_cfg)
            .await?;

        let push_account_id =
            get_gcp_artifact_registry_push_service_account_id(ctx.resource_prefix, &config.id);

        info!(
            account_id = %push_account_id,
            "Creating push service account for artifact registry"
        );

        let service_account = ServiceAccount::new()
            .set_display_name(format!(
                "Runtime Artifact Registry push SA for registry {}",
                config.id
            ))
            .set_description(format!(
                "Service account for pushing to artifact registry {}",
                config.id
            ));

        let request = CreateServiceAccountRequest::new()
            .set_name(format!("projects/{}", gcp_cfg.project_id))
            .set_account_id(push_account_id.clone())
            .set_service_account(service_account);

        let response = create_service_account(&iam_client, &gcp_cfg.project_id, request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create push service account '{}'",
                    push_account_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        self.push_service_account_email = if response.email.is_empty() {
            None
        } else {
            Some(response.email.clone())
        };

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
        let iam_client = ctx
            .service_provider
            .get_gcp_iam_admin_client(gcp_cfg)
            .await?;

        if let Some(ref email) = self.pull_service_account_email {
            // Delete pull service account - treat NotFound as success for idempotent deletion
            match delete_service_account(&iam_client, &gcp_cfg.project_id, email).await {
                Ok(_) => {
                    info!(email = %email, "Pull service account deleted successfully");
                }
                Err(e) if is_gcp_not_found(&e) => {
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
        let iam_client = ctx
            .service_provider
            .get_gcp_iam_admin_client(gcp_cfg)
            .await?;

        if let Some(ref email) = self.push_service_account_email {
            // Delete push service account - treat NotFound as success for idempotent deletion
            match delete_service_account(&iam_client, &gcp_cfg.project_id, email).await {
                Ok(_) => {
                    info!(email = %email, "Push service account deleted successfully");
                }
                Err(e) if is_gcp_not_found(&e) => {
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
        let repository_name =
            get_gcp_artifact_registry_repository_name(ctx.resource_prefix, &config.id);

        let ar_client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_cfg)
            .await?;

        match delete_artifact_registry_repository(
            &ar_client,
            &gcp_cfg.project_id,
            &gcp_cfg.region,
            &repository_name,
        )
        .await
        {
            Ok(_) => {
                info!(
                    registry_id = %config.id,
                    "Repository deleted successfully"
                );
            }
            Err(e) if matches!(e.error, Some(ErrorData::CloudResourceNotFound { .. })) => {
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
            let iam_client = ctx
                .service_provider
                .get_gcp_iam_admin_client(gcp_cfg)
                .await?;

            // Check pull service account
            if let Some(ref email) = self.pull_service_account_email {
                match get_service_account(&iam_client, &gcp_cfg.project_id, email).await {
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
                match get_service_account(&iam_client, &gcp_cfg.project_id, email).await {
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

            let repository_id = self.repository_name.clone().unwrap_or_else(|| {
                get_gcp_artifact_registry_repository_name(ctx.resource_prefix, &config.id)
            });
            let ar_client = ctx
                .service_provider
                .get_gcp_artifact_registry_client(gcp_cfg)
                .await?;
            let repository = get_artifact_registry_repository(
                &ar_client,
                stored_project_id,
                stored_location,
                &repository_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get Artifact Registry repository '{}' during heartbeat check",
                    repository_id
                ),
                resource_id: Some(config.id.clone()),
            })?;
            let iam_policy = get_artifact_registry_repository_iam_policy(
                &ar_client,
                stored_project_id,
                stored_location,
                &repository_id,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get Artifact Registry IAM policy '{}' during heartbeat check",
                    repository_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

            emit_gcp_artifact_registry_heartbeat(
                ctx,
                &config.id,
                stored_project_id,
                stored_location,
                &repository_id,
                repository,
                iam_policy,
                self.pull_service_account_email.clone(),
                self.push_service_account_email.clone(),
            );
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
        if let (Some(_project_id), Some(_location), Some(repository_name)) =
            (&self.project_id, &self.location, &self.repository_name)
        {
            let binding = ArtifactRegistryBinding::gar(
                repository_name.clone(),
                self.pull_service_account_email.clone(),
                self.push_service_account_email.clone(),
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

impl GcpArtifactRegistryController {
    /// Applies resource-scoped permissions to the artifact registry using repository-level IAM
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let gcp_config = ctx.get_gcp_config()?;
        let repository_name =
            get_gcp_artifact_registry_repository_name(ctx.resource_prefix, &config.id);

        let project_id = gcp_config.project_id.clone();
        let location = gcp_config.region.clone();

        let mut bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_resource_scoped_bindings(
            ctx,
            &config.id,
            &repository_name,
            "artifact-registry",
            &mut bindings,
        )
        .await?;

        if bindings.is_empty() {
            info!(
                repository_id = %repository_name,
                "No resource-scoped permissions to apply to Artifact Registry repository"
            );
            return Ok(());
        }

        let project_id_owned = project_id.clone();
        let location_owned = location.clone();
        let repository_id_owned = repository_name.clone();
        let config_id_owned = config.id.clone();
        let iam_policy = Policy::new().set_version(3).set_bindings(bindings);

        let client = ctx
            .service_provider
            .get_gcp_artifact_registry_client(gcp_config)
            .await?;
        set_artifact_registry_repository_iam_policy(
            &client,
            &project_id_owned,
            &location_owned,
            &repository_id_owned,
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
    }

    /// Create a mock controller for testing
    #[cfg(test)]
    pub fn mock_ready(project_id: &str, location: &str) -> Self {
        Self {
            state: GcpArtifactRegistryState::Ready,
            project_id: Some(project_id.to_string()),
            location: Some(location.to_string()),
            repository_name: Some("test-repo".to_string()),
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

fn emit_gcp_artifact_registry_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    project_id: &str,
    location: &str,
    repository_id: &str,
    repository: Repository,
    iam_policy: Policy,
    pull_service_account_email: Option<String>,
    push_service_account_email: Option<String>,
) {
    let iam_roles = iam_policy
        .bindings
        .iter()
        .map(|binding| binding.role.clone())
        .collect::<Vec<_>>();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: ArtifactRegistry::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::ArtifactRegistry(
            ArtifactRegistryHeartbeatData::GcpArtifactRegistry(GcpArtifactRegistryHeartbeatData {
                status: ArtifactRegistryHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "GCP Artifact Registry repository '{}' is reachable",
                        repository_id
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                project_id: project_id.to_string(),
                location: location.to_string(),
                repository_id: repository_id.to_string(),
                name: none_if_empty(repository.name),
                format: repository.format.name().map(String::from),
                mode: repository.mode.name().map(String::from),
                description: none_if_empty(repository.description),
                label_count: repository.labels.len() as u32,
                cleanup_policy_count: repository.cleanup_policies.len() as u32,
                cleanup_policy_dry_run: Some(repository.cleanup_policy_dry_run),
                kms_key_name_present: !repository.kms_key_name.is_empty(),
                size_bytes: Some(repository.size_bytes.to_string()),
                satisfies_pzs: Some(repository.satisfies_pzs),
                create_time: repository.create_time.map(String::from),
                update_time: repository.update_time.map(String::from),
                iam_policy_etag_present: !iam_policy.etag.is_empty(),
                iam_binding_count: iam_roles.len() as u32,
                iam_roles,
                pull_service_account_email,
                push_service_account_email,
            }),
        ),
        raw: vec![],
    });
}

fn none_if_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn artifact_registry_repository_resource_name(
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> String {
    format!("projects/{project_id}/locations/{location}/repositories/{repository_id}")
}

async fn create_artifact_registry_repository(
    client: &ArtifactRegistryClient,
    project_id: &str,
    location: &str,
    repository_id: &str,
    repository: Repository,
) -> Result<Operation> {
    client
        .create_repository()
        .set_parent(format!("projects/{project_id}/locations/{location}"))
        .set_repository_id(repository_id.to_string())
        .set_repository(repository)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Artifact Registry create_repository request failed".to_string(),
            resource_id: Some(repository_id.to_string()),
        })
}

async fn delete_artifact_registry_repository(
    client: &ArtifactRegistryClient,
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> Result<Operation> {
    let resource_name =
        artifact_registry_repository_resource_name(project_id, location, repository_id);
    match client
        .delete_repository()
        .set_name(resource_name.clone())
        .send()
        .await
    {
        Ok(operation) => Ok(operation),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Artifact Registry repository".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Artifact Registry delete_repository request failed".to_string(),
                resource_id: Some(repository_id.to_string()),
            })),
    }
}

async fn get_artifact_registry_repository(
    client: &ArtifactRegistryClient,
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> Result<Repository> {
    let resource_name =
        artifact_registry_repository_resource_name(project_id, location, repository_id);
    match client
        .get_repository()
        .set_name(resource_name.clone())
        .send()
        .await
    {
        Ok(repository) => Ok(repository),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "Artifact Registry repository".to_string(),
                resource_name,
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "Artifact Registry get_repository request failed".to_string(),
                resource_id: Some(repository_id.to_string()),
            })),
    }
}

async fn get_artifact_registry_repository_iam_policy(
    client: &ArtifactRegistryClient,
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> Result<Policy> {
    client
        .get_iam_policy()
        .set_resource(artifact_registry_repository_resource_name(
            project_id,
            location,
            repository_id,
        ))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Artifact Registry get_iam_policy request failed".to_string(),
            resource_id: Some(repository_id.to_string()),
        })
}

async fn set_artifact_registry_repository_iam_policy(
    client: &ArtifactRegistryClient,
    project_id: &str,
    location: &str,
    repository_id: &str,
    iam_policy: Policy,
) -> Result<Policy> {
    client
        .set_iam_policy()
        .set_resource(artifact_registry_repository_resource_name(
            project_id,
            location,
            repository_id,
        ))
        .set_policy(iam_policy)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Artifact Registry set_iam_policy request failed".to_string(),
            resource_id: Some(repository_id.to_string()),
        })
}

async fn get_artifact_registry_operation(
    client: &ArtifactRegistryClient,
    operation_name: &str,
) -> Result<Operation> {
    client
        .get_operation()
        .set_name(operation_name.to_string())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Artifact Registry get_operation request failed".to_string(),
            resource_id: Some(operation_name.to_string()),
        })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_core::Platform;
    use google_cloud_artifactregistry_v1::{
        model::{CreateRepositoryRequest, DeleteRepositoryRequest, GetRepositoryRequest},
        stub::ArtifactRegistry as ArtifactRegistryStubTrait,
    };
    use google_cloud_gax::{
        error::{
            rpc::{Code, Status},
            Error as GaxError,
        },
        options::RequestOptions,
        response::Response,
    };
    use google_cloud_iam_admin_v1::{
        client::Iam as IamAdminClient,
        model::{DeleteServiceAccountRequest, GetServiceAccountRequest},
        stub::Iam as IamAdminStubTrait,
    };
    use std::sync::Arc;

    const TEST_PROJECT_ID: &str = "test-project";
    const TEST_LOCATION: &str = "us-central1";
    const TEST_REPOSITORY_NAME: &str = "test-my-registry";
    const TEST_REPOSITORY_RESOURCE: &str =
        "projects/test-project/locations/us-central1/repositories/test-my-registry";

    mockall::mock! {
        #[derive(Debug)]
        ArtifactRegistrySdk {}

        impl ArtifactRegistryStubTrait for ArtifactRegistrySdk {
            async fn get_repository(
                &self,
                request: GetRepositoryRequest,
                options: RequestOptions,
            ) -> google_cloud_artifactregistry_v1::Result<Response<Repository>>;

            async fn create_repository(
                &self,
                request: CreateRepositoryRequest,
                options: RequestOptions,
            ) -> google_cloud_artifactregistry_v1::Result<Response<Operation>>;

            async fn delete_repository(
                &self,
                request: DeleteRepositoryRequest,
                options: RequestOptions,
            ) -> google_cloud_artifactregistry_v1::Result<Response<Operation>>;
        }
    }

    mockall::mock! {
        #[derive(Debug)]
        IamAdminSdk {}

        impl IamAdminStubTrait for IamAdminSdk {
            async fn create_service_account(
                &self,
                request: CreateServiceAccountRequest,
                options: RequestOptions,
            ) -> google_cloud_iam_admin_v1::Result<Response<ServiceAccount>>;

            async fn delete_service_account(
                &self,
                request: DeleteServiceAccountRequest,
                options: RequestOptions,
            ) -> google_cloud_iam_admin_v1::Result<Response<()>>;

            async fn get_service_account(
                &self,
                request: GetServiceAccountRequest,
                options: RequestOptions,
            ) -> google_cloud_iam_admin_v1::Result<Response<ServiceAccount>>;
        }
    }

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn create_successful_service_account_response(account_id: &str) -> ServiceAccount {
        ServiceAccount::new()
            .set_name(format!(
                "projects/{}/serviceAccounts/{}",
                TEST_PROJECT_ID, account_id
            ))
            .set_project_id(TEST_PROJECT_ID)
            .set_unique_id("123456789012")
            .set_email(format!(
                "{}@{}.iam.gserviceaccount.com",
                account_id, TEST_PROJECT_ID
            ))
            .set_display_name(format!("Test service account {}", account_id))
    }

    fn setup_mock_client_for_creation_and_deletion() -> IamAdminClient {
        let mut mock_iam = MockIamAdminSdk::new();

        // Mock successful service account creation (for both pull and push)
        mock_iam
            .expect_create_service_account()
            .returning(|request, _| {
                Ok(Response::from(create_successful_service_account_response(
                    &request.account_id,
                )))
            });

        // Mock successful service account deletion (for both pull and push)
        mock_iam
            .expect_delete_service_account()
            .returning(|_, _| Ok(Response::from(())));

        IamAdminClient::from_stub(mock_iam)
    }

    fn setup_mock_client_for_creation_and_update() -> IamAdminClient {
        let mut mock_iam = MockIamAdminSdk::new();

        // Mock successful service account creation (for both pull and push)
        mock_iam
            .expect_create_service_account()
            .returning(|request, _| {
                Ok(Response::from(create_successful_service_account_response(
                    &request.account_id,
                )))
            });

        // Mock successful service account retrieval for heartbeat checks
        mock_iam
            .expect_get_service_account()
            .returning(|request, _| {
                // Extract account ID from the service account name
                let account_id = request.name.split('/').last().unwrap_or("unknown");
                Ok(Response::from(create_successful_service_account_response(
                    account_id,
                )))
            });

        IamAdminClient::from_stub(mock_iam)
    }

    fn setup_mock_ar_client_existing_repo() -> ArtifactRegistryClient {
        let mut stub = MockArtifactRegistrySdk::new();

        // Repository already exists, so CreateStart skips creation.
        stub.expect_get_repository()
            .withf(|request, _| request.name == TEST_REPOSITORY_RESOURCE)
            .returning(|_, _| {
                Ok(Response::from(
                    Repository::new().set_name(TEST_REPOSITORY_RESOURCE),
                ))
            });

        stub.expect_delete_repository()
            .withf(|request, _| request.name == TEST_REPOSITORY_RESOURCE)
            .returning(|_, _| Ok(Response::from(Operation::new().set_done(true))));

        ArtifactRegistryClient::from_stub(stub)
    }

    fn setup_mock_ar_client_missing_repo_then_create() -> ArtifactRegistryClient {
        let mut stub = MockArtifactRegistrySdk::new();

        stub.expect_get_repository()
            .withf(|request, _| request.name == TEST_REPOSITORY_RESOURCE)
            .once()
            .returning(|_, _| Err(not_found_error()));

        stub.expect_create_repository()
            .withf(|request, _| {
                request.parent == "projects/test-project/locations/us-central1"
                    && request.repository_id == TEST_REPOSITORY_NAME
                    && request.repository.as_ref().is_some_and(|repository| {
                        repository.format == RepositoryFormat::Docker
                            && repository.description == "Runtime Artifact Registry for my-registry"
                    })
            })
            .once()
            .returning(|_, _| Ok(Response::from(Operation::new().set_done(true))));

        ArtifactRegistryClient::from_stub(stub)
    }

    fn not_found_error() -> GaxError {
        GaxError::service(
            Status::default()
                .set_code(Code::NotFound)
                .set_message("repository not found"),
        )
    }

    fn setup_mock_service_provider(mock_iam: IamAdminClient) -> Arc<MockPlatformServiceProvider> {
        let mock_ar = setup_mock_ar_client_existing_repo();
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_iam_admin_client()
            .returning(move |_| Ok(mock_iam.clone()));

        mock_provider
            .expect_get_gcp_artifact_registry_client()
            .returning(move |_| Ok(mock_ar.clone()));

        Arc::new(mock_provider)
    }

    fn setup_mock_service_provider_with_ar_client(
        mock_iam: IamAdminClient,
        ar_client: ArtifactRegistryClient,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_iam_admin_client()
            .returning(move |_| Ok(mock_iam.clone()));

        mock_provider
            .expect_get_gcp_artifact_registry_client()
            .returning(move |_| Ok(ar_client.clone()));

        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let registry = basic_artifact_registry();

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
            format!("projects/{}/locations/{}", TEST_PROJECT_ID, TEST_LOCATION)
        );
        assert_eq!(
            registry_outputs.registry_endpoint,
            format!("{}-docker.pkg.dev/{}", TEST_LOCATION, TEST_PROJECT_ID)
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

    #[tokio::test]
    async fn create_flow_creates_missing_repository_with_sdk_native_stub() {
        let registry = basic_artifact_registry();

        let mock_iam = setup_mock_client_for_creation_and_deletion();
        let ar_client = setup_mock_ar_client_missing_repo_then_create();
        let mock_provider = setup_mock_service_provider_with_ar_client(mock_iam, ar_client);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(GcpArtifactRegistryController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor should build");

        executor
            .run_until_terminal()
            .await
            .expect("create flow should reach ready");

        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}

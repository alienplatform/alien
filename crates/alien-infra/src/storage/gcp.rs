use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    GcpCloudStorageHeartbeatData, HeartbeatBackend, InitialSetupAuthority,
    LifecycleRule as StorageLifecycleRule, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceLifecycle, ResourceOutputs, ResourceStatus,
    Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_gcp_clients::gcs::{
    Bucket, IamConfiguration, Lifecycle, LifecycleAction, LifecycleCondition, LifecycleRule,
    UniformBucketLevelAccess, Versioning,
};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_macros::controller;
use chrono::Utc;
use sha2::{Digest, Sha256};

/// Generates the full, prefixed GCP bucket name.
fn get_gcp_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct GcpStorageController {
    /// The actual bucket name (includes stack name prefix).
    /// This is None until the bucket is created or imported.
    pub(crate) bucket_name: Option<String>,

    /// Revision of the lifecycle configuration that GCS last accepted for the bucket from this
    /// controller. `None` when an older version configured the bucket: those versions sent
    /// every rule without its prefix, so a prefixed rule expired the whole bucket.
    #[serde(default)]
    pub(crate) lifecycle_revision: Option<String>,
}

#[controller]
impl GcpStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        // Generate bucket name if not already set (initial creation)
        let bucket_name = if let Some(name) = &self.bucket_name {
            name.clone()
        } else {
            info!(name = %config.id, "Initiating GCS bucket creation");
            get_gcp_bucket_name(ctx.resource_prefix, &config.id)
        };

        info!(bucket = %bucket_name, "Creating GCS bucket with basic configuration");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Build bucket configuration with basic settings only
        let mut bucket = Bucket::default();

        // Set location from GCP config
        bucket.location = Some(gcp_config.region.clone());

        // Configure versioning if enabled
        if config.versioning {
            bucket.versioning = Some(Versioning { enabled: true });
        }

        let lifecycle = gcs_lifecycle(&config.id, &config.lifecycle_rules)?;
        let lifecycle_revision = gcs_lifecycle_revision(&config.id, &lifecycle)?;
        if lifecycle.rule.is_some() {
            bucket.lifecycle = Some(lifecycle.clone());
        }

        // Create the bucket with basic configuration only
        let created_bucket = client
            .create_bucket(bucket_name.clone(), bucket)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create GCS bucket '{}'", bucket_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(bucket = %bucket_name, "GCS bucket created successfully");

        let lifecycle_confirmed = gcs_lifecycle_matches(created_bucket.lifecycle.as_ref(), &lifecycle);
        self.bucket_name = Some(created_bucket.name.unwrap_or_else(|| bucket_name.clone()));
        if lifecycle_confirmed {
            self.lifecycle_revision = Some(lifecycle_revision);
        } else {
            // The bucket exists, so failing here would make the retry create it again. Leaving
            // the revision unset makes the next update rewrite the rules and check them again.
            warn!(bucket = %bucket_name, "GCS returned different lifecycle rules than requested; they will be rewritten on the next update");
        }

        Ok(HandlerAction::Continue {
            state: CreateWaitForActive,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = CreateWaitForActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during wait".to_string(),
            })
        })?;

        info!(bucket = %bucket_name, "Checking bucket status");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Check if bucket exists and is ready
        match client.get_bucket(bucket_name.clone()).await {
            Ok(_bucket) => {
                info!(bucket = %bucket_name, "Bucket is ready");
                Ok(HandlerAction::Continue {
                    state: SetIamPolicy,
                    suggested_delay: None,
                })
            }
            Err(_e) => {
                debug!(bucket = %bucket_name, "Bucket not yet ready, waiting");
                Ok(HandlerAction::Continue {
                    state: CreateWaitForActive,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
        }
    }

    #[handler(
        state = SetIamPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn set_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during IAM policy setup".to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Step 1: Apply resource-scoped permissions from the stack
        self.apply_resource_scoped_permissions(ctx, bucket_name, &client)
            .await?;

        // Step 2: Handle public read access if enabled
        if config.public_read {
            info!(bucket = %bucket_name, "Setting IAM policy for public read access");

            // First set uniform bucket-level access
            let mut bucket_patch = Bucket::default();
            bucket_patch.iam_configuration = Some(IamConfiguration {
                uniform_bucket_level_access: Some(UniformBucketLevelAccess {
                    enabled: true,
                    locked_time: None,
                }),
                public_access_prevention: Some("inherited".to_string()),
            });

            client
                .update_bucket(bucket_name.clone(), bucket_patch)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to enable uniform bucket-level access for bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            // Then add public read binding via read-modify-write to preserve existing bindings
            let mut existing_policy = client
                .get_bucket_iam_policy(bucket_name.clone())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get bucket IAM policy for '{}' before setting public read. Refusing to proceed to avoid overwriting existing bindings.", bucket_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let public_viewer_role = "roles/storage.objectViewer";
            let all_users = "allUsers".to_string();

            // Only add if not already present
            let already_has_public = existing_policy
                .bindings
                .iter()
                .any(|b| b.role == public_viewer_role && b.members.contains(&all_users));

            if !already_has_public {
                if let Some(binding) = existing_policy
                    .bindings
                    .iter_mut()
                    .find(|b| b.role == public_viewer_role)
                {
                    if !binding.members.contains(&all_users) {
                        binding.members.push(all_users);
                    }
                } else {
                    existing_policy.bindings.push(Binding {
                        role: public_viewer_role.to_string(),
                        members: vec![all_users],
                        condition: None,
                    });
                }

                existing_policy.version = Some(3);

                client
                    .set_bucket_iam_policy(bucket_name.clone(), existing_policy)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to set IAM policy for bucket '{}'", bucket_name),
                        resource_id: Some(config.id.clone()),
                    })?;
            }

            info!(bucket = %bucket_name, "IAM policy for public read access set successfully");
        } else {
            info!(bucket = %bucket_name, "No public read access needed, skipping public IAM policy");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        if let Some(bucket_name) = &self.bucket_name {
            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

            // Fetch bucket metadata without listing objects or reading object ACLs.
            let bucket = client.get_bucket(bucket_name.clone()).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to check GCS bucket during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            emit_gcp_storage_heartbeat(ctx, &config.id, bucket_name, bucket);

            debug!(name = %config.id, bucket = %bucket_name, "GCS bucket exists and is accessible");
        }

        debug!(name = %config.id, "Heartbeat check passed");
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
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during update".to_string(),
            })
        })?;

        info!(bucket = %bucket_name, "Starting bucket configuration update");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Build patch object with changed fields (always check all fields, no early optimization)
        let mut bucket_patch = Bucket::default();
        let mut needs_update = false;

        // Check versioning changes
        if config.versioning != prev_config.versioning {
            info!(bucket = %bucket_name, current = %config.versioning, previous = %prev_config.versioning, "Updating versioning");
            bucket_patch.versioning = Some(Versioning {
                enabled: config.versioning,
            });
            needs_update = true;
        }

        // Rewrite the lifecycle when the rules changed, or when GCS has not confirmed the
        // current rules from this controller (buckets configured by older versions lack the
        // rule prefixes). The patch replaces the bucket's whole lifecycle, so resending it is
        // safe when a retry repeats this step.
        let lifecycle = gcs_lifecycle(&config.id, &config.lifecycle_rules)?;
        let lifecycle_revision = gcs_lifecycle_revision(&config.id, &lifecycle)?;
        let write_lifecycle = config.lifecycle_rules != prev_config.lifecycle_rules
            || self.lifecycle_revision.as_deref() != Some(lifecycle_revision.as_str());
        if write_lifecycle {
            info!(bucket = %bucket_name, rules_count = %config.lifecycle_rules.len(), "Updating lifecycle rules");
            bucket_patch.lifecycle = Some(lifecycle.clone());
            needs_update = true;
        }

        // Apply bucket configuration changes if needed
        if needs_update {
            let updated_bucket = client
                .update_bucket(bucket_name.clone(), bucket_patch)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update GCS bucket '{}'", bucket_name),
                    resource_id: Some(config.id.clone()),
                })?;
            if write_lifecycle {
                if !gcs_lifecycle_matches(updated_bucket.lifecycle.as_ref(), &lifecycle) {
                    return Err(AlienError::new(ErrorData::ResourceDrift {
                        resource_id: config.id.clone(),
                        message: format!(
                            "GCS bucket '{}' returned different lifecycle rules than the update sent",
                            bucket_name
                        ),
                    }));
                }
                self.lifecycle_revision = Some(lifecycle_revision);
            }
            info!(bucket = %bucket_name, "Bucket configuration updated successfully");
        } else {
            info!(bucket = %bucket_name, "No bucket configuration changes needed");
        }

        Ok(HandlerAction::Continue {
            state: UpdateWaitForActive,
            suggested_delay: Some(Duration::from_secs(2)),
        })
    }

    #[handler(
        state = UpdateWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during update wait".to_string(),
            })
        })?;

        info!(bucket = %bucket_name, "Checking bucket status after update");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Check if bucket is ready after update
        match client.get_bucket(bucket_name.clone()).await {
            Ok(_bucket) => {
                info!(bucket = %bucket_name, "Bucket is ready after update");
                Ok(HandlerAction::Continue {
                    state: UpdateIamPolicy,
                    suggested_delay: None,
                })
            }
            Err(_e) => {
                debug!(bucket = %bucket_name, "Bucket not yet ready after update, waiting");
                Ok(HandlerAction::Continue {
                    state: UpdateWaitForActive,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
        }
    }

    #[handler(
        state = UpdateIamPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during IAM policy update".to_string(),
            })
        })?;

        // Always perform this step to check for IAM policy changes
        if config.public_read != prev_config.public_read {
            info!(bucket = %bucket_name, current = %config.public_read, previous = %prev_config.public_read, "Updating public access");

            let gcp_config = ctx.get_gcp_config()?;
            let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

            if config.public_read {
                // Enable public read access
                info!(bucket = %bucket_name, "Setting IAM policy for public read access");

                // First set uniform bucket-level access
                let mut bucket_patch = Bucket::default();
                bucket_patch.iam_configuration = Some(IamConfiguration {
                    uniform_bucket_level_access: Some(UniformBucketLevelAccess {
                        enabled: true,
                        locked_time: None,
                    }),
                    public_access_prevention: Some("inherited".to_string()),
                });

                client
                    .update_bucket(bucket_name.clone(), bucket_patch)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to enable uniform bucket-level access for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                // Then set IAM policy
                let iam_policy = IamPolicy {
                    version: Some(1),
                    bindings: vec![Binding {
                        role: "roles/storage.objectViewer".to_string(),
                        members: vec!["allUsers".to_string()],
                        condition: None,
                    }],
                    etag: None,
                    kind: Some("storage#policy".to_string()),
                    resource_id: Some(bucket_name.clone()),
                };

                client
                    .set_bucket_iam_policy(bucket_name.clone(), iam_policy)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to set IAM policy for bucket '{}'", bucket_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(bucket = %bucket_name, "IAM policy for public read access set successfully");
            } else {
                // Remove public read access
                info!(bucket = %bucket_name, "Removing public read access");

                // First disable uniform bucket-level access
                let mut bucket_patch = Bucket::default();
                bucket_patch.iam_configuration = Some(IamConfiguration {
                    uniform_bucket_level_access: Some(UniformBucketLevelAccess {
                        enabled: false,
                        locked_time: None,
                    }),
                    public_access_prevention: Some("enforced".to_string()),
                });

                client
                    .update_bucket(bucket_name.clone(), bucket_patch)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to disable uniform bucket-level access for bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                // Then remove allUsers from IAM policy
                match client.get_bucket_iam_policy(bucket_name.clone()).await {
                    Ok(mut current_policy) => {
                        current_policy.bindings.retain(|binding| {
                            !(binding.role == "roles/storage.objectViewer"
                                && binding.members.contains(&"allUsers".to_string()))
                        });

                        client
                            .set_bucket_iam_policy(bucket_name.clone(), current_policy)
                            .await
                            .context(ErrorData::CloudPlatformError {
                                message: format!(
                                    "Failed to update IAM policy for bucket '{}'",
                                    bucket_name
                                ),
                                resource_id: Some(config.id.clone()),
                            })?;
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        // Policy doesn't exist, nothing to remove
                        debug!(bucket = %bucket_name, "No IAM policy found to remove");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to get IAM policy for bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }

                info!(bucket = %bucket_name, "Public read access removed successfully");
            }
        } else {
            info!(bucket = %bucket_name, "No IAM policy changes needed");
        }

        Ok(HandlerAction::Continue {
            state: UpdatingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Bucket name not set in state during resource permissions update"
                    .to_string(),
            })
        })?;

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        info!(bucket = %bucket_name, "Re-applying resource-scoped permissions after update");
        self.apply_resource_scoped_permissions(ctx, bucket_name, &client)
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
        let config = ctx.desired_resource_config::<Storage>()?;

        // Handle case where bucket_name is not set (e.g., creation failed early)
        let bucket_name = match self.bucket_name.as_ref() {
            Some(name) => name,
            None => {
                // No bucket was created, nothing to delete
                info!(resource_id=%config.id, "No GCS bucket to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.bucket_name = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(bucket = %bucket_name, "Starting bucket deletion by emptying contents");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Best effort: try to empty the bucket first
        match client.empty_bucket(bucket_name.clone()).await {
            Ok(_) => {
                info!(bucket = %bucket_name, "Bucket emptied successfully");
            }
            Err(e) => {
                // Log but continue - bucket might not exist or might already be empty
                info!(bucket = %bucket_name, error=?e, "Could not empty bucket, continuing with deletion attempt");
            }
        }

        Ok(HandlerAction::Continue {
            state: DeleteBucket,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteBucket,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_bucket(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;

        // Handle case where bucket_name is not set (defensive programming)
        let bucket_name = match self.bucket_name.as_ref() {
            Some(name) => name,
            None => {
                // This should not happen if delete_start worked correctly, but handle gracefully
                warn!(resource_id=%config.id, "No bucket name set during delete_bucket, proceeding to Deleted state");

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        info!(bucket = %bucket_name, "Deleting GCS bucket");

        let gcp_config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;

        // Best effort: try to delete the bucket
        match client.delete_bucket(bucket_name.clone()).await {
            Ok(()) => {
                info!(bucket = %bucket_name, "GCS bucket deleted successfully");
            }
            Err(e) => {
                // Check if it's a resource not found error (bucket doesn't exist)
                match &e.error {
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. }) => {
                        warn!(bucket = %bucket_name, "Bucket already deleted or never existed");
                    }
                    _ => {
                        return Err(e).context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete bucket '{}'. Non-transient error; not treating as already-deleted.", bucket_name),
                            resource_id: Some(config.id.clone()),
                        })?;
                    }
                }
            }
        }

        self.bucket_name = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
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

    /// Schedules a lifecycle rewrite when GCS has not confirmed the desired rules from this
    /// controller, such as a bucket whose prefixed rules an older version sent without the
    /// prefix. Without this, a deployment whose storage config never changes keeps the
    /// bucket-wide rules forever.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let config = ctx.desired_resource_config::<Storage>()?;
        // Setup owns a Frozen bucket after handoff: Terraform sent the prefixes itself, and the
        // runtime may not rewrite the bucket. Direct setup runs this controller with setup
        // credentials, so it can repair a bucket this controller created.
        let setup_owned = ctx
            .desired_stack
            .resources
            .get(&config.id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen);
        if setup_owned && ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup {
            return Ok(false);
        }
        match &self.lifecycle_revision {
            Some(applied) => {
                let lifecycle = gcs_lifecycle(&config.id, &config.lifecycle_rules)?;
                Ok(applied != &gcs_lifecycle_revision(&config.id, &lifecycle)?)
            }
            // Older versions sent everything except the prefix correctly.
            None => Ok(config
                .lifecycle_rules
                .iter()
                .any(|rule| rule.prefix.is_some())),
        }
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Bucket name is always known from either bucket_name field or resource ID
        Some(ResourceOutputs::new(StorageOutputs {
            bucket_name: self
                .bucket_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::StorageBinding;

        if let Some(bucket_name) = &self.bucket_name {
            let binding = StorageBinding::gcs(bucket_name.clone());
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

/// Builds the Cloud Storage lifecycle configuration for a storage's rules. Each rule deletes
/// objects older than `days`; a rule with a prefix applies only to object names starting with
/// it. No rules gives an empty configuration, which a patch uses to remove all rules.
fn gcs_lifecycle(resource_id: &str, rules: &[StorageLifecycleRule]) -> Result<Lifecycle> {
    if rules.is_empty() {
        return Ok(Lifecycle { rule: None });
    }
    let mut gcs_rules = Vec::with_capacity(rules.len());
    for rule in rules {
        let age = i32::try_from(rule.days).map_err(|_| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "lifecycle rule days {} exceeds the Cloud Storage maximum of {}",
                    rule.days,
                    i32::MAX
                ),
                resource_id: Some(resource_id.to_string()),
            })
        })?;
        gcs_rules.push(LifecycleRule {
            action: Some(LifecycleAction {
                action_type: "Delete".to_string(),
                storage_class: None,
            }),
            condition: Some(LifecycleCondition {
                age: Some(age),
                matches_prefix: rule.prefix.clone().map(|prefix| vec![prefix]),
                ..Default::default()
            }),
        });
    }
    Ok(Lifecycle {
        rule: Some(gcs_rules),
    })
}

/// Identifies a lifecycle configuration by the request body that sends it.
fn gcs_lifecycle_revision(resource_id: &str, lifecycle: &Lifecycle) -> Result<String> {
    let body = serde_json::to_vec(lifecycle).into_alien_error().context(
        ErrorData::ResourceStateSerializationFailed {
            resource_id: resource_id.to_string(),
            message: "Failed to serialize the bucket lifecycle configuration".to_string(),
        },
    )?;
    Ok(format!("{:x}", Sha256::digest(body)))
}

/// Whether the lifecycle GCS returned for a bucket has the expected rules, in order. Compares
/// only the fields this controller sends.
fn gcs_lifecycle_matches(observed: Option<&Lifecycle>, expected: &Lifecycle) -> bool {
    fn fields(lifecycle: Option<&Lifecycle>) -> Vec<(Option<&str>, Option<i32>, Option<&[String]>)> {
        lifecycle
            .and_then(|lifecycle| lifecycle.rule.as_deref())
            .unwrap_or_default()
            .iter()
            .map(|rule| {
                let condition = rule.condition.as_ref();
                (
                    rule.action.as_ref().map(|action| action.action_type.as_str()),
                    condition.and_then(|condition| condition.age),
                    condition.and_then(|condition| condition.matches_prefix.as_deref()),
                )
            })
            .collect()
    }
    fields(observed) == fields(Some(expected))
}

fn emit_gcp_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    bucket_name: &str,
    bucket: Bucket,
) {
    let observed_bucket_name = bucket
        .name
        .clone()
        .unwrap_or_else(|| bucket_name.to_string());
    let lifecycle_rule_count = bucket
        .lifecycle
        .as_ref()
        .and_then(|lifecycle| lifecycle.rule.as_ref())
        .map(|rules| rules.len() as u64);
    let lifecycle_present = lifecycle_rule_count.map(|count| count > 0).unwrap_or(false);
    let versioning_enabled = bucket
        .versioning
        .as_ref()
        .map(|versioning| versioning.enabled);
    let iam_configuration = bucket.iam_configuration.as_ref();
    let uniform_bucket_level_access = iam_configuration
        .and_then(|configuration| configuration.uniform_bucket_level_access.as_ref());
    let public_access_prevention =
        iam_configuration.and_then(|configuration| configuration.public_access_prevention.clone());
    let default_kms_key_name = bucket
        .encryption
        .as_ref()
        .and_then(|encryption| encryption.default_kms_key_name.clone());
    let encryption_config_present = default_kms_key_name.is_some();
    let retention_policy = bucket.retention_policy.as_ref();
    let soft_delete_policy = bucket.soft_delete_policy.as_ref();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Storage::RESOURCE_TYPE,
        controller_platform: Platform::Gcp,
        backend: HeartbeatBackend::Gcp,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Storage(StorageHeartbeatData::GcpCloudStorage(
            GcpCloudStorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!(
                        "GCS bucket '{}' metadata is reachable",
                        observed_bucket_name
                    )),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                name: observed_bucket_name,
                bucket_id: bucket.id,
                location: bucket.location,
                location_type: bucket.location_type,
                storage_class: bucket.storage_class,
                versioning_enabled,
                lifecycle_present,
                lifecycle_rule_count,
                retention_policy_effective_time: retention_policy
                    .and_then(|policy| policy.effective_time.clone()),
                retention_policy_is_locked: retention_policy.and_then(|policy| policy.is_locked),
                retention_period: retention_policy
                    .and_then(|policy| policy.retention_period.clone()),
                soft_delete_retention_duration_seconds: soft_delete_policy
                    .and_then(|policy| policy.retention_duration_seconds.clone()),
                soft_delete_effective_time: soft_delete_policy
                    .and_then(|policy| policy.effective_time.clone()),
                uniform_bucket_level_access_enabled: uniform_bucket_level_access
                    .map(|access| access.enabled),
                uniform_bucket_level_access_locked_time: uniform_bucket_level_access
                    .and_then(|access| access.locked_time.clone()),
                public_access_prevention,
                encryption_config_present,
                default_kms_key_name,
            },
        )),
        raw: vec![],
    });
}

impl GcpStorageController {
    /// Applies resource-scoped permissions to the bucket from stack permission profiles.
    ///
    /// Collects custom-role bindings and applies them to the bucket.
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        bucket_name: &str,
        client: &Arc<dyn alien_gcp_clients::gcs::GcsApi>,
    ) -> Result<()> {
        use crate::core::ResourcePermissionsHelper;

        let config = ctx.desired_resource_config::<Storage>()?;

        // Collect resource-scoped custom-role bindings.
        let mut all_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_resource_scoped_bindings(
            ctx,
            &config.id,
            bucket_name,
            "storage",
            &mut all_bindings,
        )
        .await?;

        // This identity is wholly Alien-owned. Remove its previous bucket grants before adding
        // the desired ones so direct setup updates can revoke `remoteAccess` without leaving
        // stale data-plane access behind.
        let remote_bindings_member = ctx
            .state
            .resources
            .values()
            .find(|state| state.resource_type == alien_core::RemoteBindings::RESOURCE_TYPE.as_ref())
            .and_then(|state| state.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<alien_core::RemoteBindingsOutputs>())
            .map(|outputs| format!("serviceAccount:{}", outputs.access_configuration));

        // Apply desired bindings, or remove stale Remote Bindings grants when opt-in was removed.
        if !all_bindings.is_empty() || remote_bindings_member.is_some() {
            info!(
                bucket = %bucket_name,
                bindings_count = all_bindings.len(),
                "Applying resource-scoped IAM policy to bucket"
            );

            // Get existing IAM policy to merge with new bindings
            let mut existing_policy = client
                .get_bucket_iam_policy(bucket_name.to_string())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get bucket IAM policy for '{}' before applying resource-scoped permissions. Refusing to proceed to avoid overwriting existing bindings.", bucket_name),
                    resource_id: Some(config.id.clone()),
                })?;

            if let Some(member) = remote_bindings_member.as_ref() {
                for binding in &mut existing_policy.bindings {
                    binding.members.retain(|candidate| candidate != member);
                }
                existing_policy
                    .bindings
                    .retain(|binding| !binding.members.is_empty());
            }

            // Merge new bindings with existing ones
            existing_policy.bindings.extend(all_bindings);

            // GCP requires version 3 when any binding has a condition
            existing_policy.version = Some(3);

            // Apply the updated IAM policy
            client
                .set_bucket_iam_policy(bucket_name.to_string(), existing_policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply resource-scoped IAM policy to bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(bucket = %bucket_name, "Resource-scoped IAM policy applied successfully");
        } else {
            info!(bucket = %bucket_name, "No resource-scoped permissions to apply");
        }

        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: GcpStorageState::Ready,
            bucket_name: Some(get_gcp_bucket_name("test-stack", storage_name)),
            lifecycle_revision: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # GCP Storage Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        LifecycleRule as AlienLifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs,
    };
    use alien_error::AlienError;
    use alien_gcp_clients::gcs::{Bucket, MockGcsApi};
    use alien_gcp_clients::iam::{Binding, IamPolicy, MockIamApi};
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };
    use crate::storage::GcpStorageController;

    // ─────────────── STORAGE FIXTURES ──────────────────────────

    #[fixture]
    fn basic_storage() -> Storage {
        Storage::new("basic-storage".to_string()).build()
    }

    #[fixture]
    fn storage_with_versioning() -> Storage {
        Storage::new("versioned-storage".to_string())
            .versioning(true)
            .build()
    }

    #[fixture]
    fn storage_with_public_read() -> Storage {
        Storage::new("public-storage".to_string())
            .public_read(true)
            .build()
    }

    #[fixture]
    fn storage_with_lifecycle_rules() -> Storage {
        Storage::new("lifecycle-storage".to_string())
            .lifecycle_rules(vec![
                AlienLifecycleRule {
                    prefix: Some("logs/".to_string()),
                    days: 30,
                },
                AlienLifecycleRule {
                    prefix: Some("temp/".to_string()),
                    days: 7,
                },
            ])
            .build()
    }

    #[fixture]
    fn storage_with_all_features() -> Storage {
        Storage::new("full-featured-storage".to_string())
            .versioning(true)
            .public_read(true)
            .lifecycle_rules(vec![AlienLifecycleRule {
                prefix: Some("archive/".to_string()),
                days: 365,
            }])
            .build()
    }

    // ─────────────── MOCK SETUP HELPERS ────────────────────────

    fn create_successful_bucket_response(bucket_name: &str) -> Bucket {
        Bucket {
            name: Some(bucket_name.to_string()),
            location: Some("us-central1".to_string()),
            ..Default::default()
        }
    }

    fn setup_mock_client_for_creation_and_deletion(bucket_name: &str) -> Arc<MockGcsApi> {
        let mut mock_gcs = MockGcsApi::new();

        // Mock successful bucket creation
        let bucket_name = bucket_name.to_string();
        let bucket_name_clone1 = bucket_name.clone();
        let bucket_name_clone2 = bucket_name.clone();
        let bucket_name_clone3 = bucket_name.clone();

        mock_gcs
            .expect_create_bucket()
            .returning(move |_, _| Ok(create_successful_bucket_response(&bucket_name)));

        // Mock bucket status checks
        mock_gcs
            .expect_get_bucket()
            .returning(move |_| Ok(create_successful_bucket_response(&bucket_name_clone1)));

        // Mock bucket updates for IAM and other configurations
        mock_gcs
            .expect_update_bucket()
            .returning(move |_, _| Ok(create_successful_bucket_response(&bucket_name_clone2)));

        // Mock IAM policy operations
        mock_gcs.expect_get_bucket_iam_policy().returning(|_| {
            Ok(IamPolicy {
                version: Some(1),
                bindings: vec![],
                etag: None,
                kind: Some("storage#policy".to_string()),
                resource_id: None,
            })
        });

        mock_gcs.expect_set_bucket_iam_policy().returning(|_, _| {
            Ok(IamPolicy {
                version: Some(1),
                bindings: vec![],
                etag: None,
                kind: Some("storage#policy".to_string()),
                resource_id: None,
            })
        });

        // Mock deletion operations
        mock_gcs.expect_empty_bucket().returning(|_| Ok(()));
        mock_gcs.expect_delete_bucket().returning(|_| Ok(()));

        Arc::new(mock_gcs)
    }

    fn setup_mock_client_for_creation_and_update(bucket_name: &str) -> Arc<MockGcsApi> {
        let mut mock_gcs = MockGcsApi::new();

        // Mock bucket status checks
        let bucket_name = bucket_name.to_string();
        let bucket_name_clone1 = bucket_name.clone();

        mock_gcs
            .expect_get_bucket()
            .returning(move |_| Ok(create_successful_bucket_response(&bucket_name)));

        // GCS answers a patch with the updated bucket, including the lifecycle it stored.
        mock_gcs.expect_update_bucket().returning(move |_, patch| {
            Ok(Bucket {
                lifecycle: patch.lifecycle,
                ..create_successful_bucket_response(&bucket_name_clone1)
            })
        });

        // Mock IAM policy operations for public read changes
        mock_gcs.expect_set_bucket_iam_policy().returning(|_, _| {
            Ok(IamPolicy {
                version: Some(1),
                bindings: vec![],
                etag: None,
                kind: Some("storage#policy".to_string()),
                resource_id: None,
            })
        });

        mock_gcs.expect_get_bucket_iam_policy().returning(|_| {
            Ok(IamPolicy {
                version: Some(1),
                bindings: vec![],
                etag: None,
                kind: Some("storage#policy".to_string()),
                resource_id: None,
            })
        });

        Arc::new(mock_gcs)
    }

    fn setup_mock_client_for_best_effort_deletion(_bucket_name: &str) -> Arc<MockGcsApi> {
        let mut mock_gcs = MockGcsApi::new();

        // Mock empty bucket failure (bucket doesn't exist)
        mock_gcs.expect_empty_bucket().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "GCS Bucket".to_string(),
                    resource_name: "test-bucket".to_string(),
                },
            ))
        });

        // Mock successful bucket deletion
        mock_gcs.expect_delete_bucket().returning(|_| Ok(()));

        Arc::new(mock_gcs)
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
        Arc::new(MockIamApi::new())
    }

    fn setup_mock_service_provider(mock_gcs: Arc<MockGcsApi>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(mock_gcs.clone()));

        // Mock IAM client for resource-scoped permissions.
        let mock_iam = create_gcp_iam_mock_for_resource_permissions();
        mock_provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        Arc::new(mock_provider)
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())]
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())]
    #[case::all_features(storage_with_all_features())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] storage: Storage) {
        let bucket_name = format!("test-{}", storage.id);
        let mock_gcs = setup_mock_client_for_creation_and_deletion(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_gcs);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(GcpStorageController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let storage_outputs = outputs.downcast_ref::<StorageOutputs>().unwrap();
        assert!(storage_outputs.bucket_name.starts_with("test-"));

        // Delete the storage
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_versioned(basic_storage(), storage_with_versioning())]
    #[case::versioned_to_public(storage_with_versioning(), storage_with_public_read())]
    #[case::public_to_lifecycle(storage_with_public_read(), storage_with_lifecycle_rules())]
    #[case::lifecycle_to_all_features(storage_with_lifecycle_rules(), storage_with_all_features())]
    #[case::all_features_to_basic(storage_with_all_features(), basic_storage())]
    #[tokio::test]
    async fn test_update_flow_succeeds(#[case] from_storage: Storage, #[case] to_storage: Storage) {
        // Ensure both storages have the same ID for valid updates
        let storage_id = "test-update-storage".to_string();
        let mut from_storage = from_storage;
        from_storage.id = storage_id.clone();

        let mut to_storage = to_storage;
        to_storage.id = storage_id.clone();

        let bucket_name = format!("test-{}", storage_id);
        let mock_gcs = setup_mock_client_for_creation_and_update(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_gcs);

        // Start with the "from" storage in Ready state
        let ready_controller = GcpStorageController::mock_ready(&storage_id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_storage)
            .controller(ready_controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new storage
        executor.update(to_storage).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_storage())]
    #[case::versioning(storage_with_versioning())]
    #[case::public_read(storage_with_public_read())]
    #[case::lifecycle_rules(storage_with_lifecycle_rules())]
    #[tokio::test]
    async fn test_best_effort_deletion_when_bucket_missing(#[case] storage: Storage) {
        let bucket_name = format!("test-{}", storage.id);
        let mock_gcs = setup_mock_client_for_best_effort_deletion(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_gcs);

        // Start with a ready controller
        let ready_controller = GcpStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though emptying fails
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_bucket_delete_fails() {
        let storage = basic_storage();
        let bucket_name = format!("test-{}", storage.id);

        let mut mock_gcs = MockGcsApi::new();

        // Mock successful empty bucket
        mock_gcs.expect_empty_bucket().returning(|_| Ok(()));

        // Mock bucket deletion failure (bucket doesn't exist)
        mock_gcs.expect_delete_bucket().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "GCS Bucket".to_string(),
                    resource_name: "test-bucket".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_gcs));

        // Start with a ready controller
        let ready_controller = GcpStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even though bucket deletion fails
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies correct bucket naming convention
    #[tokio::test]
    async fn test_bucket_naming_validation() {
        let storage = Storage::new("my-awesome-storage".to_string()).build();

        let mut mock_gcs = MockGcsApi::new();

        // Validate that bucket names are prefixed correctly
        mock_gcs
            .expect_create_bucket()
            .withf(|bucket_name, _| bucket_name == "test-my-awesome-storage")
            .returning(|bucket_name, _| Ok(create_successful_bucket_response(&bucket_name)));

        // Mock other required methods
        mock_gcs
            .expect_get_bucket()
            .returning(|_| Ok(create_successful_bucket_response("test-my-awesome-storage")));
        mock_gcs
            .expect_update_bucket()
            .returning(|_, _| Ok(create_successful_bucket_response("test-my-awesome-storage")));
        mock_gcs
            .expect_set_bucket_iam_policy()
            .returning(|_, _| Ok(IamPolicy::default()));

        let mock_provider = setup_mock_service_provider(Arc::new(mock_gcs));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(GcpStorageController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies versioning configuration is applied correctly
    #[tokio::test]
    async fn test_versioning_configuration() {
        let storage = Storage::new("versioning-test".to_string())
            .versioning(true)
            .build();

        let mut mock_gcs = MockGcsApi::new();

        // Validate that versioning is enabled in bucket configuration
        mock_gcs
            .expect_create_bucket()
            .withf(|_bucket_name, bucket| {
                if let Some(versioning) = &bucket.versioning {
                    if !versioning.enabled {
                        eprintln!("Expected versioning to be enabled");
                        return false;
                    }
                    true
                } else {
                    eprintln!("Expected versioning configuration");
                    false
                }
            })
            .returning(|bucket_name, _| Ok(create_successful_bucket_response(&bucket_name)));

        mock_gcs
            .expect_get_bucket()
            .returning(|_| Ok(create_successful_bucket_response("test-versioning-test")));
        mock_gcs
            .expect_update_bucket()
            .returning(|_, _| Ok(create_successful_bucket_response("test-versioning-test")));
        mock_gcs
            .expect_set_bucket_iam_policy()
            .returning(|_, _| Ok(IamPolicy::default()));

        let mock_provider = setup_mock_service_provider(Arc::new(mock_gcs));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(GcpStorageController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public read configuration generates correct IAM policy
    #[tokio::test]
    async fn test_public_read_iam_policy_generation() {
        let storage = Storage::new("public-test".to_string())
            .public_read(true)
            .build();

        let mut mock_gcs = MockGcsApi::new();

        mock_gcs
            .expect_create_bucket()
            .returning(|bucket_name, _| Ok(create_successful_bucket_response(&bucket_name)));

        mock_gcs
            .expect_get_bucket()
            .returning(|_| Ok(create_successful_bucket_response("test-public-test")));

        // Validate uniform bucket-level access configuration
        mock_gcs
            .expect_update_bucket()
            .withf(|_bucket_name, bucket| {
                if let Some(iam_config) = &bucket.iam_configuration {
                    if let Some(ubla) = &iam_config.uniform_bucket_level_access {
                        if !ubla.enabled {
                            eprintln!("Expected uniform bucket-level access to be enabled");
                            return false;
                        }
                    } else {
                        eprintln!("Expected uniform bucket-level access configuration");
                        return false;
                    }

                    if let Some(pap) = &iam_config.public_access_prevention {
                        if pap != "inherited" {
                            eprintln!(
                                "Expected public access prevention to be 'inherited', got '{}'",
                                pap
                            );
                            return false;
                        }
                    } else {
                        eprintln!("Expected public access prevention configuration");
                        return false;
                    }

                    true
                } else {
                    eprintln!("Expected IAM configuration");
                    false
                }
            })
            .returning(|bucket_name, _| Ok(create_successful_bucket_response(&bucket_name)));

        // Return empty policy for the read-modify-write pattern
        mock_gcs
            .expect_get_bucket_iam_policy()
            .returning(|_| Ok(IamPolicy::default()));

        // Validate IAM policy for public read access
        mock_gcs
            .expect_set_bucket_iam_policy()
            .withf(|_bucket_name, iam_policy| {
                // Should have the correct binding for public read
                if iam_policy.bindings.len() != 1 {
                    eprintln!("Expected 1 binding, got {}", iam_policy.bindings.len());
                    return false;
                }

                let binding = &iam_policy.bindings[0];
                if binding.role != "roles/storage.objectViewer" {
                    eprintln!(
                        "Expected role 'roles/storage.objectViewer', got '{}'",
                        binding.role
                    );
                    return false;
                }

                if binding.members.len() != 1 || binding.members[0] != "allUsers" {
                    eprintln!("Expected members ['allUsers'], got {:?}", binding.members);
                    return false;
                }

                true
            })
            .returning(|_, _| {
                Ok(IamPolicy {
                    version: Some(1),
                    bindings: vec![Binding {
                        role: "roles/storage.objectViewer".to_string(),
                        members: vec!["allUsers".to_string()],
                        condition: None,
                    }],
                    etag: None,
                    kind: Some("storage#policy".to_string()),
                    resource_id: None,
                })
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_gcs));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(GcpStorageController::default())
            .platform(Platform::Gcp)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}

#[cfg(test)]
mod lifecycle_prefix_tests {
    //! The lifecycle rules this controller sends to Cloud Storage. A rule's prefix must reach
    //! GCS as `condition.matchesPrefix`; without it the rule expires every object in the
    //! bucket. These tests assert the exact JSON body sent to GCS.

    use std::sync::{Arc, Mutex};

    use alien_core::{
        ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
        GcpClientConfig, InitialSetupAuthority, LifecycleRule, Platform, Resource,
        ResourceDefinition, ResourceLifecycle, ResourceStatus, Stack, StackResourceState,
        StackSettings, StackState, Storage,
    };
    use alien_gcp_clients::{
        gcs::{Bucket, MockGcsApi},
        iam::MockIamApi,
        GcpClientConfigExt as _,
    };
    use serde_json::{json, Value};

    use crate::core::{
        controller_test::SingleControllerExecutor, MockPlatformServiceProvider,
        PlatformServiceProvider, StackExecutor, StackResourceStateExt,
    };
    use crate::storage::{GcpStorageController, GcpStorageState};

    const BUCKET: &str = "test-tmp-storage";

    fn storage(rules: Vec<LifecycleRule>) -> Storage {
        Storage::new("tmp-storage".to_string())
            .lifecycle_rules(rules)
            .build()
    }

    fn rule(days: u32, prefix: Option<&str>) -> LifecycleRule {
        LifecycleRule {
            days,
            prefix: prefix.map(str::to_string),
        }
    }

    fn delete_rule(age: u32, prefix: Option<&str>) -> Value {
        let mut condition = json!({ "age": age });
        if let Some(prefix) = prefix {
            condition["matchesPrefix"] = json!([prefix]);
        }
        json!({ "action": { "type": "Delete" }, "condition": condition })
    }

    /// GCS answers a bucket insert or patch with the bucket, including the lifecycle it
    /// stored. `respond` decides what the mock returns for a request.
    fn gcs(
        bodies: Arc<Mutex<Vec<Value>>>,
        respond: fn(Bucket) -> Bucket,
    ) -> Arc<dyn PlatformServiceProvider> {
        let mut gcs = MockGcsApi::new();
        let created = bodies.clone();
        gcs.expect_create_bucket().returning(move |name, bucket| {
            created
                .lock()
                .unwrap()
                .push(serde_json::to_value(&bucket).unwrap());
            Ok(Bucket {
                name: Some(name),
                ..respond(bucket)
            })
        });
        let patched = bodies;
        gcs.expect_update_bucket().returning(move |name, bucket| {
            patched
                .lock()
                .unwrap()
                .push(serde_json::to_value(&bucket).unwrap());
            Ok(Bucket {
                name: Some(name),
                ..respond(bucket)
            })
        });
        gcs.expect_get_bucket().returning(|name| {
            Ok(Bucket {
                name: Some(name),
                ..Default::default()
            })
        });
        let gcs = Arc::new(gcs);
        let iam = Arc::new(MockIamApi::new());
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(gcs.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        Arc::new(provider)
    }

    fn echo(bucket: Bucket) -> Bucket {
        bucket
    }

    /// The controller state an older version saved for a Ready bucket: no lifecycle revision.
    fn previous_version_ready_controller() -> GcpStorageController {
        let mut saved = serde_json::to_value(GcpStorageController {
            state: GcpStorageState::Ready,
            bucket_name: Some(BUCKET.to_string()),
            lifecycle_revision: Some("unused".to_string()),
            _internal_stay_count: None,
        })
        .unwrap();
        saved.as_object_mut().unwrap().remove("lifecycleRevision");
        serde_json::from_value(saved).unwrap()
    }

    #[tokio::test]
    async fn create_sends_each_rule_prefix_as_matches_prefix() {
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage(vec![rule(1, Some("tmp/")), rule(365, None)]))
            .controller(GcpStorageController::default())
            .platform(Platform::Gcp)
            .service_provider(gcs(bodies.clone(), echo))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 1, "create sends one request: {bodies:?}");
        assert_eq!(
            bodies[0]["lifecycle"],
            json!({ "rule": [delete_rule(1, Some("tmp/")), delete_rule(365, None)] })
        );
        assert!(executor
            .internal_state::<GcpStorageController>()
            .unwrap()
            .lifecycle_revision
            .is_some());
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn update_sends_each_rule_prefix_as_matches_prefix() {
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage(vec![rule(30, Some("logs/"))]))
            .controller(GcpStorageController::mock_ready("tmp-storage"))
            .platform(Platform::Gcp)
            .service_provider(gcs(bodies.clone(), echo))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor
            .update(storage(vec![rule(7, Some("cache/")), rule(1, Some("tmp/"))]))
            .unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            *bodies.lock().unwrap(),
            vec![json!({
                "lifecycle": {
                    "rule": [delete_rule(7, Some("cache/")), delete_rule(1, Some("tmp/"))]
                }
            })]
        );
        assert!(!executor.needs_update().unwrap());
    }

    /// A bucket configured by an older version carries the prefixed rule without its prefix.
    /// The desired config has not changed, so only `needs_update` can schedule the repair.
    /// This drives the executor's own plan and step, as a deployment update does.
    #[tokio::test]
    async fn unchanged_prefixed_rules_are_rewritten_on_buckets_from_older_versions() {
        let desired = storage(vec![rule(1, Some("tmp/"))]);
        let stack = Stack::new("test".to_string())
            .add(desired.clone(), ResourceLifecycle::Live)
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .build();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let executor = StackExecutor::builder(
            &stack,
            ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
        )
        .deployment_config(&config)
        .service_provider(gcs(bodies.clone(), echo))
        .build()
        .unwrap();
        let mut state = StackState::with_resource_prefix(Platform::Gcp, "test".to_string());
        let mut resource = StackResourceState::new_pending(
            Storage::RESOURCE_TYPE.to_string(),
            Resource::new(desired),
            Some(ResourceLifecycle::Live),
            vec![],
        );
        resource.status = ResourceStatus::Running;
        resource
            .set_internal_controller(Some(Box::new(previous_version_ready_controller())))
            .unwrap();
        state.resources.insert("tmp-storage".to_string(), resource);

        assert!(executor
            .plan(&state)
            .unwrap()
            .updates
            .contains_key("tmp-storage"));
        for _ in 0..10 {
            state = executor.step(state).await.unwrap().next_state;
            if state.resources["tmp-storage"].status == ResourceStatus::Running {
                break;
            }
        }

        assert_eq!(
            state.resources["tmp-storage"].status,
            ResourceStatus::Running
        );
        assert_eq!(
            *bodies.lock().unwrap(),
            vec![json!({ "lifecycle": { "rule": [delete_rule(1, Some("tmp/"))] } })]
        );
        assert!(
            !executor
                .plan(&state)
                .unwrap()
                .updates
                .contains_key("tmp-storage"),
            "the repair is not scheduled again once GCS confirmed the rules"
        );
    }

    #[tokio::test]
    async fn buckets_from_older_versions_without_prefixed_rules_need_no_repair() {
        for rules in [vec![], vec![rule(30, None)]] {
            let executor = SingleControllerExecutor::builder()
                .resource(storage(rules))
                .controller(previous_version_ready_controller())
                .platform(Platform::Gcp)
                .service_provider(gcs(Arc::default(), echo))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            assert!(!executor.needs_update().unwrap());
        }
    }

    /// After handoff the runtime may not rewrite a setup-owned bucket, and Terraform sent the
    /// prefixes itself. Direct setup created the bucket with this controller, so it repairs it.
    #[tokio::test]
    async fn frozen_buckets_are_repaired_only_under_direct_setup() {
        for (authority, expected) in [
            (InitialSetupAuthority::ImportedHandoff, false),
            (InitialSetupAuthority::DirectSetup, true),
        ] {
            let executor = SingleControllerExecutor::builder()
                .resource(storage(vec![rule(1, Some("tmp/"))]))
                .controller(previous_version_ready_controller())
                .platform(Platform::Gcp)
                .resource_lifecycle(ResourceLifecycle::Frozen)
                .initial_setup_authority(authority)
                .service_provider(gcs(Arc::default(), echo))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            assert_eq!(executor.needs_update().unwrap(), expected, "{authority:?}");
        }
    }

    /// If GCS stores something other than what was sent, the update fails loudly instead of
    /// recording the rules as applied.
    #[tokio::test]
    async fn update_fails_when_gcs_does_not_store_the_prefix() {
        fn drop_prefixes(mut bucket: Bucket) -> Bucket {
            for rule in bucket
                .lifecycle
                .iter_mut()
                .flat_map(|lifecycle| lifecycle.rule.iter_mut().flatten())
            {
                rule.condition.as_mut().unwrap().matches_prefix = None;
            }
            bucket
        }
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage(vec![rule(1, Some("tmp/"))]))
            .controller(previous_version_ready_controller())
            .platform(Platform::Gcp)
            .service_provider(gcs(Arc::default(), drop_prefixes))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor
            .update(storage(vec![rule(2, Some("tmp/"))]))
            .unwrap();
        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a lifecycle GCS did not store must fail the update");

        assert_eq!(error.code, "RESOURCE_DRIFT");
        assert!(executor
            .internal_state::<GcpStorageController>()
            .unwrap()
            .lifecycle_revision
            .is_none());
        assert!(executor.needs_update().unwrap());
    }
}

use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::error::{ErrorData, Result};
use alien_core::{
    GcpCloudStorageHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_macros::controller;
use chrono::Utc;
use google_cloud_storage::model::{
    bucket::{
        iam_config::UniformBucketLevelAccess,
        lifecycle::{
            rule::{Action as LifecycleAction, Condition as LifecycleCondition},
            Rule as LifecycleRule,
        },
        IamConfig as IamConfiguration, Lifecycle, Versioning,
    },
    Bucket,
};

use crate::core::{Binding, GcsApi, Policy, ResourceControllerContext};

/// Generates the full, prefixed GCP bucket name.
fn get_gcp_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

fn is_remote_resource_not_found(error: &AlienError<ErrorData>) -> bool {
    matches!(
        error.code.as_str(),
        "REMOTE_RESOURCE_NOT_FOUND" | "CLOUD_RESOURCE_NOT_FOUND"
    )
}

fn none_if_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn bucket_name_from_resource_name(resource_name: &str) -> Option<String> {
    resource_name
        .rsplit_once("/buckets/")
        .map(|(_, bucket_name)| bucket_name.to_string())
        .filter(|bucket_name| !bucket_name.is_empty())
}

fn observed_bucket_name(bucket: &Bucket, fallback: &str) -> String {
    none_if_empty(bucket.bucket_id.clone())
        .or_else(|| bucket_name_from_resource_name(&bucket.name))
        .or_else(|| none_if_empty(bucket.name.clone()))
        .unwrap_or_else(|| fallback.to_string())
}

fn timestamp_to_string(timestamp: wkt::Timestamp) -> String {
    String::from(timestamp)
}

fn duration_seconds(duration: wkt::Duration) -> String {
    duration.seconds().to_string()
}

fn gcs_lifecycle_rules(rules: &[alien_core::LifecycleRule]) -> Vec<LifecycleRule> {
    rules
        .iter()
        .map(|rule| {
            let action = LifecycleAction::new().set_type("Delete");
            let mut condition = LifecycleCondition::new().set_age_days(rule.days as i32);

            if let Some(prefix) = &rule.prefix {
                condition = condition.set_matches_prefix([prefix.clone()]);
            }

            LifecycleRule::new()
                .set_action(action)
                .set_condition(condition)
        })
        .collect()
}

fn gcs_lifecycle(rules: &[alien_core::LifecycleRule]) -> Lifecycle {
    Lifecycle::new().set_rule(gcs_lifecycle_rules(rules))
}

fn gcs_iam_patch(uniform_bucket_level_access: bool, public_access_prevention: &str) -> Bucket {
    Bucket::new().set_iam_config(
        IamConfiguration::new()
            .set_uniform_bucket_level_access(
                UniformBucketLevelAccess::new().set_enabled(uniform_bucket_level_access),
            )
            .set_public_access_prevention(public_access_prevention),
    )
}

#[controller]
pub struct GcpStorageController {
    /// The actual bucket name (includes stack name prefix).
    /// This is None until the bucket is created or imported.
    pub(crate) bucket_name: Option<String>,
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
        let mut bucket = Bucket::new().set_location(gcp_config.region.clone());

        // Configure versioning if enabled
        if config.versioning {
            bucket = bucket.set_versioning(Versioning::new().set_enabled(true));
        }

        // Configure lifecycle rules if any
        if !config.lifecycle_rules.is_empty() {
            bucket = bucket.set_lifecycle(gcs_lifecycle(&config.lifecycle_rules));
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

        self.bucket_name = Some(observed_bucket_name(&created_bucket, &bucket_name));

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
            let bucket_patch = gcs_iam_patch(true, "inherited");

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
                    existing_policy.bindings.push(
                        Binding::new()
                            .set_role(public_viewer_role)
                            .set_members([all_users]),
                    );
                }

                existing_policy.version = 3;

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
            bucket_patch =
                bucket_patch.set_versioning(Versioning::new().set_enabled(config.versioning));
            needs_update = true;
        }

        // Check lifecycle rules changes
        if config.lifecycle_rules != prev_config.lifecycle_rules {
            info!(bucket = %bucket_name, rules_count = %config.lifecycle_rules.len(), "Updating lifecycle rules");

            if config.lifecycle_rules.is_empty() {
                bucket_patch = bucket_patch
                    .set_lifecycle(Lifecycle::new().set_rule(Vec::<LifecycleRule>::new()));
            } else {
                bucket_patch = bucket_patch.set_lifecycle(gcs_lifecycle(&config.lifecycle_rules));
            }
            needs_update = true;
        }

        // Apply bucket configuration changes if needed
        if needs_update {
            client
                .update_bucket(bucket_name.clone(), bucket_patch)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update GCS bucket '{}'", bucket_name),
                    resource_id: Some(config.id.clone()),
                })?;
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
                let bucket_patch = gcs_iam_patch(true, "inherited");

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
                let iam_policy = Policy::new().set_version(1).set_bindings([Binding::new()
                    .set_role("roles/storage.objectViewer")
                    .set_members(["allUsers"])]);

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
                let bucket_patch = gcs_iam_patch(false, "enforced");

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
                    Err(e) if is_remote_resource_not_found(&e) => {
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
                if is_remote_resource_not_found(&e) {
                    warn!(bucket = %bucket_name, "Bucket already deleted or never existed");
                } else {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete bucket '{}'. Non-transient error; not treating as already-deleted.", bucket_name),
                        resource_id: Some(config.id.clone()),
                    })?;
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

fn emit_gcp_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    bucket_name: &str,
    bucket: Bucket,
) {
    let observed_bucket_name = observed_bucket_name(&bucket, bucket_name);
    let lifecycle_rule_count = bucket
        .lifecycle
        .as_ref()
        .map(|lifecycle| lifecycle.rule.len() as u64);
    let lifecycle_present = lifecycle_rule_count.map(|count| count > 0).unwrap_or(false);
    let versioning_enabled = bucket
        .versioning
        .as_ref()
        .map(|versioning| versioning.enabled);
    let iam_configuration = bucket.iam_config.as_ref();
    let uniform_bucket_level_access = iam_configuration
        .and_then(|configuration| configuration.uniform_bucket_level_access.as_ref());
    let public_access_prevention = iam_configuration
        .and_then(|configuration| none_if_empty(configuration.public_access_prevention.clone()));
    let default_kms_key_name = bucket
        .encryption
        .as_ref()
        .and_then(|encryption| none_if_empty(encryption.default_kms_key.clone()));
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
                bucket_id: none_if_empty(bucket.bucket_id).or_else(|| none_if_empty(bucket.name)),
                location: none_if_empty(bucket.location),
                location_type: none_if_empty(bucket.location_type),
                storage_class: none_if_empty(bucket.storage_class),
                versioning_enabled,
                lifecycle_present,
                lifecycle_rule_count,
                retention_policy_effective_time: retention_policy
                    .and_then(|policy| policy.effective_time.clone())
                    .map(timestamp_to_string),
                retention_policy_is_locked: retention_policy.map(|policy| policy.is_locked),
                retention_period: retention_policy
                    .and_then(|policy| policy.retention_duration.clone())
                    .map(duration_seconds),
                soft_delete_retention_duration_seconds: soft_delete_policy
                    .and_then(|policy| policy.retention_duration.clone())
                    .map(duration_seconds),
                soft_delete_effective_time: soft_delete_policy
                    .and_then(|policy| policy.effective_time.clone())
                    .map(timestamp_to_string),
                uniform_bucket_level_access_enabled: uniform_bucket_level_access
                    .map(|access| access.enabled),
                uniform_bucket_level_access_locked_time: uniform_bucket_level_access
                    .and_then(|access| access.lock_time.clone())
                    .map(timestamp_to_string),
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
        client: &Arc<dyn GcsApi>,
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

        // Apply the bindings to the bucket if we have any
        if !all_bindings.is_empty() {
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

            // Merge new bindings with existing ones
            existing_policy.bindings.extend(all_bindings);

            // GCP requires version 3 when any binding has a condition
            existing_policy.version = 3;

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

    use alien_core::{
        LifecycleRule as AlienLifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs,
    };
    use alien_error::AlienError;
    use google_cloud_storage::model::Bucket;
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::SingleControllerExecutor, Binding, MockGcpIamApi, MockGcsApi,
        MockPlatformServiceProvider, Policy,
    };
    use crate::error::ErrorData;
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
        Bucket::new()
            .set_name(format!("projects/test-project/buckets/{bucket_name}"))
            .set_bucket_id(bucket_name)
            .set_location("us-central1")
    }

    fn setup_mock_client_for_creation_and_deletion(bucket_name: &str) -> Arc<MockGcsApi> {
        let mut mock_gcs = MockGcsApi::new();

        // Mock successful bucket creation
        let bucket_name = bucket_name.to_string();
        let bucket_name_clone1 = bucket_name.clone();
        let bucket_name_clone2 = bucket_name.clone();

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
        mock_gcs
            .expect_get_bucket_iam_policy()
            .returning(|_| Ok(Policy::new().set_version(1)));

        mock_gcs
            .expect_set_bucket_iam_policy()
            .returning(|_, _| Ok(Policy::new().set_version(1)));

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

        // Mock bucket updates for configuration changes
        mock_gcs
            .expect_update_bucket()
            .returning(move |_, _| Ok(create_successful_bucket_response(&bucket_name_clone1)));

        // Mock IAM policy operations for public read changes
        mock_gcs
            .expect_set_bucket_iam_policy()
            .returning(|_, _| Ok(Policy::new().set_version(1)));

        mock_gcs
            .expect_get_bucket_iam_policy()
            .returning(|_| Ok(Policy::new().set_version(1)));

        Arc::new(mock_gcs)
    }

    fn setup_mock_client_for_best_effort_deletion(_bucket_name: &str) -> Arc<MockGcsApi> {
        let mut mock_gcs = MockGcsApi::new();

        // Mock empty bucket failure (bucket doesn't exist)
        mock_gcs.expect_empty_bucket().returning(|_| {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCS Bucket".to_string(),
                resource_name: "test-bucket".to_string(),
            }))
        });

        // Mock successful bucket deletion
        mock_gcs.expect_delete_bucket().returning(|_| Ok(()));

        Arc::new(mock_gcs)
    }

    fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockGcpIamApi> {
        Arc::new(MockGcpIamApi::new())
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

        let mut mock_gcs = MockGcsApi::new();

        // Mock successful empty bucket
        mock_gcs.expect_empty_bucket().returning(|_| Ok(()));

        // Mock bucket deletion failure (bucket doesn't exist)
        mock_gcs.expect_delete_bucket().returning(|_| {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCS Bucket".to_string(),
                resource_name: "test-bucket".to_string(),
            }))
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
            .returning(|_, _| Ok(Policy::default()));

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

    /// Test that verifies lifecycle rules are converted correctly to GCS format
    #[tokio::test]
    async fn test_lifecycle_rules_generation() {
        let storage = Storage::new("lifecycle-test".to_string())
            .lifecycle_rules(vec![
                AlienLifecycleRule {
                    prefix: Some("logs/".to_string()),
                    days: 30,
                },
                AlienLifecycleRule {
                    prefix: None, // No prefix rule
                    days: 365,
                },
            ])
            .build();

        let mut mock_gcs = MockGcsApi::new();

        // Validate that the generated lifecycle configuration contains expected rules
        mock_gcs
            .expect_create_bucket()
            .withf(|_bucket_name, bucket| {
                if let Some(lifecycle) = &bucket.lifecycle {
                    let rules = &lifecycle.rule;
                    if rules.len() != 2 {
                        eprintln!("Expected 2 lifecycle rules, got {}", rules.len());
                        return false;
                    }

                    let rule1 = &rules[0];
                    if let Some(condition) = &rule1.condition {
                        if condition.age_days != Some(30) {
                            eprintln!("Expected 30 days for rule 1, got {:?}", condition.age_days);
                            return false;
                        }
                        if condition.matches_prefix != ["logs/".to_string()] {
                            eprintln!(
                                "Expected logs/ prefix for rule 1, got {:?}",
                                condition.matches_prefix
                            );
                            return false;
                        }
                    } else {
                        eprintln!("Expected condition for rule 1");
                        return false;
                    }

                    if let Some(action) = &rule1.action {
                        if action.r#type != "Delete" {
                            eprintln!("Expected Delete action, got {}", action.r#type);
                            return false;
                        }
                    } else {
                        eprintln!("Expected action for rule 1");
                        return false;
                    }

                    let rule2 = &rules[1];
                    if let Some(condition) = &rule2.condition {
                        if condition.age_days != Some(365) {
                            eprintln!("Expected 365 days for rule 2, got {:?}", condition.age_days);
                            return false;
                        }
                        if !condition.matches_prefix.is_empty() {
                            eprintln!(
                                "Expected no prefix for rule 2, got {:?}",
                                condition.matches_prefix
                            );
                            return false;
                        }
                    } else {
                        eprintln!("Expected condition for rule 2");
                        return false;
                    }

                    true
                } else {
                    eprintln!("Expected lifecycle configuration");
                    false
                }
            })
            .returning(|bucket_name, _| Ok(create_successful_bucket_response(&bucket_name)));

        mock_gcs
            .expect_get_bucket()
            .returning(|_| Ok(create_successful_bucket_response("test-lifecycle-test")));
        mock_gcs
            .expect_update_bucket()
            .returning(|_, _| Ok(create_successful_bucket_response("test-lifecycle-test")));
        mock_gcs
            .expect_set_bucket_iam_policy()
            .returning(|_, _| Ok(Policy::default()));

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
            .returning(|_, _| Ok(Policy::default()));

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
                if let Some(iam_config) = &bucket.iam_config {
                    if let Some(ubla) = &iam_config.uniform_bucket_level_access {
                        if !ubla.enabled {
                            eprintln!("Expected uniform bucket-level access to be enabled");
                            return false;
                        }
                    } else {
                        eprintln!("Expected uniform bucket-level access configuration");
                        return false;
                    }

                    if iam_config.public_access_prevention != "inherited" {
                        eprintln!(
                            "Expected public access prevention to be 'inherited', got '{}'",
                            iam_config.public_access_prevention
                        );
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
            .returning(|_| Ok(Policy::default()));

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
                Ok(Policy::new().set_version(1).set_bindings([Binding::new()
                    .set_role("roles/storage.objectViewer")
                    .set_members(["allUsers"])]))
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

use alien_error::{AlienError, Context, ContextError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{ResourceOutputs, ResourceStatus, Storage, StorageOutputs};
use alien_gcp_clients::gcs::{
    Bucket, IamConfiguration, Lifecycle, LifecycleAction, LifecycleCondition, LifecycleRule,
    UniformBucketLevelAccess, Versioning,
};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_macros::controller;

/// Generates the full, prefixed GCP bucket name.
fn get_gcp_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
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
        let mut bucket = Bucket::default();

        // Set location from GCP config
        bucket.location = Some(gcp_config.region.clone());

        // Configure versioning if enabled
        if config.versioning {
            bucket.versioning = Some(Versioning { enabled: true });
        }

        // Configure lifecycle rules if any
        if !config.lifecycle_rules.is_empty() {
            let gcs_rules: Vec<LifecycleRule> = config
                .lifecycle_rules
                .iter()
                .map(|rule| {
                    let action = LifecycleAction {
                        action_type: "Delete".to_string(),
                        storage_class: None,
                    };

                    let mut condition = LifecycleCondition::default();
                    condition.age = Some(rule.days as i32);

                    LifecycleRule {
                        action: Some(action),
                        condition: Some(condition),
                    }
                })
                .collect();

            bucket.lifecycle = Some(Lifecycle {
                rule: Some(gcs_rules),
            });
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

        self.bucket_name = Some(created_bucket.name.unwrap_or_else(|| bucket_name.clone()));

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

            // Check if bucket still exists
            client.get_bucket(bucket_name.clone()).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to check GCS bucket during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

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

        // Check lifecycle rules changes
        if config.lifecycle_rules != prev_config.lifecycle_rules {
            info!(bucket = %bucket_name, rules_count = %config.lifecycle_rules.len(), "Updating lifecycle rules");

            if config.lifecycle_rules.is_empty() {
                bucket_patch.lifecycle = Some(Lifecycle { rule: None });
            } else {
                let gcs_rules: Vec<LifecycleRule> = config
                    .lifecycle_rules
                    .iter()
                    .map(|rule| {
                        let action = LifecycleAction {
                            action_type: "Delete".to_string(),
                            storage_class: None,
                        };

                        let mut condition = LifecycleCondition::default();
                        condition.age = Some(rule.days as i32);

                        LifecycleRule {
                            action: Some(action),
                            condition: Some(condition),
                        }
                    })
                    .collect();

                bucket_patch.lifecycle = Some(Lifecycle {
                    rule: Some(gcs_rules),
                });
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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Bucket name is always known from either bucket_name field or resource ID
        Some(ResourceOutputs::new(StorageOutputs {
            bucket_name: self
                .bucket_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        }))
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, StorageBinding};

        if let Some(bucket_name) = &self.bucket_name {
            let binding = StorageBinding::gcs(bucket_name.clone());
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}

impl GcpStorageController {
    /// Applies resource-scoped permissions to the bucket from stack permission profiles.
    ///
    /// This first ensures the required GCP custom roles exist (they are referenced
    /// by the IAM bindings but not created by any other controller), then collects
    /// and applies the bindings to the bucket.
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        bucket_name: &str,
        client: &Arc<dyn alien_gcp_clients::gcs::GcsApi>,
    ) -> Result<()> {
        use crate::core::ResourcePermissionsHelper;

        let config = ctx.desired_resource_config::<Storage>()?;

        // Collect resource-scoped bindings (this also ensures any required custom roles exist)
        let mut all_bindings = Vec::new();
        ResourcePermissionsHelper::collect_gcp_resource_scoped_bindings(
            ctx,
            &config.id,
            bucket_name,
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
    use alien_gcp_clients::gcs::{
        Bucket, IamConfiguration, Lifecycle, LifecycleAction, LifecycleCondition, LifecycleRule,
        ListObjectsResponse, MockGcsApi, Object, UniformBucketLevelAccess, Versioning,
    };
    use alien_gcp_clients::iam::{Binding, IamPolicy};
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

        // Mock bucket updates for configuration changes
        mock_gcs
            .expect_update_bucket()
            .returning(move |_, _| Ok(create_successful_bucket_response(&bucket_name_clone1)));

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

    fn setup_mock_service_provider(mock_gcs: Arc<MockGcsApi>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(mock_gcs.clone()));

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
                    if let Some(rules) = &lifecycle.rule {
                        // Should have 2 rules
                        if rules.len() != 2 {
                            eprintln!("Expected 2 lifecycle rules, got {}", rules.len());
                            return false;
                        }

                        // Check first rule (with prefix)
                        let rule1 = &rules[0];
                        if let Some(condition) = &rule1.condition {
                            if condition.age != Some(30) {
                                eprintln!("Expected 30 days for rule 1, got {:?}", condition.age);
                                return false;
                            }
                        } else {
                            eprintln!("Expected condition for rule 1");
                            return false;
                        }

                        if let Some(action) = &rule1.action {
                            if action.action_type != "Delete" {
                                eprintln!("Expected Delete action, got {}", action.action_type);
                                return false;
                            }
                        } else {
                            eprintln!("Expected action for rule 1");
                            return false;
                        }

                        // Check second rule (no prefix, different age)
                        let rule2 = &rules[1];
                        if let Some(condition) = &rule2.condition {
                            if condition.age != Some(365) {
                                eprintln!("Expected 365 days for rule 2, got {:?}", condition.age);
                                return false;
                            }
                        } else {
                            eprintln!("Expected condition for rule 2");
                            return false;
                        }

                        true
                    } else {
                        eprintln!("Expected lifecycle rules");
                        false
                    }
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

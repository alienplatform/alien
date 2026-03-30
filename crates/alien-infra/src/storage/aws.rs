use alien_error::{AlienError, ContextError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};
use alien_aws_clients::s3::{
    LifecycleConfiguration, LifecycleExpiration, LifecycleRule, LifecycleRuleFilter,
    LifecycleRuleStatus, PublicAccessBlockConfiguration, S3Api, S3Client, VersioningStatus,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Resource, ResourceOutputs, ResourceStatus, Storage, StorageOutputs};
use alien_error::{Context, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use std::sync::Arc;

/// Generates the full, prefixed AWS bucket name.
fn get_aws_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[controller]
pub struct AwsStorageController {
    /// The actual bucket name (includes stack name prefix).
    /// This is None until the bucket is created.
    pub(crate) bucket_name: Option<String>,
}

#[controller]
impl AwsStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;

        // Compute bucket name if not already set (for initial creation or retry)
        let bucket_name = self
            .bucket_name
            .clone()
            .unwrap_or_else(|| get_aws_bucket_name(ctx.resource_prefix, &config.id));

        info!(name=%config.id, bucket=%bucket_name, "Creating S3 bucket");

        // Create the bucket using our custom S3 client
        client
            .create_bucket(&bucket_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create S3 bucket '{}'", bucket_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(bucket=%bucket_name, "S3 bucket created successfully");

        self.bucket_name = Some(bucket_name);

        Ok(HandlerAction::Continue {
            state: ConfiguringVersioning,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringVersioning,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_versioning(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;

        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Bucket name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        if config.versioning {
            info!(bucket=%bucket_name, "Configuring bucket versioning");

            // Configure versioning using our custom S3 client
            client
                .put_bucket_versioning(bucket_name, VersioningStatus::Enabled)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to configure versioning for S3 bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(bucket=%bucket_name, "Bucket versioning configured successfully");
        } else {
            info!(bucket=%bucket_name, "Skipping versioning configuration (not enabled)");
        }

        Ok(HandlerAction::Continue {
            state: ConfiguringPublicAccess,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringPublicAccess,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_public_access(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let storage_config = ctx.desired_resource_config::<Storage>()?;

        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Bucket name not set in state".to_string(),
                resource_id: Some(storage_config.id.clone()),
            })
        })?;

        if storage_config.public_read {
            info!(bucket=%bucket_name, "Configuring public access block");

            // Configure public access block using our custom S3 client
            let public_access_config = PublicAccessBlockConfiguration::builder()
                .block_public_acls(false)
                .block_public_policy(false)
                .ignore_public_acls(false)
                .restrict_public_buckets(false)
                .build();

            client
                .put_public_access_block(bucket_name, public_access_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to configure public access block for S3 bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(storage_config.id.clone()),
                })?;

            info!(bucket=%bucket_name, "Public access block configured successfully");
        } else {
            info!(bucket=%bucket_name, "Skipping public access configuration (not enabled)");
        }

        Ok(HandlerAction::Continue {
            state: ConfiguringPublicPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringPublicPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_public_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;

        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Bucket name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        if config.public_read {
            info!(bucket=%bucket_name, "Configuring bucket policy for public read");

            // Set bucket policy for public read access
            let policy = serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Allow",
                        "Principal": "*",
                        "Action": "s3:GetObject",
                        "Resource": format!("arn:aws:s3:::{}/*", bucket_name)
                    }
                ]
            });

            client
                .put_bucket_policy(bucket_name, &policy.to_string())
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to configure bucket policy for S3 bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(bucket=%bucket_name, "Bucket policy configured successfully");
        } else {
            info!(bucket=%bucket_name, "Skipping bucket policy configuration (public read not enabled)");
        }

        Ok(HandlerAction::Continue {
            state: ConfiguringLifecycle,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringLifecycle,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;

        let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Bucket name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        if !config.lifecycle_rules.is_empty() {
            info!(bucket=%bucket_name, rules_count=%config.lifecycle_rules.len(), "Configuring lifecycle rules");

            // Convert our lifecycle rules to the S3 format
            let mut s3_rules = Vec::new();
            for (i, rule) in config.lifecycle_rules.iter().enumerate() {
                let rule_id = format!("Rule{}", i + 1);

                let s3_rule = LifecycleRule::builder()
                    .id(rule_id)
                    .status(LifecycleRuleStatus::Enabled)
                    .filter(
                        LifecycleRuleFilter::builder()
                            .maybe_prefix(rule.prefix.clone())
                            .build(),
                    )
                    .expiration(
                        LifecycleExpiration::builder()
                            .days(rule.days as i32)
                            .build(),
                    )
                    .build();

                s3_rules.push(s3_rule);
            }

            let lifecycle_config = LifecycleConfiguration::builder().rules(s3_rules).build();

            client
                .put_bucket_lifecycle_configuration(bucket_name, &lifecycle_config)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to configure lifecycle rules for S3 bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(bucket=%bucket_name, "Lifecycle rules configured successfully");
        } else {
            info!(bucket=%bucket_name, "Skipping lifecycle configuration (no rules defined)");
        }

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
        let config = ctx.desired_resource_config::<Storage>()?;

        info!(bucket=%config.id, "Applying resource-scoped permissions for S3 bucket");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        if let Some(bucket_name) = &self.bucket_name {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                bucket_name,
                "storage",
            )
            .await?;
        }

        info!(bucket=%config.id, "Successfully applied resource-scoped permissions");

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
            let aws_cfg = ctx.get_aws_config()?;
            let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;

            // Verify the bucket exists using get_bucket_location.
            // This AWS API call is used because it requires s3:GetBucketLocation permission,
            // which is included in 'heartbeat' level roles, unlike s3:ListBucket
            // required by head_bucket.
            client
                .get_bucket_location(bucket_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check S3 bucket during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            debug!(name = %config.id, bucket = %bucket_name, "S3 bucket exists and is accessible");
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
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;

        info!(name=%config.id, "Starting bucket configuration update");

        // Check if versioning needs to be updated
        if config.versioning != prev_config.versioning {
            let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Bucket name not set in state during versioning update".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(bucket=%bucket_name, current=%config.versioning, previous=%prev_config.versioning, "Updating bucket versioning");

            // Update versioning configuration using our custom S3 client
            let status = if config.versioning {
                VersioningStatus::Enabled
            } else {
                VersioningStatus::Suspended
            };

            client
                .put_bucket_versioning(bucket_name, status)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update versioning for S3 bucket '{}'",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(bucket=%bucket_name, "Bucket versioning updated successfully");
        } else {
            info!(name=%config.id, "Skipping versioning update (no changes needed)");
        }

        Ok(HandlerAction::Continue {
            state: UpdatePublicAccess,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatePublicAccess,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_public_access(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let storage_config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;

        // Check if public access needs to be updated
        if storage_config.public_read != prev_config.public_read {
            let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Bucket name not set in state during public access update".to_string(),
                    resource_id: Some(storage_config.id.clone()),
                })
            })?;

            info!(bucket=%bucket_name, current=%storage_config.public_read, previous=%prev_config.public_read, "Updating public access settings");

            if storage_config.public_read {
                // Enable public access
                let public_access_config = PublicAccessBlockConfiguration::builder()
                    .block_public_acls(false)
                    .block_public_policy(false)
                    .ignore_public_acls(false)
                    .restrict_public_buckets(false)
                    .build();

                client
                    .put_public_access_block(bucket_name, public_access_config)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to enable public access for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(storage_config.id.clone()),
                    })?;

                info!(bucket=%bucket_name, "Public access enabled successfully");
            } else {
                // Disable public access
                let public_access_config = PublicAccessBlockConfiguration::builder()
                    .block_public_acls(true)
                    .block_public_policy(true)
                    .ignore_public_acls(true)
                    .restrict_public_buckets(true)
                    .build();

                client
                    .put_public_access_block(bucket_name, public_access_config)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to disable public access for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(storage_config.id.clone()),
                    })?;

                info!(bucket=%bucket_name, "Public access disabled successfully");
            }
        } else {
            info!(name=%storage_config.id, "Skipping public access update (no changes needed)");
        }

        Ok(HandlerAction::Continue {
            state: UpdatePublicPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatePublicPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_public_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;

        // Check if public policy needs to be updated
        if config.public_read != prev_config.public_read {
            let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Bucket name not set in state during public policy update".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(bucket=%bucket_name, "Updating bucket policy for public read");

            if config.public_read {
                // Set bucket policy for public read access
                let policy = serde_json::json!({
                    "Version": "2012-10-17",
                    "Statement": [
                        {
                            "Effect": "Allow",
                            "Principal": "*",
                            "Action": "s3:GetObject",
                            "Resource": format!("arn:aws:s3:::{}/*", bucket_name)
                        }
                    ]
                });

                client
                    .put_bucket_policy(bucket_name, &policy.to_string())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to update bucket policy for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(bucket=%bucket_name, "Bucket policy set successfully");
            } else {
                // Remove bucket policy - ignore NotFound errors
                match client.delete_bucket_policy(bucket_name).await {
                    Ok(_) => {
                        info!(bucket=%bucket_name, "Bucket policy removed successfully");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(bucket=%bucket_name, "Bucket policy already removed or never existed");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to remove bucket policy for S3 bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            }
        } else {
            info!(name=%config.id, "Skipping bucket policy update (no changes needed)");
        }

        Ok(HandlerAction::Continue {
            state: UpdateLifecycle,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateLifecycle,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_lifecycle(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;

        // Check if lifecycle rules need to be updated
        if config.lifecycle_rules != prev_config.lifecycle_rules {
            let bucket_name = self.bucket_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Bucket name not set in state during lifecycle update".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(bucket=%bucket_name, rules_count=%config.lifecycle_rules.len(), "Updating lifecycle rules");

            if config.lifecycle_rules.is_empty() {
                // Remove lifecycle configuration - ignore NotFound errors
                match client.delete_bucket_lifecycle(bucket_name).await {
                    Ok(_) => {
                        info!(bucket=%bucket_name, "Lifecycle rules removed successfully");
                    }
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(bucket=%bucket_name, "Lifecycle configuration already removed or never existed");
                    }
                    Err(e) => {
                        return Err(e.context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to remove lifecycle configuration for S3 bucket '{}'",
                                bucket_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            } else {
                // Update lifecycle rules - convert our rules to S3 format
                let mut s3_rules = Vec::new();
                for (i, rule) in config.lifecycle_rules.iter().enumerate() {
                    let rule_id = format!("Rule{}", i + 1);

                    let s3_rule = LifecycleRule::builder()
                        .id(rule_id)
                        .status(LifecycleRuleStatus::Enabled)
                        .filter(
                            LifecycleRuleFilter::builder()
                                .maybe_prefix(rule.prefix.clone())
                                .build(),
                        )
                        .expiration(
                            LifecycleExpiration::builder()
                                .days(rule.days as i32)
                                .build(),
                        )
                        .build();

                    s3_rules.push(s3_rule);
                }

                let lifecycle_config = LifecycleConfiguration::builder().rules(s3_rules).build();

                client
                    .put_bucket_lifecycle_configuration(bucket_name, &lifecycle_config)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to update lifecycle configuration for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(bucket=%bucket_name, "Lifecycle rules updated successfully");
            }
        } else {
            info!(name=%config.id, "Skipping lifecycle update (no changes needed)");
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
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Bucket name not set in state during resource permissions update"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(bucket=%bucket_name, "Re-applying resource-scoped permissions after update");
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                bucket_name,
                "storage",
            )
            .await?;
        }

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
                info!(resource_id=%config.id, "No S3 bucket to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.bucket_name = None;

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        // Only get the S3 client if we actually have a bucket to delete
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;

        info!(bucket=%bucket_name, "Starting bucket deletion");

        // Best effort: try to empty the bucket first
        match client.empty_bucket(bucket_name).await {
            Ok(_) => {
                info!(bucket=%bucket_name, "Bucket emptied successfully");
            }
            Err(e) => {
                // Log but continue - bucket might not exist or might already be empty
                info!(bucket=%bucket_name, error=?e, "Could not empty bucket, continuing with deletion attempt");
            }
        }

        // Best effort: try to delete the bucket
        match client.delete_bucket(bucket_name).await {
            Ok(_) => {
                info!(bucket=%bucket_name, "S3 bucket deleted successfully");
            }
            Err(e) => {
                // Check if it's a resource not found error (bucket doesn't exist)
                match &e.error {
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. }) => {
                        warn!(bucket=%bucket_name, "Bucket already deleted or never existed");
                    }
                    _ => {
                        // Log but continue - bucket might already be deleted
                        warn!(bucket=%bucket_name, error=?e, "Could not delete bucket, considering deletion complete");
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
        // Only return outputs when the bucket has been successfully created
        self.bucket_name.as_ref().map(|bucket_name| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: bucket_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, StorageBinding};

        if let Some(bucket_name) = &self.bucket_name {
            let binding = StorageBinding::s3(bucket_name.clone());
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

impl AwsStorageController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AwsStorageState::Ready,
            bucket_name: Some(get_aws_bucket_name("test-stack", storage_name)),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # AWS Storage Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;

    use alien_aws_clients::s3::{
        DeleteObjectsOutput, LifecycleConfiguration, LifecycleExpiration, LifecycleRule,
        LifecycleRuleFilter, LifecycleRuleStatus, ListObjectsV2Output, ListVersionsOutput,
        MockS3Api, PublicAccessBlockConfiguration, VersioningStatus,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        LifecycleRule as AlienLifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs,
    };
    use alien_error::AlienError;
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider,
    };
    use crate::storage::AwsStorageController;
    use crate::AwsStorageState;

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

    fn setup_mock_client_for_creation_and_deletion(bucket_name: &str) -> Arc<MockS3Api> {
        let mut mock_s3 = MockS3Api::new();

        // Mock successful bucket creation
        mock_s3.expect_create_bucket().returning(|_| Ok(()));

        // Mock configuration methods
        mock_s3
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        mock_s3.expect_put_bucket_policy().returning(|_, _| Ok(()));
        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .returning(|_, _| Ok(()));

        // Mock deletion methods
        mock_s3.expect_empty_bucket().returning(|_| Ok(()));
        mock_s3.expect_delete_bucket().returning(|_| Ok(()));

        Arc::new(mock_s3)
    }

    fn setup_mock_client_for_creation_and_update(bucket_name: &str) -> Arc<MockS3Api> {
        let mut mock_s3 = MockS3Api::new();

        // Mock configuration methods for create and update
        mock_s3
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        mock_s3.expect_put_bucket_policy().returning(|_, _| Ok(()));
        mock_s3.expect_delete_bucket_policy().returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_delete_bucket_lifecycle()
            .returning(|_| Ok(()));

        Arc::new(mock_s3)
    }

    fn setup_mock_client_for_best_effort_deletion(_bucket_name: &str) -> Arc<MockS3Api> {
        let mut mock_s3 = MockS3Api::new();

        // Mock empty bucket failure (bucket doesn't exist)
        mock_s3.expect_empty_bucket().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "S3 Bucket".to_string(),
                    resource_name: "test-bucket".to_string(),
                },
            ))
        });

        // Mock successful bucket deletion
        mock_s3.expect_delete_bucket().returning(|_| Ok(()));

        Arc::new(mock_s3)
    }

    fn setup_mock_service_provider(mock_s3: Arc<MockS3Api>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(mock_s3.clone()));

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
        let mock_s3 = setup_mock_client_for_creation_and_deletion(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_s3);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
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
        let mock_s3 = setup_mock_client_for_creation_and_update(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_s3);

        // Start with the "from" storage in Ready state
        let ready_controller = AwsStorageController::mock_ready(&storage_id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_storage)
            .controller(ready_controller)
            .platform(Platform::Aws)
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
        let mock_s3 = setup_mock_client_for_best_effort_deletion(&bucket_name);
        let mock_provider = setup_mock_service_provider(mock_s3);

        // Start with a ready controller
        let ready_controller = AwsStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Aws)
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

        let mut mock_s3 = MockS3Api::new();

        // Mock successful empty bucket
        mock_s3.expect_empty_bucket().returning(|_| Ok(()));

        // Mock bucket deletion failure (bucket doesn't exist)
        mock_s3.expect_delete_bucket().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "S3 Bucket".to_string(),
                    resource_name: "test-bucket".to_string(),
                },
            ))
        });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_s3));

        // Start with a ready controller
        let ready_controller = AwsStorageController::mock_ready(&storage.id);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(ready_controller)
            .platform(Platform::Aws)
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

        let mut mock_s3 = MockS3Api::new();

        // Validate that bucket names are prefixed correctly
        mock_s3
            .expect_create_bucket()
            .withf(|bucket_name| bucket_name == "test-my-awesome-storage")
            .returning(|_| Ok(()));

        // Mock other required methods
        mock_s3
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        mock_s3.expect_put_bucket_policy().returning(|_, _| Ok(()));
        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .returning(|_, _| Ok(()));

        let mock_provider = setup_mock_service_provider(Arc::new(mock_s3));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies lifecycle rules are converted correctly to S3 format
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

        let mut mock_s3 = MockS3Api::new();

        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        mock_s3.expect_put_bucket_policy().returning(|_, _| Ok(()));

        // Validate that the generated lifecycle configuration contains expected rules
        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .withf(|_bucket_name, lifecycle_config| {
                // Should have 2 rules
                if lifecycle_config.rules.len() != 2 {
                    eprintln!(
                        "Expected 2 lifecycle rules, got {}",
                        lifecycle_config.rules.len()
                    );
                    return false;
                }

                // Check first rule (with prefix)
                let rule1 = &lifecycle_config.rules[0];
                if rule1.id.as_ref().unwrap() != "Rule1" {
                    eprintln!("Expected rule ID 'Rule1', got {:?}", rule1.id);
                    return false;
                }
                if rule1.filter.prefix.as_ref().unwrap() != "logs/" {
                    eprintln!("Expected prefix 'logs/', got {:?}", rule1.filter.prefix);
                    return false;
                }
                if rule1.expiration.as_ref().unwrap().days.unwrap() != 30 {
                    eprintln!(
                        "Expected 30 days, got {:?}",
                        rule1.expiration.as_ref().unwrap().days
                    );
                    return false;
                }

                // Check second rule (no prefix)
                let rule2 = &lifecycle_config.rules[1];
                if rule2.id.as_ref().unwrap() != "Rule2" {
                    eprintln!("Expected rule ID 'Rule2', got {:?}", rule2.id);
                    return false;
                }
                if rule2.filter.prefix.is_some() {
                    eprintln!("Expected no prefix, got {:?}", rule2.filter.prefix);
                    return false;
                }
                if rule2.expiration.as_ref().unwrap().days.unwrap() != 365 {
                    eprintln!(
                        "Expected 365 days, got {:?}",
                        rule2.expiration.as_ref().unwrap().days
                    );
                    return false;
                }

                true
            })
            .returning(|_, _| Ok(()));

        let mock_provider = setup_mock_service_provider(Arc::new(mock_s3));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies public read configuration generates correct policy
    #[tokio::test]
    async fn test_public_read_policy_generation() {
        let storage = Storage::new("public-test".to_string())
            .public_read(true)
            .build();

        let mut mock_s3 = MockS3Api::new();

        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));

        // Validate public access block configuration
        mock_s3
            .expect_put_public_access_block()
            .withf(|_bucket_name, config| {
                config.block_public_acls == Some(false)
                    && config.block_public_policy == Some(false)
                    && config.ignore_public_acls == Some(false)
                    && config.restrict_public_buckets == Some(false)
            })
            .returning(|_, _| Ok(()));

        // Validate bucket policy for public read access
        mock_s3
            .expect_put_bucket_policy()
            .withf(|bucket_name, policy| {
                // Parse policy as JSON to validate structure
                let policy_json: serde_json::Value =
                    serde_json::from_str(policy).expect("Policy should be valid JSON");

                // Should have Version and Statement
                if policy_json["Version"] != "2012-10-17" {
                    eprintln!(
                        "Expected version '2012-10-17', got {:?}",
                        policy_json["Version"]
                    );
                    return false;
                }

                let statements = policy_json["Statement"]
                    .as_array()
                    .expect("Statement should be an array");

                if statements.len() != 1 {
                    eprintln!("Expected 1 statement, got {}", statements.len());
                    return false;
                }

                let statement = &statements[0];

                // Check for correct action and resource
                if statement["Action"] != "s3:GetObject" {
                    eprintln!(
                        "Expected action 's3:GetObject', got {:?}",
                        statement["Action"]
                    );
                    return false;
                }

                let expected_resource = format!("arn:aws:s3:::{}/*", bucket_name);
                if statement["Resource"] != expected_resource {
                    eprintln!(
                        "Expected resource '{}', got {:?}",
                        expected_resource, statement["Resource"]
                    );
                    return false;
                }

                true
            })
            .returning(|_, _| Ok(()));

        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .returning(|_, _| Ok(()));

        let mock_provider = setup_mock_service_provider(Arc::new(mock_s3));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies deletion works when bucket_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_bucket_name_succeeds() {
        let storage = basic_storage();

        // Create a controller with no bucket name set (simulating early creation failure)
        let controller = AwsStorageController {
            state: AwsStorageState::CreateFailed,
            bucket_name: None, // This is the key - no bucket name set
            _internal_stay_count: None,
        };

        // Mock provider - no expectations since no API calls should be made
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Start in CreateFailed state
        assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

        // Delete the storage
        executor.delete().unwrap();

        // Run the delete flow - should succeed without making any API calls
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are not available for deleted resources (standard behavior)
        assert!(executor.outputs().is_none());
    }
}

use alien_error::{AlienError, ContextError};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::s3::{
    GetBucketEncryptionOutput, LifecycleConfiguration, LifecycleExpiration, LifecycleRule,
    LifecycleRuleFilter, LifecycleRuleStatus, PublicAccessBlockConfiguration, S3Api,
    VersioningStatus,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsS3StorageHeartbeatData, HeartbeatBackend, HeartbeatCollectionIssue,
    HeartbeatCollectionIssueReason, HeartbeatIssueSeverity, InitialSetupAuthority, ObservedHealth,
    Platform, ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceLifecycle,
    ResourceOutputs, ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus,
    StorageOutputs,
};
use alien_error::{Context, IntoAlienError};
use alien_macros::controller;
use chrono::{DateTime, Utc};

/// How long a bucket waits after `PutBucketAbac` was denied before the controller schedules
/// another attempt. A deployment whose setup predates the ABAC permissions gets them only when
/// its setup is updated, which the runtime cannot observe, so it retries on this interval.
const ABAC_RETRY_INTERVAL_HOURS: i64 = 24;

fn is_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

/// Generates the full, prefixed AWS bucket name.
fn get_aws_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

fn is_bucket_already_owned(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { message, .. })
            if message.contains("BucketAlreadyOwnedByYou")
                || message.contains("you already own")
                || message.contains("already owned by you")
                || message.contains("previous request to create the named bucket succeeded")
    )
}

fn is_missing_optional_bucket_metadata(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

#[controller]
pub struct AwsStorageController {
    /// The actual bucket name (includes stack name prefix).
    /// This is None until the bucket is created.
    pub(crate) bucket_name: Option<String>,
    /// Whether this controller has enabled ABAC on the bucket. State saved before it did reads
    /// as `false`, which schedules one update that enables it.
    #[serde(default)]
    pub(crate) abac_enabled: bool,
    /// When `PutBucketAbac` was last denied. Set while the deployment's setup lacks the ABAC
    /// permissions; the bucket keeps working without ABAC and reports a warning.
    #[serde(default)]
    pub(crate) abac_denied_at: Option<DateTime<Utc>>,
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
        self.bucket_name = Some(bucket_name.clone());

        info!(name=%config.id, bucket=%bucket_name, "Creating S3 bucket");

        // Create the bucket using our custom S3 client
        match client.create_bucket(&bucket_name).await {
            Ok(()) => {}
            Err(error) if is_bucket_already_owned(&error) => {
                info!(
                    bucket = %bucket_name,
                    "S3 bucket already exists and is owned by this account; continuing create flow"
                );
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create S3 bucket '{}'", bucket_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.tag_bucket(
            client.as_ref(),
            &bucket_name,
            &standard_resource_tags(ctx.resource_prefix, &config.id),
            &config.id,
        )
        .await?;
        self.enable_abac(client.as_ref(), &bucket_name, &config.id)
            .await?;

        info!(bucket=%bucket_name, "S3 bucket created successfully");

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
            let location = client.get_bucket_location(bucket_name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to check S3 bucket during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;
            let versioning = client.get_bucket_versioning(bucket_name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to get S3 bucket versioning during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            let lifecycle = match client.get_bucket_lifecycle_configuration(bucket_name).await {
                Ok(configuration) => Some(configuration),
                Err(error) if is_missing_optional_bucket_metadata(&error) => None,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to get S3 bucket lifecycle during heartbeat".to_string(),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            let encryption = match client.get_bucket_encryption(bucket_name).await {
                Ok(configuration) => Some(configuration),
                Err(error) if is_missing_optional_bucket_metadata(&error) => None,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to get S3 bucket encryption during heartbeat".to_string(),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            let public_access_block = match client.get_public_access_block(bucket_name).await {
                Ok(configuration) => Some(configuration),
                Err(error) if is_missing_optional_bucket_metadata(&error) => None,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to get S3 bucket public access block during heartbeat"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            let bucket_policy_present = match client.get_bucket_policy(bucket_name).await {
                Ok(output) => Some(!output.policy.trim().is_empty()),
                Err(error) if is_missing_optional_bucket_metadata(&error) => Some(false),
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to get S3 bucket policy during heartbeat".to_string(),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            let bucket_acl_present = match client.get_bucket_acl(bucket_name).await {
                Ok(output) => {
                    Some(output.owner.is_some() || !output.access_control_list.grants.is_empty())
                }
                Err(error) if is_missing_optional_bucket_metadata(&error) => Some(false),
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to get S3 bucket ACL during heartbeat".to_string(),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            };

            emit_aws_s3_storage_heartbeat(
                ctx,
                &config.id,
                bucket_name,
                location,
                versioning.status,
                lifecycle,
                encryption,
                public_access_block,
                bucket_policy_present,
                bucket_acl_present,
                self.abac_issue(),
            );

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

        if self.abac_pending(ctx, &config.id) {
            let bucket_name = self.bucket_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Bucket name not set in state during ABAC update".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            self.enable_abac(client.as_ref(), &bucket_name, &config.id)
                .await?;
        }

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

    /// Buckets without ABAC get an update that enables it: once for state saved before this
    /// controller enabled ABAC, and again every `ABAC_RETRY_INTERVAL_HOURS` after a denial.
    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let retry_due = self.abac_denied_at.is_none_or(|denied_at| {
            Utc::now() - denied_at >= chrono::Duration::hours(ABAC_RETRY_INTERVAL_HOURS)
        });
        Ok(self.abac_pending(ctx, &config.id) && retry_due)
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        // Only return outputs when the bucket has been successfully created
        self.bucket_name.as_ref().map(|bucket_name| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: bucket_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::StorageBinding;

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

fn emit_aws_s3_storage_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    bucket_name: &str,
    location: alien_aws_clients::s3::GetBucketLocationOutput,
    versioning_status: Option<VersioningStatus>,
    lifecycle: Option<LifecycleConfiguration>,
    encryption: Option<GetBucketEncryptionOutput>,
    public_access_block: Option<PublicAccessBlockConfiguration>,
    bucket_policy_present: Option<bool>,
    bucket_acl_present: Option<bool>,
    abac_issue: Option<HeartbeatCollectionIssue>,
) {
    let region = location.region();
    let versioning_status_label = versioning_status.map(versioning_status_label);
    let versioning_enabled = Some(matches!(
        versioning_status_label.as_deref(),
        Some("Enabled")
    ));
    let lifecycle_rule_count = lifecycle
        .as_ref()
        .map(|configuration| configuration.rules.len() as u64);
    let lifecycle_present = lifecycle_rule_count.map(|count| count > 0).unwrap_or(false);
    let encryption_config_present = encryption.is_some();
    let encryption_enabled = encryption
        .as_ref()
        .map(|configuration| !configuration.rules.is_empty());
    let public_access_block_present = public_access_block.is_some();

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: resource_id.to_string(),
        resource_type: Storage::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::Storage(StorageHeartbeatData::AwsS3(
            AwsS3StorageHeartbeatData {
                status: StorageHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: Some(format!("S3 bucket '{}' metadata is reachable", bucket_name)),
                    stale: false,
                    partial: false,
                    collection_issues: abac_issue.into_iter().collect(),
                },
                name: bucket_name.to_string(),
                region: Some(region.clone()),
                bucket_location: Some(region),
                versioning_status: versioning_status_label,
                versioning_enabled,
                lifecycle_present,
                lifecycle_rule_count,
                encryption_config_present,
                encryption_enabled,
                public_access_block_present,
                block_public_acls: public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.block_public_acls),
                ignore_public_acls: public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.ignore_public_acls),
                block_public_policy: public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.block_public_policy),
                restrict_public_buckets: public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.restrict_public_buckets),
                bucket_policy_present,
                bucket_acl_present,
            },
        )),
        raw: vec![],
    });
}

fn versioning_status_label(status: VersioningStatus) -> String {
    match status {
        VersioningStatus::Enabled => "Enabled".to_string(),
        VersioningStatus::Suspended => "Suspended".to_string(),
    }
}

impl AwsStorageController {
    /// Tags the bucket with `TagResource`, which works with ABAC on or off. A setup that predates
    /// the ABAC permissions grants only `PutBucketTagging`; that call is used only while this
    /// controller has not enabled ABAC, because S3 rejects it on an ABAC bucket.
    async fn tag_bucket(
        &self,
        client: &dyn S3Api,
        bucket_name: &str,
        tags: &HashMap<String, String>,
        resource_id: &str,
    ) -> Result<()> {
        let result = match client.tag_bucket(bucket_name, tags).await {
            Err(error) if is_access_denied(&error) && !self.abac_enabled => {
                warn!(
                    bucket = %bucket_name,
                    "s3:TagResource was denied; tagging with PutBucketTagging until the deployment's setup is updated"
                );
                client.put_bucket_tagging(bucket_name, tags).await
            }
            result => result,
        };
        result.context(ErrorData::CloudPlatformError {
            message: format!("Failed to tag S3 bucket '{}'", bucket_name),
            resource_id: Some(resource_id.to_string()),
        })
    }

    /// Enables ABAC so the `aws:ResourceTag/deployment` conditions in the deployment's grants
    /// are evaluated against this bucket's tags. Access denied means the deployment's setup
    /// predates the ABAC permissions: the bucket keeps working, the denial is recorded and
    /// reported in the heartbeat, and `needs_update` retries later.
    async fn enable_abac(
        &mut self,
        client: &dyn S3Api,
        bucket_name: &str,
        resource_id: &str,
    ) -> Result<()> {
        match client.enable_bucket_abac(bucket_name).await {
            Ok(()) => {
                self.abac_enabled = true;
                self.abac_denied_at = None;
                Ok(())
            }
            Err(error) if is_access_denied(&error) => {
                warn!(
                    bucket = %bucket_name,
                    "s3:PutBucketAbac was denied; the bucket stays without ABAC until the deployment's setup is updated"
                );
                self.abac_denied_at = Some(Utc::now());
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to enable ABAC on S3 bucket '{}'", bucket_name),
                resource_id: Some(resource_id.to_string()),
            })),
        }
    }

    /// The heartbeat warning for a bucket whose ABAC enable was denied.
    fn abac_issue(&self) -> Option<HeartbeatCollectionIssue> {
        (!self.abac_enabled && self.abac_denied_at.is_some()).then(|| HeartbeatCollectionIssue {
            source: "abac".to_string(),
            reason: HeartbeatCollectionIssueReason::Forbidden,
            severity: HeartbeatIssueSeverity::Warning,
            message: "ABAC is not enabled on this bucket: the deployment's setup does not grant \
                      s3:PutBucketAbac. Update the setup to enable it."
                .to_string(),
        })
    }

    /// Whether the bucket still needs ABAC and this controller may change it. A Live bucket is
    /// the runtime's, managed with `storage/provision`. A Frozen bucket belongs to setup: only a
    /// direct setup run, which holds the deployer's credentials, changes it, while templates
    /// enable ABAC themselves.
    fn abac_pending(&self, ctx: &ResourceControllerContext<'_>, resource_id: &str) -> bool {
        if self.bucket_name.is_none() || self.abac_enabled {
            return false;
        }
        match ctx
            .state
            .resources
            .get(resource_id)
            .and_then(|resource| resource.lifecycle)
        {
            Some(ResourceLifecycle::Live) => true,
            Some(ResourceLifecycle::Frozen) => {
                ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup
            }
            None => false,
        }
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AwsStorageState::Ready,
            bucket_name: Some(get_aws_bucket_name("test-stack", storage_name)),
            abac_enabled: true,
            abac_denied_at: None,
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
        AccessControlList, DeleteObjectsOutput, GetBucketAclOutput, GetBucketLocationOutput,
        GetBucketPolicyOutput, GetBucketVersioningOutput, LifecycleConfiguration,
        LifecycleExpiration, LifecycleRule, LifecycleRuleFilter, LifecycleRuleStatus,
        ListObjectsV2Output, ListVersionsOutput, MockS3Api, PublicAccessBlockConfiguration,
        VersioningStatus,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        HeartbeatIssueSeverity, InitialSetupAuthority, LifecycleRule as AlienLifecycleRule,
        Platform, ResourceHeartbeatData, ResourceLifecycle, ResourceStatus, Storage,
        StorageHeartbeatData, StorageOutputs,
    };
    use alien_error::AlienError;
    use chrono::Utc;
    use mockall::Sequence;
    use rstest::{fixture, rstest};
    use std::collections::HashMap;

    use super::ABAC_RETRY_INTERVAL_HOURS;
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
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|_| Ok(()));

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

        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|_| Ok(()));

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

    #[tokio::test]
    async fn ready_storage_emits_observed_heartbeat() {
        let storage = basic_storage();
        let mut mock_s3 = MockS3Api::new();
        mock_s3.expect_get_bucket_location().returning(|_| {
            Ok(GetBucketLocationOutput {
                location_constraint: Some("us-east-1".to_string()),
            })
        });
        mock_s3.expect_get_bucket_versioning().returning(|_| {
            Ok(GetBucketVersioningOutput {
                status: Some(VersioningStatus::Enabled),
                mfa_delete: None,
            })
        });
        mock_s3
            .expect_get_bucket_lifecycle_configuration()
            .returning(|_| Ok(LifecycleConfiguration { rules: vec![] }));
        mock_s3
            .expect_get_bucket_encryption()
            .returning(|_| Ok(alien_aws_clients::s3::GetBucketEncryptionOutput { rules: vec![] }));
        mock_s3
            .expect_get_public_access_block()
            .returning(|_| Ok(PublicAccessBlockConfiguration::default()));
        mock_s3.expect_get_bucket_policy().returning(|_| {
            Ok(GetBucketPolicyOutput {
                policy: String::new(),
            })
        });
        mock_s3.expect_get_bucket_acl().returning(|_| {
            Ok(GetBucketAclOutput {
                owner: None,
                access_control_list: AccessControlList::default(),
            })
        });
        let mock_provider = setup_mock_service_provider(Arc::new(mock_s3));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(AwsStorageController::mock_ready(&storage.id))
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.step().await.unwrap();

        assert_eq!(executor.last_heartbeats().len(), 1);
        assert_eq!(executor.last_heartbeats()[0].resource_id, storage.id);
    }

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
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|_| Ok(()));

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
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|_| Ok(()));
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
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|_| Ok(()));
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
            abac_enabled: false,
            abac_denied_at: None,
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

    // ─────────────── ABAC TESTS ────────────────────────────────

    /// A ready controller as saved before it enabled ABAC: the stored state has no
    /// `abacEnabled` field.
    fn ready_controller_saved_before_abac(storage_id: &str) -> AwsStorageController {
        let mut saved = serde_json::to_value(AwsStorageController::mock_ready(storage_id))
            .expect("controller serializes");
        saved
            .as_object_mut()
            .expect("controller state is an object")
            .remove("abacEnabled")
            .expect("controller state has abacEnabled");
        saved.as_object_mut().unwrap().remove("abacDeniedAt");
        serde_json::from_value(saved).expect("state without abacEnabled deserializes")
    }

    #[tokio::test]
    async fn create_tags_the_bucket_then_enables_abac() {
        let storage = basic_storage();
        let bucket_name = format!("test-{}", storage.id);
        let expected_tags = HashMap::from([
            ("deployment".to_string(), "test".to_string()),
            ("resource".to_string(), storage.id.clone()),
            ("managed-by".to_string(), "runtime".to_string()),
        ]);

        let mut sequence = Sequence::new();
        let mut mock_s3 = MockS3Api::new();
        let created = bucket_name.clone();
        mock_s3
            .expect_create_bucket()
            .withf(move |bucket| bucket == created)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        let tagged = bucket_name.clone();
        mock_s3
            .expect_tag_bucket()
            .withf(move |bucket, tags| bucket == tagged && *tags == expected_tags)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        let abac_bucket = bucket_name.clone();
        mock_s3
            .expect_enable_bucket_abac()
            .withf(move |bucket| bucket == abac_bucket)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
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

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(!executor.needs_update().unwrap());
    }

    fn access_denied(bucket: &str) -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: "Bucket".to_string(),
            resource_name: bucket.to_string(),
        })
    }

    fn expect_ready_heartbeat_reads(mock_s3: &mut MockS3Api) {
        mock_s3.expect_get_bucket_location().returning(|_| {
            Ok(GetBucketLocationOutput {
                location_constraint: Some("us-east-1".to_string()),
            })
        });
        mock_s3.expect_get_bucket_versioning().returning(|_| {
            Ok(GetBucketVersioningOutput {
                status: None,
                mfa_delete: None,
            })
        });
        mock_s3
            .expect_get_bucket_lifecycle_configuration()
            .returning(|_| Ok(LifecycleConfiguration { rules: vec![] }));
        mock_s3
            .expect_get_bucket_encryption()
            .returning(|_| Ok(alien_aws_clients::s3::GetBucketEncryptionOutput { rules: vec![] }));
        mock_s3
            .expect_get_public_access_block()
            .returning(|_| Ok(PublicAccessBlockConfiguration::default()));
        mock_s3.expect_get_bucket_policy().returning(|_| {
            Ok(GetBucketPolicyOutput {
                policy: String::new(),
            })
        });
        mock_s3.expect_get_bucket_acl().returning(|_| {
            Ok(GetBucketAclOutput {
                owner: None,
                access_control_list: AccessControlList::default(),
            })
        });
    }

    fn heartbeat_issue_sources(executor: &SingleControllerExecutor) -> Vec<String> {
        let [heartbeat] = executor.last_heartbeats() else {
            panic!(
                "expected one heartbeat, got {:?}",
                executor.last_heartbeats()
            );
        };
        let ResourceHeartbeatData::Storage(StorageHeartbeatData::AwsS3(data)) = &heartbeat.data
        else {
            panic!("expected an S3 storage heartbeat, got {:?}", heartbeat.data);
        };
        data.status
            .collection_issues
            .iter()
            .map(|issue| {
                assert_eq!(issue.severity, HeartbeatIssueSeverity::Warning);
                issue.source.clone()
            })
            .collect()
    }

    /// A deployment whose setup predates the ABAC permissions: `TagResource` and
    /// `PutBucketAbac` are denied, `PutBucketTagging` is allowed.
    #[tokio::test]
    async fn create_on_a_setup_without_abac_permissions_keeps_the_bucket_working() {
        let storage = basic_storage();
        let mut mock_s3 = MockS3Api::new();
        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_tag_bucket()
            .times(1)
            .returning(|bucket, _| Err(access_denied(bucket)));
        mock_s3
            .expect_put_bucket_tagging()
            .withf(|_, tags| tags.get("deployment").map(String::as_str) == Some("test"))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_s3
            .expect_enable_bucket_abac()
            .times(1)
            .returning(|bucket| Err(access_denied(bucket)));
        mock_s3
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        mock_s3.expect_put_bucket_policy().returning(|_, _| Ok(()));
        mock_s3
            .expect_put_bucket_lifecycle_configuration()
            .returning(|_, _| Ok(()));
        expect_ready_heartbeat_reads(&mut mock_s3);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Reconcile several times: each Ready step reports the warning, and nothing schedules
        // another attempt. The mock's `times(1)` fails the test on a second ABAC or tag call.
        for _ in 0..3 {
            assert!(!executor.needs_update().unwrap());
            executor.step().await.unwrap();
            assert_eq!(executor.status(), ResourceStatus::Running);
            assert_eq!(heartbeat_issue_sources(&executor), vec!["abac".to_string()]);
        }
    }

    #[tokio::test]
    async fn create_never_falls_back_to_put_bucket_tagging_once_abac_is_enabled() {
        let storage = basic_storage();
        let mut controller = AwsStorageController::default();
        controller.abac_enabled = true;

        let mut mock_s3 = MockS3Api::new();
        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_tag_bucket()
            .returning(|bucket, _| Err(access_denied(bucket)));
        mock_s3.expect_put_bucket_tagging().never();

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a denied TagResource on an ABAC bucket must fail");
        assert!(error.message.contains("Failed to tag S3 bucket"));
    }

    #[tokio::test]
    async fn create_fails_when_enabling_abac_fails_for_another_reason() {
        let storage = basic_storage();
        let mut mock_s3 = MockS3Api::new();
        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3.expect_tag_bucket().returning(|_, _| Ok(()));
        mock_s3.expect_enable_bucket_abac().returning(|bucket| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Bucket".to_string(),
                    resource_name: bucket.to_string(),
                },
            ))
        });

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("create must fail when ABAC cannot be enabled");
        assert!(error.message.contains("Failed to enable ABAC"));
        assert_ne!(executor.status(), ResourceStatus::Running);
    }

    #[rstest]
    #[case::retry_interval_elapsed(ABAC_RETRY_INTERVAL_HOURS + 1, true)]
    #[case::within_retry_interval(1, false)]
    #[tokio::test]
    async fn denied_abac_is_retried_by_the_next_update(
        #[case] hours_since_denial: i64,
        #[case] update_scheduled: bool,
    ) {
        let storage = basic_storage();
        let mut controller = AwsStorageController::mock_ready(&storage.id);
        controller.abac_enabled = false;
        controller.abac_denied_at = Some(Utc::now() - chrono::Duration::hours(hours_since_denial));

        let mut mock_s3 = MockS3Api::new();
        mock_s3
            .expect_enable_bucket_abac()
            .times(1)
            .returning(|_| Ok(()));
        expect_ready_heartbeat_reads(&mut mock_s3);

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.needs_update().unwrap(), update_scheduled);

        // Any update of the bucket retries, whether the interval scheduled it or not.
        executor.update(storage).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(!executor.needs_update().unwrap());

        executor.step().await.unwrap();
        assert!(heartbeat_issue_sources(&executor).is_empty());
    }

    #[tokio::test]
    async fn live_bucket_created_before_abac_gets_it_on_the_next_update() {
        let storage = basic_storage();
        let controller = ready_controller_saved_before_abac(&storage.id);
        let bucket_name = controller.bucket_name.clone().unwrap();

        let mut mock_s3 = MockS3Api::new();
        mock_s3
            .expect_enable_bucket_abac()
            .withf(move |bucket| bucket == bucket_name)
            .times(1)
            .returning(|_| Ok(()));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert!(executor.needs_update().unwrap());

        executor.update(storage).unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn frozen_bucket_from_a_template_is_left_to_the_template() {
        let storage = basic_storage();

        let executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(ready_controller_saved_before_abac(&storage.id))
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .service_provider(setup_mock_service_provider(Arc::new(MockS3Api::new())))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert!(!executor.needs_update().unwrap());
    }

    #[tokio::test]
    async fn frozen_bucket_gets_abac_during_a_direct_setup() {
        let storage = basic_storage();
        let mut mock_s3 = MockS3Api::new();
        mock_s3
            .expect_enable_bucket_abac()
            .times(1)
            .returning(|_| Ok(()));

        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(ready_controller_saved_before_abac(&storage.id))
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert!(executor.needs_update().unwrap());

        executor.update(storage).unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(!executor.needs_update().unwrap());
    }
}

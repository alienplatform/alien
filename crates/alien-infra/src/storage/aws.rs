use alien_error::{AlienError, ContextError};
use alien_permissions::aws_resource_policies::aws_prefixed_resource_name;
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::{aws_ownership::is_tagged_as_ours, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_aws_clients::s3::{
    GetBucketEncryptionOutput, LifecycleConfiguration, LifecycleExpiration, LifecycleRule,
    LifecycleRuleFilter, LifecycleRuleStatus, PublicAccessBlockConfiguration, S3Api,
    VersioningStatus,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, AwsS3StorageHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_error::{Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

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

fn is_bucket_missing(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

/// What already holds a bucket name, as far as this deployment is concerned.
enum ExistingBucket {
    Missing,
    /// Carries this deployment's tags for the resource.
    Ours,
    /// Exists with no tags at all.
    Untagged,
    /// Exists with other tags, or in another region.
    Foreign,
}

async fn inspect_bucket(
    client: &dyn S3Api,
    bucket_name: &str,
    our_tags: &HashMap<String, String>,
    resource_id: &str,
) -> Result<ExistingBucket> {
    match client.get_bucket_tagging(bucket_name).await {
        Ok(tags) if is_tagged_as_ours(&tags, our_tags) => Ok(ExistingBucket::Ours),
        Ok(tags) if tags.is_empty() => Ok(ExistingBucket::Untagged),
        Ok(_) => Ok(ExistingBucket::Foreign),
        Err(error) if is_bucket_missing(&error) => Ok(ExistingBucket::Missing),
        // S3 redirects requests for a bucket that exists in another region. This deployment
        // only creates buckets in its own region, so that bucket is not ours, and retrying
        // cannot change that.
        Err(error) if is_redirect(&error) => Ok(ExistingBucket::Foreign),
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to check whether S3 bucket '{}' already exists",
                bucket_name
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

fn bucket_not_owned(resource_id: &str, bucket_name: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
        resource_id: resource_id.to_string(),
        object: format!("S3 bucket '{}'", bucket_name),
    })
}

fn is_redirect(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(
            CloudClientErrorData::RemoteRequestRejected {
                http_status: 301,
                ..
            } | CloudClientErrorData::HttpResponseError {
                http_status: 301,
                ..
            }
        )
    )
}

fn is_access_denied(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::RemoteAccessDenied { .. })
    )
}

fn is_accepted_without_body(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(CloudClientErrorData::HttpResponseBodyReadFailed { .. })
    )
}

/// Whether a failed request may still have been applied: no answer from S3 arrived.
fn is_outcome_unknown(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        &error.error,
        Some(
            CloudClientErrorData::HttpRequestFailed { .. }
                | CloudClientErrorData::Timeout { .. }
                | CloudClientErrorData::RemoteServiceUnavailable { .. }
        )
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
    /// Set only once this controller created the bucket, or found it carrying this
    /// deployment's tags. Deletion acts on this bucket only.
    pub(crate) bucket_name: Option<String>,
    /// A bucket name this controller found free and sent CreateBucket for, before the
    /// bucket was confirmed and tagged. A retry that then finds the bucket untagged knows
    /// it is the one this controller created, whose CreateBucket response was lost.
    #[serde(default)]
    pub(crate) requested_bucket_name: Option<String>,
    /// Whether `bucket_name` was recorded after its tags proved it ours. Earlier versions
    /// recorded the name before CreateBucket, so state they wrote can name a bucket in another
    /// region or another account.
    #[serde(default)]
    pub(crate) ownership_verified: bool,
    /// S3 acknowledged the browser-read CORS operation. Old checkpoints and imports
    /// do not prove that an unchanged declaration was applied.
    #[serde(default)]
    pub(crate) read_cors_applied: bool,
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

        // Ownership is proven again below; until then deletion must not touch the bucket.
        self.bucket_name = None;
        self.ownership_verified = false;
        let bucket_name = aws_prefixed_resource_name(ctx.resource_prefix, &config.id);
        let our_tags = standard_resource_tags(ctx.resource_prefix, &config.id);

        // CreateBucket cannot tell whether a bucket is ours: in us-east-1 it returns 200 for a
        // bucket the account already owns, elsewhere BucketAlreadyOwnedByYou. Check first, and
        // never adopt a bucket this deployment did not create.
        match inspect_bucket(&*client, &bucket_name, &our_tags, &config.id).await? {
            ExistingBucket::Missing => {
                // Saved before CreateBucket is sent, so a retry after a lost response knows
                // the untagged bucket it then finds is the one this controller asked for.
                self.requested_bucket_name = Some(bucket_name);
                Ok(HandlerAction::Continue {
                    state: CreatingBucket,
                    suggested_delay: None,
                })
            }
            ExistingBucket::Untagged
                if self.requested_bucket_name.as_deref() == Some(bucket_name.as_str()) =>
            {
                Ok(HandlerAction::Continue {
                    state: CreatingBucket,
                    suggested_delay: None,
                })
            }
            ExistingBucket::Ours => {
                info!(
                    bucket = %bucket_name,
                    "S3 bucket was already created by this deployment; continuing create flow"
                );
                self.finish_bucket_creation(&*client, bucket_name, &our_tags, &config.id)
                    .await
            }
            ExistingBucket::Untagged | ExistingBucket::Foreign => {
                Err(bucket_not_owned(&config.id, &bucket_name))
            }
        }
    }

    #[handler(
        state = CreatingBucket,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_bucket(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket_name = aws_prefixed_resource_name(ctx.resource_prefix, &config.id);
        let our_tags = standard_resource_tags(ctx.resource_prefix, &config.id);

        // Checked again on every attempt: a retry may follow a CreateBucket whose outcome
        // was never seen.
        match inspect_bucket(&*client, &bucket_name, &our_tags, &config.id).await? {
            ExistingBucket::Missing => {
                // Between this check and CreateBucket, someone else in the same account could
                // create a bucket with this name; in us-east-1 our CreateBucket would then return
                // 200 and we would tag that bucket as ours. The window is one round trip, and the
                // name starts with this deployment's own prefix. Closing it fully would need tags
                // on CreateBucket itself, which requires s3:TagResource.
                self.requested_bucket_name = Some(bucket_name.clone());
                info!(name=%config.id, bucket=%bucket_name, "Creating S3 bucket");
                match client.create_bucket(&bucket_name).await {
                    Ok(()) => {}
                    // The name was free a moment ago, so this is our own request seen twice
                    // (the transport resends a CreateBucket whose response it did not get).
                    Err(error) if is_bucket_already_owned(&error) => {
                        info!(
                            bucket = %bucket_name,
                            "S3 bucket was created by this request's earlier attempt"
                        );
                    }
                    // S3 answered 2xx; only reading the body failed. The bucket was created.
                    Err(error) if is_accepted_without_body(&error) => {
                        info!(
                            bucket = %bucket_name,
                            "S3 accepted CreateBucket but its response body could not be read"
                        );
                    }
                    Err(error) => {
                        if !is_outcome_unknown(&error) {
                            // S3 answered and created nothing, so an untagged bucket found
                            // later under this name is not ours.
                            self.requested_bucket_name = None;
                        }
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to create S3 bucket '{}'", bucket_name),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                }
            }
            ExistingBucket::Ours => {}
            ExistingBucket::Untagged
                if self.requested_bucket_name.as_deref() == Some(bucket_name.as_str()) =>
            {
                info!(
                    bucket = %bucket_name,
                    "S3 bucket this controller requested earlier exists; continuing create flow"
                );
            }
            ExistingBucket::Untagged | ExistingBucket::Foreign => {
                self.requested_bucket_name = None;
                return Err(bucket_not_owned(&config.id, &bucket_name));
            }
        }

        self.finish_bucket_creation(&*client, bucket_name, &our_tags, &config.id)
            .await
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
        self.apply_bucket_policy(ctx).await?;

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
            state: ConfiguringCors,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ConfiguringCors,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_cors(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        if !config.cors_allowed_origins.is_empty() {
            self.apply_read_cors(ctx).await?;
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
        let config = ctx.desired_resource_config::<Storage>()?;
        let prev_config = ctx.previous_resource_config::<Storage>()?;

        if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::ImportedHandoff
            && ctx
                .desired_stack
                .resources
                .get(&config.id)
                .is_some_and(|entry| entry.lifecycle == alien_core::ResourceLifecycle::Frozen)
            && config == prev_config
        {
            // Setup owns this bucket and its IAM grants. Dependency refreshes must
            // preserve the installed bucket rather than rerun its setup flow.
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        if config.cors_allowed_origins != prev_config.cors_allowed_origins {
            self.require_cors_authority(ctx)?;
        }

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;

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
        let config = ctx.desired_resource_config::<Storage>()?;
        let setup_owned = ctx.initial_setup_authority
            != alien_core::InitialSetupAuthority::DirectSetup
            && ctx
                .desired_stack
                .resources
                .get(&config.id)
                .is_some_and(|entry| entry.lifecycle == alien_core::ResourceLifecycle::Frozen);
        // A saved checkpoint can resume here without passing through UpdateStart.
        if !setup_owned || config != ctx.previous_resource_config::<Storage>()? {
            // Reapply even when public_read is unchanged: older buckets may not
            // have the HTTPS requirement yet. Never delete the private policy.
            self.apply_bucket_policy(ctx).await?;
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
            state: UpdatingCors,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingCors,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_cors(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Storage>()?;
        let changed = ctx
            .state
            .resources
            .get(&config.id)
            .and_then(|resource| resource.previous_config.as_ref())
            .is_none()
            || config.cors_allowed_origins
                != ctx
                    .previous_resource_config::<Storage>()?
                    .cors_allowed_origins;
        if changed {
            self.apply_read_cors(ctx).await?;
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
        if ctx.initial_setup_authority == alien_core::InitialSetupAuthority::ImportedHandoff
            && ctx
                .desired_stack
                .resources
                .get(&config.id)
                .is_some_and(|entry| entry.lifecycle == alien_core::ResourceLifecycle::Frozen)
            && config == ctx.previous_resource_config::<Storage>()?
        {
            // Manual retries resume this handler directly, bypassing UpdateStart.
            // Preserve the same setup-owned bucket and grants on that path too.
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

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

        // A bucket is recorded only once creation proved it ours. A requested bucket is one
        // whose creation was interrupted; it is deleted only if it already carries our tags.
        let (bucket_name, recorded) = match (&self.bucket_name, &self.requested_bucket_name) {
            (Some(name), _) => (name.clone(), true),
            (None, Some(name)) => (name.clone(), false),
            (None, None) => {
                info!(resource_id=%config.id, "No S3 bucket to delete");
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        let aws_cfg = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_s3_client(aws_cfg).await?;

        // Prove the bucket is the one this deployment created before emptying it: state
        // written by earlier versions recorded the name before the bucket was created.
        let tags = match client.get_bucket_tagging(&bucket_name).await {
            Ok(tags) => tags,
            Err(error) if is_bucket_missing(&error) => {
                info!(bucket=%bucket_name, "S3 bucket already deleted");
                self.bucket_name = None;
                self.requested_bucket_name = None;
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
            // A bucket in another region was never created by this deployment, whose buckets
            // are all in its own region. Only state from earlier versions can name one.
            Err(error) if is_redirect(&error) => {
                return Ok(self.keep_bucket(
                    &config.id,
                    &bucket_name,
                    "it is in another region, so this deployment did not create it",
                ));
            }
            // State from earlier versions can name another account's bucket (CreateBucket
            // failed with BucketAlreadyExists after the name was recorded). Access to a bucket
            // whose ownership was proven is expected, so there a denial is an error.
            Err(error) if is_access_denied(&error) && !self.ownership_verified => {
                return Ok(self.keep_bucket(
                    &config.id,
                    &bucket_name,
                    "access to it is denied and this deployment never proved it created it",
                ));
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to read the tags of S3 bucket '{}' before deleting it",
                        bucket_name
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };
        if !is_tagged_as_ours(
            &tags,
            &standard_resource_tags(ctx.resource_prefix, &config.id),
        ) {
            // Versions that predate bucket tags created untagged buckets. Such a bucket may be
            // ours, but nothing proves it, so it is kept rather than emptied.
            let reason = if tags.is_empty() && recorded && !self.ownership_verified {
                "it has no tags; it may have been created by a version that predates bucket tags. Delete it by hand if it is no longer needed"
            } else {
                "it is not tagged as created by this deployment"
            };
            return Ok(self.keep_bucket(&config.id, &bucket_name, reason));
        }

        info!(bucket=%bucket_name, "Starting bucket deletion");

        let deleted = match client.empty_bucket(&bucket_name).await {
            Ok(()) => client.delete_bucket(&bucket_name).await,
            Err(error) => Err(error),
        };
        match deleted {
            Ok(()) => info!(bucket=%bucket_name, "S3 bucket deleted successfully"),
            Err(error) if is_bucket_missing(&error) => {
                info!(bucket=%bucket_name, "S3 bucket already deleted");
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete S3 bucket '{}'", bucket_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.bucket_name = None;
        self.requested_bucket_name = None;
        self.ownership_verified = false;

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
                    collection_issues: vec![],
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
    async fn apply_bucket_policy(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<Storage>()?;
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
            && ctx
                .desired_stack
                .resources
                .get(&config.id)
                .is_some_and(|entry| entry.lifecycle == alien_core::ResourceLifecycle::Frozen)
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Storage bucket policy is setup-owned; apply the updated setup package to change this Frozen bucket".into(),
            }));
        }
        let bucket = self.bucket_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Bucket name missing while configuring bucket policy".into(),
            })
        })?;
        let bucket_arn = format!("arn:aws:s3:::{bucket}");
        let mut statements = vec![serde_json::json!({
            "Sid": "DenyInsecureTransport",
            "Effect": "Deny",
            "Principal": "*",
            "Action": "s3:*",
            "Resource": [bucket_arn, format!("{bucket_arn}/*")],
            "Condition": {"Bool": {"aws:SecureTransport": "false"}}
        })];
        if config.public_read {
            statements.push(serde_json::json!({
                "Sid": "AllowPublicRead",
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": format!("{bucket_arn}/*")
            }));
        }
        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": statements
        });
        ctx.service_provider
            .get_aws_s3_client(ctx.get_aws_config()?)
            .await?
            .put_bucket_policy(bucket, &policy.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                resource_id: Some(config.id.clone()),
                message: format!(
                    "Failed to configure HTTPS-only bucket policy for S3 bucket '{bucket}'"
                ),
            })?;
        Ok(())
    }

    /// Finishes deletion without touching a bucket this deployment cannot prove it created.
    fn keep_bucket(
        &mut self,
        resource_id: &str,
        bucket_name: &str,
        reason: &str,
    ) -> AwsStorageHandlerAction {
        warn!(
            resource_id = %resource_id,
            bucket = %bucket_name,
            "Keeping S3 bucket '{}' and its objects: {}",
            bucket_name,
            reason
        );
        self.bucket_name = None;
        self.requested_bucket_name = None;
        self.ownership_verified = false;
        AwsStorageHandlerAction::Continue {
            state: AwsStorageState::Deleted,
            suggested_delay: None,
        }
    }

    /// Tags a bucket this controller created or proved its own, then records it.
    async fn finish_bucket_creation(
        &mut self,
        client: &dyn S3Api,
        bucket_name: String,
        our_tags: &HashMap<String, String>,
        resource_id: &str,
    ) -> Result<AwsStorageHandlerAction> {
        client
            .put_bucket_abac_tags(&bucket_name, our_tags)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to tag S3 bucket '{}'", bucket_name),
                resource_id: Some(resource_id.to_string()),
            })?;

        info!(bucket=%bucket_name, "S3 bucket created successfully");
        self.bucket_name = Some(bucket_name);
        self.requested_bucket_name = None;
        self.ownership_verified = true;

        Ok(AwsStorageHandlerAction::Continue {
            state: AwsStorageState::ConfiguringVersioning,
            suggested_delay: None,
        })
    }
}

impl AwsStorageController {
    fn require_cors_authority(&self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let config = ctx.desired_resource_config::<Storage>()?;
        if ctx.initial_setup_authority != alien_core::InitialSetupAuthority::DirectSetup
            && ctx
                .desired_stack
                .resources
                .get(&config.id)
                .is_some_and(|entry| entry.lifecycle == alien_core::ResourceLifecycle::Frozen)
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Storage CORS is setup-owned; apply the updated setup package to change this Frozen bucket".into(),
            }));
        }
        Ok(())
    }

    async fn apply_read_cors(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        self.require_cors_authority(ctx)?;
        let config = ctx.desired_resource_config::<Storage>()?;
        let bucket = self.bucket_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Bucket name missing while configuring CORS".into(),
            })
        })?;
        ctx.service_provider.get_aws_s3_client(ctx.get_aws_config()?).await?
            .set_bucket_read_cors(bucket, &config.cors_allowed_origins).await
            .context(ErrorData::CloudPlatformError {
                resource_id: Some(config.id.clone()), message: format!("Failed to configure browser-read CORS for bucket '{bucket}'; a setup update is needed to grant s3:PutBucketCORS before changing CORS"),
            })?;
        self.read_cors_applied = true;
        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(storage_name: &str) -> Self {
        Self {
            state: AwsStorageState::Ready,
            bucket_name: Some(aws_prefixed_resource_name("test-stack", storage_name)),
            requested_bucket_name: None,
            ownership_verified: true,
            read_cors_applied: false,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # AWS Storage Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use alien_aws_clients::{
        s3::{
            AccessControlList, DeleteObjectsOutput, GetBucketAclOutput, GetBucketLocationOutput,
            GetBucketPolicyOutput, GetBucketVersioningOutput, LifecycleConfiguration,
            LifecycleExpiration, LifecycleRule, LifecycleRuleFilter, LifecycleRuleStatus,
            ListObjectsV2Output, ListVersionsOutput, MockS3Api, PublicAccessBlockConfiguration,
            S3Api, S3Client, VersioningStatus,
        },
        AwsClientConfigExt, AwsCredentialProvider,
    };
    use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
    use alien_core::{
        standard_resource_tags, AwsClientConfig, AwsCredentials, AwsServiceOverrides, ClientConfig,
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority,
        LifecycleRule as AlienLifecycleRule, Platform, Resource, ResourceLifecycle, ResourceStatus,
        Stack, StackResourceState, StackSettings, StackState, Storage, StorageOutputs,
    };
    use alien_error::AlienError;
    use httpmock::{Method::PUT, MockServer};
    use rstest::{fixture, rstest};

    use crate::core::{
        controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
        MockPlatformServiceProvider, PlatformServiceProvider, StackExecutor, StackResourceStateExt,
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

    /// One S3 bucket as the account sees it: `None` while no bucket has the name.
    #[derive(Debug, Clone, PartialEq)]
    struct FakeBucket {
        tags: HashMap<String, String>,
        objects: usize,
    }

    type FakeAccount = Arc<Mutex<Option<FakeBucket>>>;

    fn missing_bucket() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "Bucket".to_string(),
            resource_name: "test-bucket".to_string(),
        })
    }

    fn lost_response() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::HttpRequestFailed {
            message: "connection reset before the response arrived".to_string(),
        })
    }

    /// A fault the fake injects once.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Fault {
        None,
        /// CreateBucket creates the bucket but its response is lost.
        LoseCreateResponse,
        /// PutBucketTagging tags the bucket but its response is lost.
        LoseTaggingResponse,
        /// S3 refuses CreateBucket and creates nothing.
        RejectCreate,
        /// CreateBucket creates the bucket and answers 2xx, but its body cannot be read.
        UnreadableCreateResponse,
    }

    /// An S3 mock that keeps one bucket's existence, tags and object count, with us-east-1
    /// semantics: CreateBucket on a bucket the account already owns returns 200.
    fn fake_s3(account: FakeAccount, fault: Fault) -> MockS3Api {
        let mut mock_s3 = MockS3Api::new();

        let state = account.clone();
        mock_s3.expect_get_bucket_tagging().returning(move |_| {
            match state.lock().unwrap().as_ref() {
                Some(bucket) => Ok(bucket.tags.clone()),
                None => Err(missing_bucket()),
            }
        });
        let state = account.clone();
        let lose_create = Arc::new(Mutex::new(fault == Fault::LoseCreateResponse));
        let reject_create = Arc::new(Mutex::new(fault == Fault::RejectCreate));
        let unreadable_create = Arc::new(Mutex::new(fault == Fault::UnreadableCreateResponse));
        mock_s3.expect_create_bucket().returning(move |_| {
            if std::mem::take(&mut *reject_create.lock().unwrap()) {
                return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "Bucket".to_string(),
                    resource_name: "test-bucket".to_string(),
                }));
            }
            let mut bucket = state.lock().unwrap();
            if bucket.is_none() {
                *bucket = Some(FakeBucket {
                    tags: HashMap::new(),
                    objects: 0,
                });
            }
            if std::mem::take(&mut *lose_create.lock().unwrap()) {
                return Err(lost_response());
            }
            if std::mem::take(&mut *unreadable_create.lock().unwrap()) {
                return Err(AlienError::new(
                    CloudClientErrorData::HttpResponseBodyReadFailed { http_status: 200 },
                ));
            }
            Ok(())
        });
        let state = account.clone();
        let lose_tagging = Arc::new(Mutex::new(fault == Fault::LoseTaggingResponse));
        mock_s3
            .expect_put_bucket_abac_tags()
            .returning(move |_, tags| {
                let mut bucket = state.lock().unwrap();
                let bucket = bucket.as_mut().ok_or_else(missing_bucket)?;
                bucket.tags = tags.clone();
                if std::mem::take(&mut *lose_tagging.lock().unwrap()) {
                    return Err(lost_response());
                }
                Ok(())
            });
        let state = account.clone();
        mock_s3.expect_empty_bucket().returning(move |_| {
            let mut bucket = state.lock().unwrap();
            bucket.as_mut().ok_or_else(missing_bucket)?.objects = 0;
            Ok(())
        });
        let state = account.clone();
        mock_s3.expect_delete_bucket().returning(move |_| {
            state.lock().unwrap().take().ok_or_else(missing_bucket)?;
            Ok(())
        });

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

        mock_s3
    }

    fn account_with(bucket: Option<FakeBucket>) -> FakeAccount {
        Arc::new(Mutex::new(bucket))
    }

    /// A bucket someone made by hand in the same account, holding data.
    fn handmade_bucket() -> FakeBucket {
        FakeBucket {
            tags: HashMap::from([("owner".to_string(), "handmade".to_string())]),
            objects: 1,
        }
    }

    /// The tags the controller writes for `storage_id` under the test executor's prefix.
    fn our_tags(storage_id: &str) -> HashMap<String, String> {
        standard_resource_tags("test", storage_id)
    }

    fn setup_mock_client_for_creation_and_update(bucket_name: &str) -> Arc<MockS3Api> {
        let mut mock_s3 = MockS3Api::new();

        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_abac_tags()
            .returning(|_, _| Ok(()));

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

    fn setup_mock_client_for_missing_bucket() -> Arc<MockS3Api> {
        let mut mock_s3 = MockS3Api::new();
        mock_s3
            .expect_get_bucket_tagging()
            .returning(|_| Err(missing_bucket()));
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
        let storage_id = storage.id.clone();
        let account = account_with(None);
        let mock_provider =
            setup_mock_service_provider(Arc::new(fake_s3(account.clone(), Fault::None)));

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
        assert_eq!(storage_outputs.bucket_name, format!("test-{storage_id}"));
        assert_eq!(
            account.lock().unwrap().as_ref().map(|bucket| &bucket.tags),
            Some(&our_tags(&storage_id))
        );

        // Delete the storage
        executor.delete().unwrap();

        // Run delete flow: the bucket this deployment created is deleted.
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), None);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── OWNERSHIP TESTS ──────────────────────────────────

    /// A same-account bucket that already has the resource's name was made by someone else.
    /// Creating must refuse it without touching its tags or data, and removing the resource
    /// afterwards must not delete it.
    #[rstest]
    #[case::other_tags(handmade_bucket())]
    #[case::no_tags(FakeBucket { tags: HashMap::new(), objects: 1 })]
    #[tokio::test]
    async fn existing_bucket_without_our_tags_is_never_adopted_or_deleted(
        #[case] existing: FakeBucket,
    ) {
        let storage = Storage::new("data".to_string()).build();
        let account = account_with(Some(existing.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(fake_s3(
                account.clone(),
                Fault::None,
            ))))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("a bucket this deployment did not create is refused");
        assert!(
            matches!(
                &error.error,
                Some(crate::error::ErrorData::ExistingCloudResourceNotOwned { resource_id, object })
                    if resource_id == "data" && object == "S3 bucket 'test-data'"
            ),
            "{error:?}"
        );
        assert!(!error.retryable, "retrying cannot make the bucket ours");
        assert_eq!(*account.lock().unwrap(), Some(existing.clone()));
        let controller = executor.internal_state::<AwsStorageController>().unwrap();
        assert_eq!(controller.bucket_name, None);
        assert!(executor.outputs().is_none());

        // The release that drops the resource deletes nothing.
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), Some(existing));
    }

    /// State written by earlier versions can name a bucket this deployment never created.
    /// Deletion checks the tags first and leaves such a bucket and its objects alone.
    #[tokio::test]
    async fn delete_skips_a_recorded_bucket_without_our_tags() {
        let storage = basic_storage();
        let account = account_with(Some(handmade_bucket()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(AwsStorageController::mock_ready(&storage.id))
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(fake_s3(
                account.clone(),
                Fault::None,
            ))))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        let bucket = account.lock().unwrap().clone().expect("the bucket is kept");
        assert_eq!(bucket.objects, 1, "its objects are kept");
        assert_eq!(bucket, handmade_bucket());
        assert!(executor.outputs().is_none());
    }

    /// A step whose S3 call succeeded but whose response was lost is retried. The retry finds
    /// the bucket this controller just made (untagged, or already tagged) and carries on.
    #[rstest]
    #[case::create_response_lost(Fault::LoseCreateResponse)]
    #[case::tagging_response_lost(Fault::LoseTaggingResponse)]
    #[tokio::test]
    async fn retry_after_a_lost_response_resumes_our_bucket(#[case] fault: Fault) {
        let storage = Storage::new("data".to_string()).build();
        let account = account_with(None);
        let mut executor = data_executor(storage, &account, fault).await;

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the first attempt loses its response");
        assert!(error.retryable, "{error:?}");
        assert!(account.lock().unwrap().is_some(), "S3 created the bucket");

        // The executor retries the same step with the controller state the failure left.
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            account
                .lock()
                .unwrap()
                .as_ref()
                .map(|bucket| bucket.tags.clone()),
            Some(our_tags("data"))
        );
        let outputs = executor.outputs().unwrap();
        assert_eq!(
            outputs
                .downcast_ref::<StorageOutputs>()
                .unwrap()
                .bucket_name,
            "test-data"
        );
        let controller = executor.internal_state::<AwsStorageController>().unwrap();
        assert_eq!(controller.requested_bucket_name, None);
    }

    /// A 2xx CreateBucket whose body cannot be read still created the bucket.
    #[tokio::test]
    async fn create_accepted_with_an_unreadable_response_continues() {
        let storage = Storage::new("data".to_string()).build();
        let account = account_with(None);
        let mut executor = data_executor(storage, &account, Fault::UnreadableCreateResponse).await;

        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            account
                .lock()
                .unwrap()
                .as_ref()
                .map(|bucket| bucket.tags.clone()),
            Some(our_tags("data"))
        );
    }

    /// When S3 refuses CreateBucket, nothing was created, so the request no longer vouches for
    /// an untagged bucket. One that appears under the name before the retry is refused.
    #[tokio::test]
    async fn retry_after_a_refused_create_does_not_adopt_a_new_untagged_bucket() {
        let storage = Storage::new("data".to_string()).build();
        let account = account_with(None);
        let mut executor = data_executor(storage, &account, Fault::RejectCreate).await;

        executor
            .run_until_terminal()
            .await
            .expect_err("S3 refuses the create");
        assert_eq!(*account.lock().unwrap(), None);
        let someone_elses = FakeBucket {
            tags: HashMap::new(),
            objects: 1,
        };
        *account.lock().unwrap() = Some(someone_elses.clone());

        let error = executor
            .run_until_terminal()
            .await
            .expect_err("the untagged bucket is not ours");
        assert!(matches!(
            error.error,
            Some(crate::error::ErrorData::ExistingCloudResourceNotOwned { .. })
        ));
        assert_eq!(*account.lock().unwrap(), Some(someone_elses));
    }

    /// Destroy can follow a create interrupted after the bucket was tagged but before the
    /// controller recorded it. The bucket carries our tags, so it is deleted, not leaked.
    #[tokio::test]
    async fn delete_after_an_interrupted_create_removes_our_tagged_bucket() {
        let storage = Storage::new("data".to_string()).build();
        let account = account_with(None);
        let mut executor = data_executor(storage, &account, Fault::LoseTaggingResponse).await;

        executor
            .run_until_terminal()
            .await
            .expect_err("the tagging response is lost");
        assert_eq!(
            account
                .lock()
                .unwrap()
                .as_ref()
                .map(|bucket| bucket.tags.clone()),
            Some(our_tags("data"))
        );

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), None);
    }

    /// S3 redirects requests for a bucket that exists in another region. That bucket is not
    /// this deployment's, and retrying the create cannot change it.
    #[tokio::test]
    async fn bucket_in_another_region_is_refused_without_retries() {
        let storage = Storage::new("data".to_string()).build();
        let mut mock_s3 = MockS3Api::new();
        mock_s3.expect_get_bucket_tagging().times(1).returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteRequestRejected {
                    service: "S3".to_string(),
                    http_status: 301,
                    provider_code: "PermanentRedirect".to_string(),
                    message: "The bucket you are attempting to access must be addressed using the specified endpoint.".to_string(),
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
            .expect_err("the bucket elsewhere is refused");
        assert!(matches!(
            error.error,
            Some(crate::error::ErrorData::ExistingCloudResourceNotOwned { .. })
        ));
        assert!(!error.retryable);
    }

    /// State written by versions that recorded the bucket name before CreateBucket.
    fn legacy_recorded(storage_id: &str) -> AwsStorageController {
        AwsStorageController {
            state: AwsStorageState::Ready,
            bucket_name: Some(format!("test-{storage_id}")),
            requested_bucket_name: None,
            ownership_verified: false,
            read_cors_applied: false,
            _internal_stay_count: None,
        }
    }

    async fn delete_with(
        controller: AwsStorageController,
        mock_s3: MockS3Api,
    ) -> (
        SingleControllerExecutor,
        alien_error::Result<(), crate::error::ErrorData>,
    ) {
        let mut executor = SingleControllerExecutor::builder()
            .resource(Storage::new("data".to_string()).build())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(mock_s3)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        executor.delete().unwrap();
        let result = executor.run_until_terminal().await;
        (executor, result)
    }

    fn tagging_fails_with(error: CloudClientErrorData) -> MockS3Api {
        // No empty or delete expectations: any attempt to remove the bucket fails the test.
        let mut mock_s3 = MockS3Api::new();
        mock_s3
            .expect_get_bucket_tagging()
            .returning(move |_| Err(AlienError::new(error.clone())));
        mock_s3
    }

    /// Earlier versions could record a bucket in another region (CreateBucket answered
    /// BucketAlreadyOwnedByYou there). Destroy keeps it instead of failing forever.
    #[tokio::test]
    async fn delete_keeps_a_recorded_bucket_in_another_region() {
        let (executor, result) = delete_with(
            legacy_recorded("data"),
            tagging_fails_with(CloudClientErrorData::RemoteRequestRejected {
                service: "S3".to_string(),
                http_status: 301,
                provider_code: "PermanentRedirect".to_string(),
                message: "use the specified endpoint".to_string(),
            }),
        )
        .await;

        result.expect("a bucket in another region is kept, not an error");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Earlier versions could record another account's bucket (CreateBucket failed with
    /// BucketAlreadyExists). Access is denied; destroy keeps it. Once ownership was proven,
    /// the same denial is a real failure.
    #[rstest]
    #[case::recorded_by_an_earlier_version(false, true)]
    #[case::ownership_proven(true, false)]
    #[tokio::test]
    async fn access_denied_on_delete_depends_on_proven_ownership(
        #[case] ownership_verified: bool,
        #[case] kept: bool,
    ) {
        let mut controller = legacy_recorded("data");
        controller.ownership_verified = ownership_verified;
        let (executor, result) = delete_with(
            controller,
            tagging_fails_with(CloudClientErrorData::RemoteAccessDenied {
                resource_type: "Bucket".to_string(),
                resource_name: "test-data".to_string(),
            }),
        )
        .await;

        if kept {
            result.expect("another account's bucket is kept");
            assert_eq!(executor.status(), ResourceStatus::Deleted);
        } else {
            let error = result.expect_err("a denial on our own bucket fails the delete");
            assert!(error.retryable, "{error:?}");
            assert_eq!(executor.status(), ResourceStatus::Deleting);
        }
    }

    /// Versions before the `runtime` tag value wrote `managed-by=deployment`. Their buckets are
    /// still recognized and deleted.
    #[tokio::test]
    async fn delete_removes_a_bucket_tagged_by_an_earlier_version() {
        let mut tags = our_tags("data");
        tags.insert("managed-by".to_string(), "deployment".to_string());
        let account = account_with(Some(FakeBucket { tags, objects: 2 }));
        let (executor, result) = delete_with(
            legacy_recorded("data"),
            fake_s3(account.clone(), Fault::None),
        )
        .await;

        result.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), None);
    }

    /// Versions that predate bucket tags left untagged buckets. Nothing proves such a bucket is
    /// ours, so destroy keeps it with its objects rather than emptying it.
    #[tokio::test]
    async fn delete_keeps_an_untagged_bucket_recorded_by_an_earlier_version() {
        let untagged = FakeBucket {
            tags: HashMap::new(),
            objects: 2,
        };
        let account = account_with(Some(untagged.clone()));
        let (executor, result) = delete_with(
            legacy_recorded("data"),
            fake_s3(account.clone(), Fault::None),
        )
        .await;

        result.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(*account.lock().unwrap(), Some(untagged));
    }

    async fn data_executor(
        storage: Storage,
        account: &FakeAccount,
        fault: Fault,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(fake_s3(
                account.clone(),
                fault,
            ))))
            .with_test_dependencies()
            .build()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn unchanged_legacy_cors_does_not_plan_cloud_writes() {
        let desired = Storage::new("cors".into())
            .cors_allowed_origins(vec!["https://app.example".into()])
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        for lifecycle in [ResourceLifecycle::Live, ResourceLifecycle::Frozen] {
            let stack = Stack::new("test".into())
                .add(desired.clone(), lifecycle)
                .build();
            let executor = StackExecutor::builder(
                &stack,
                ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            )
            .deployment_config(&config)
            .service_provider(setup_mock_service_provider(Arc::new(MockS3Api::new())))
            .build()
            .unwrap();
            // Old roles lack PutBucketCORS and old checkpoints lack its acknowledgement.
            let mut old = serde_json::to_value(legacy_recorded("cors")).unwrap();
            old.as_object_mut().unwrap().remove("readCorsApplied");
            let controller: AwsStorageController = serde_json::from_value(old).unwrap();
            assert!(!controller.read_cors_applied);
            let mut state = StackState::with_resource_prefix(Platform::Aws, "test".into());
            let mut resource = StackResourceState::new_pending(
                Storage::RESOURCE_TYPE.into(),
                Resource::new(desired.clone()),
                Some(lifecycle),
                vec![],
            );
            resource.status = ResourceStatus::Running;
            resource
                .set_internal_controller(Some(Box::new(controller)))
                .unwrap();
            state.resources.insert("cors".into(), resource);
            assert!(!executor.plan(&state).unwrap().updates.contains_key("cors"));
            assert_eq!(state.resources["cors"].status, ResourceStatus::Running);
        }
    }

    #[tokio::test]
    async fn cors_create_replace_clear_and_delete_preserve_bucket_identity() {
        let account = Arc::new(Mutex::new(None));
        let cors = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut client = fake_s3(account.clone(), Fault::None);
        let observed = cors.clone();
        client
            .expect_set_bucket_read_cors()
            .times(3)
            .returning(move |bucket, origins| {
                assert_eq!(bucket, "test-cors");
                *observed.lock().unwrap() = origins.to_vec();
                Ok(())
            });
        let mut storage = Storage::new("cors".into())
            .cors_allowed_origins(vec!["https://one.example".into()])
            .build();
        let mut run = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert_eq!(*cors.lock().unwrap(), storage.cors_allowed_origins);
        storage.cors_allowed_origins = vec!["https://two.example".into()];
        run.update(storage.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(*cors.lock().unwrap(), storage.cors_allowed_origins);
        assert!(account.lock().unwrap().is_some());
        run.update(storage.clone()).unwrap();
        run.run_until_terminal().await.unwrap();
        storage.cors_allowed_origins.clear();
        run.update(storage).unwrap();
        run.run_until_terminal().await.unwrap();
        assert!(cors.lock().unwrap().is_empty());
        assert_eq!(run.status(), ResourceStatus::Running);
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
        assert!(account.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn cors_saved_handlers_refuse_frozen_mutations_before_cloud_access() {
        for state in [
            AwsStorageState::ConfiguringCors,
            AwsStorageState::UpdatingCors,
        ] {
            let previous = Storage::new("cors".into()).build();
            let storage = Storage::new("cors".into())
                .cors_allowed_origins(vec!["https://one.example".into()])
                .build();
            let mut controller = AwsStorageController::mock_ready("cors");
            controller.state = state;
            let controller: AwsStorageController =
                serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
            let mut run = SingleControllerExecutor::builder()
                .resource(storage)
                .previous_resource(previous)
                .controller(controller)
                .platform(Platform::Aws)
                .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
                .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            let error = run.step().await.unwrap_err();
            assert!(error.to_string().contains("setup-owned"));
        }
    }

    #[tokio::test]
    async fn unchanged_cors_checkpoint_preserves_old_roles() {
        let storage = Storage::new("cors".into())
            .cors_allowed_origins(vec!["https://app.example".into()])
            .build();
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let mut controller = legacy_recorded("cors");
            controller.state = AwsStorageState::UpdatingCors;
            let mut run = SingleControllerExecutor::builder()
                .resource(storage.clone())
                .previous_resource(storage.clone())
                .controller(controller)
                .platform(Platform::Aws)
                .resource_lifecycle(lifecycle)
                .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
                .service_provider(setup_mock_service_provider(Arc::new(MockS3Api::new())))
                .with_test_dependencies()
                .build()
                .await
                .unwrap();
            run.run_until_terminal().await.unwrap();
            assert_eq!(run.status(), ResourceStatus::Running);
            let controller = run.internal_state::<AwsStorageController>().unwrap();
            assert!(!controller.read_cors_applied);
            assert_eq!(controller.bucket_name.as_deref(), Some("test-cors"));
        }
    }

    #[tokio::test]
    async fn changed_cors_denied_requires_setup_update() {
        let previous = Storage::new("cors".into()).build();
        let desired = Storage::new("cors".into())
            .cors_allowed_origins(vec!["https://app.example".into()])
            .build();
        let mut client = MockS3Api::new();
        client
            .expect_set_bucket_read_cors()
            .times(1)
            .returning(|bucket, _| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "Bucket".into(),
                    resource_name: bucket.into(),
                }))
            });
        let mut controller = legacy_recorded("cors");
        controller.state = AwsStorageState::UpdatingCors;
        let mut run = SingleControllerExecutor::builder()
            .resource(desired)
            .previous_resource(previous)
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Live)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        let error = run.step().await.unwrap_err();
        assert!(
            error.to_string().contains("setup update is needed"),
            "{error}"
        );
        assert!(error.to_string().contains("s3:PutBucketCORS"), "{error}");
        let controller = run.internal_state::<AwsStorageController>().unwrap();
        assert!(!controller.read_cors_applied);
        assert_eq!(controller.bucket_name.as_deref(), Some("test-cors"));
    }

    #[tokio::test]
    async fn cors_checkpoint_retry_after_lost_response_replaces_same_policy() {
        let account = Arc::new(Mutex::new(Some(FakeBucket {
            tags: standard_resource_tags("test", "cors"),
            objects: 1,
        })));
        let cors = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut client = fake_s3(account.clone(), Fault::None);
        let observed = cors.clone();
        let first = Arc::new(Mutex::new(true));
        client
            .expect_set_bucket_read_cors()
            .times(2)
            .returning(move |bucket, origins| {
                assert_eq!(bucket, "test-cors");
                *observed.lock().unwrap() = origins.to_vec();
                if std::mem::take(&mut *first.lock().unwrap()) {
                    return Err(lost_response());
                }
                Ok(())
            });
        let storage = Storage::new("cors".into())
            .cors_allowed_origins(vec!["https://one.example".into()])
            .build();
        let mut controller = AwsStorageController::mock_ready("cors");
        controller.bucket_name = Some("test-cors".into());
        controller.ownership_verified = true;
        controller.state = AwsStorageState::UpdatingCors;
        let controller: AwsStorageController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        let mut run = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .previous_resource(Storage::new("cors".into()).build())
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        assert!(run.step().await.is_err());
        assert_eq!(*cors.lock().unwrap(), storage.cors_allowed_origins);
        run.step().await.unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert_eq!(account.lock().unwrap().as_ref().unwrap().objects, 1);
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert!(account.lock().unwrap().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[tokio::test]
    async fn unchanged_imported_bucket_refresh_preserves_setup_permissions() {
        let storage = Storage::new("objects".to_string()).build();
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
            .controller(AwsStorageController::mock_ready(&storage.id))
            .platform(Platform::Aws)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        let original_bucket = executor
            .outputs()
            .unwrap()
            .downcast_ref::<StorageOutputs>()
            .unwrap()
            .bucket_name
            .clone();
        executor.update(storage).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            executor
                .outputs()
                .unwrap()
                .downcast_ref::<StorageOutputs>()
                .unwrap()
                .bucket_name,
            original_bucket
        );
    }

    #[tokio::test]
    async fn unchanged_imported_bucket_permission_retry_preserves_setup_permissions() {
        let storage = Storage::new("objects".to_string()).build();
        let mut controller = AwsStorageController::mock_ready(&storage.id);
        controller.state = AwsStorageState::UpdatingResourcePermissions;
        let failed = alien_core::StackResourceState::builder()
            .resource_type("storage".to_string())
            .status(ResourceStatus::UpdateFailed)
            .config(alien_core::Resource::new(storage.clone()))
            .previous_config(alien_core::Resource::new(storage.clone()))
            .last_failed_state(crate::core::serialize_controller(&controller).unwrap())
            .lifecycle(alien_core::ResourceLifecycle::Frozen)
            .build();
        let mut failed: alien_core::StackResourceState =
            serde_json::from_value(serde_json::to_value(failed).unwrap()).unwrap();
        assert!(failed.retry_failed().unwrap());
        assert_eq!(failed.status, ResourceStatus::Updating);
        assert!(failed.last_failed_state.is_none());
        let controller: AwsStorageController =
            serde_json::from_value(failed.internal_state.unwrap()).unwrap();
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .previous_resource(storage.clone())
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
            .controller(controller)
            .platform(Platform::Aws)
            // No provider calls are permitted for this setup-owned bucket.
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        assert_eq!(executor.status(), ResourceStatus::Updating);
        let original_bucket = executor
            .outputs()
            .unwrap()
            .downcast_ref::<StorageOutputs>()
            .unwrap()
            .bucket_name
            .clone();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(
            executor
                .internal_state::<AwsStorageController>()
                .unwrap()
                .state,
            AwsStorageState::Ready
        );
        assert_eq!(
            executor
                .outputs()
                .unwrap()
                .downcast_ref::<StorageOutputs>()
                .unwrap()
                .bucket_name,
            original_bucket
        );
    }

    #[tokio::test]
    async fn changed_imported_bucket_permission_retry_still_requires_setup() {
        let previous = Storage::new("objects".to_string()).build();
        let mut desired = previous.clone();
        desired.versioning = true;
        let mut controller = AwsStorageController::mock_ready(&desired.id);
        controller.state = AwsStorageState::UpdatingResourcePermissions;
        let mut executor = SingleControllerExecutor::builder()
            .resource(desired)
            .previous_resource(previous)
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .initial_setup_authority(alien_core::InitialSetupAuthority::ImportedHandoff)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        let error = executor.run_until_terminal().await.unwrap_err();
        assert!(matches!(
            error.error,
            Some(crate::error::ErrorData::ImportedSetupStateInvalid { .. })
        ));
        assert_eq!(executor.status(), ResourceStatus::Updating);
    }

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
        let mock_provider = setup_mock_service_provider(setup_mock_client_for_missing_bucket());

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

        // Run the delete flow - a bucket that is already gone needs no deletion
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

        let tags = our_tags(&storage.id);
        mock_s3
            .expect_get_bucket_tagging()
            .returning(move |_| Ok(tags.clone()));

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

        // Run the delete flow - a bucket deleted concurrently counts as deleted
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

        mock_s3
            .expect_get_bucket_tagging()
            .returning(|_| Err(missing_bucket()));
        // Validate that bucket names are prefixed correctly
        mock_s3
            .expect_create_bucket()
            .withf(|bucket_name| bucket_name == "test-my-awesome-storage")
            .returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_abac_tags()
            .returning(|_, _| Ok(()));

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

        mock_s3
            .expect_get_bucket_tagging()
            .returning(|_| Err(missing_bucket()));
        mock_s3.expect_create_bucket().returning(|_| Ok(()));
        mock_s3
            .expect_put_bucket_abac_tags()
            .returning(|_, _| Ok(()));
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

    #[rstest]
    #[case::private(false)]
    #[case::public(true)]
    #[tokio::test]
    async fn create_requires_https(#[case] public_read: bool) {
        let storage = Storage::new("objects".into())
            .public_read(public_read)
            .build();
        let mut client = MockS3Api::new();
        client
            .expect_get_bucket_tagging()
            .returning(|_| Err(missing_bucket()));
        client.expect_create_bucket().times(1).returning(|_| Ok(()));
        client
            .expect_put_bucket_abac_tags()
            .times(1)
            .returning(|_, _| Ok(()));
        client
            .expect_put_bucket_versioning()
            .returning(|_, _| Ok(()));
        client
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        client
            .expect_put_bucket_policy()
            .times(1)
            .returning(move |bucket, policy| {
                assert_https_policy(bucket, policy, public_read);
                Ok(())
            });
        let mut run = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController::default())
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
    }

    fn assert_https_policy(bucket: &str, policy: &str, public_read: bool) {
        let policy: serde_json::Value = serde_json::from_str(policy).unwrap();
        assert_eq!(policy["Version"], "2012-10-17");
        let statements = policy["Statement"].as_array().unwrap();
        assert_eq!(statements.len(), if public_read { 2 } else { 1 });
        assert_eq!(
            statements[0],
            serde_json::json!({
                "Sid": "DenyInsecureTransport",
                "Effect": "Deny",
                "Principal": "*",
                "Action": "s3:*",
                "Resource": [format!("arn:aws:s3:::{bucket}"), format!("arn:aws:s3:::{bucket}/*")],
                "Condition": {"Bool": {"aws:SecureTransport": "false"}}
            })
        );
        if public_read {
            assert_eq!(
                statements[1],
                serde_json::json!({
                    "Sid": "AllowPublicRead", "Effect": "Allow", "Principal": "*",
                    "Action": "s3:GetObject", "Resource": format!("arn:aws:s3:::{bucket}/*")
                })
            );
        }
    }

    #[rstest]
    #[case::private_unchanged(false, false)]
    #[case::public_unchanged(true, true)]
    #[case::enable_public(false, true)]
    #[case::disable_public(true, false)]
    #[tokio::test]
    async fn update_keeps_https_requirement(
        #[case] previous_public: bool,
        #[case] public_read: bool,
    ) {
        let storage = Storage::new("objects".into())
            .public_read(public_read)
            .build();
        let mut client = MockS3Api::new();
        client
            .expect_put_public_access_block()
            .returning(|_, _| Ok(()));
        client
            .expect_put_bucket_policy()
            .times(1)
            .returning(move |bucket, policy| {
                assert_https_policy(bucket, policy, public_read);
                Ok(())
            });
        let mut run = SingleControllerExecutor::builder()
            .resource(
                Storage::new("objects".into())
                    .public_read(previous_public)
                    .build(),
            )
            .controller(AwsStorageController::mock_ready("objects"))
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        run.update(storage).unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
    }

    #[rstest]
    #[case::create(AwsStorageState::ConfiguringPublicPolicy)]
    #[case::update(AwsStorageState::UpdatePublicPolicy)]
    #[tokio::test]
    async fn bucket_policy_checkpoint_retries_lost_response(#[case] state: AwsStorageState) {
        let storage = Storage::new("objects".into()).build();
        let mut client = MockS3Api::new();
        let applied = Arc::new(Mutex::new(Vec::new()));
        let policies = applied.clone();
        client
            .expect_put_bucket_policy()
            .times(2)
            .returning(move |bucket, policy| {
                assert_https_policy(bucket, policy, false);
                let mut policies = policies.lock().unwrap();
                policies.push(policy.to_string());
                if policies.len() == 1 {
                    Err(lost_response())
                } else {
                    Ok(())
                }
            });
        client
            .expect_get_bucket_tagging()
            .returning(|_| Ok(our_tags("objects")));
        client.expect_empty_bucket().times(1).returning(|_| Ok(()));
        client.expect_delete_bucket().times(1).returning(|_| Ok(()));
        let mut controller = legacy_recorded("objects");
        controller.state = state;
        let controller: AwsStorageController =
            serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
        let mut run = SingleControllerExecutor::builder()
            .resource(storage.clone())
            .previous_resource(storage)
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(setup_mock_service_provider(Arc::new(client)))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        assert!(run.step().await.is_err());
        assert_eq!(
            run.internal_state::<AwsStorageController>()
                .unwrap()
                .bucket_name
                .as_deref(),
            Some("test-objects")
        );
        run.step().await.unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        {
            let policies = applied.lock().unwrap();
            assert_eq!(policies.len(), 2);
            assert_eq!(policies[0], policies[1]);
        }
        run.delete().unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Deleted);
    }

    #[rstest]
    #[case::unchanged(false)]
    #[case::changed(true)]
    #[tokio::test]
    async fn bucket_policy_checkpoint_preserves_setup_ownership(#[case] changed: bool) {
        let mut controller = legacy_recorded("objects");
        controller.state = AwsStorageState::UpdatePublicPolicy;
        let mut run = SingleControllerExecutor::builder()
            .resource(Storage::new("objects".into()).public_read(changed).build())
            .previous_resource(Storage::new("objects".into()).build())
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .service_provider(setup_mock_service_provider(Arc::new(MockS3Api::new())))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        if changed {
            assert!(run
                .step()
                .await
                .unwrap_err()
                .to_string()
                .contains("setup-owned"));
        } else {
            run.run_until_terminal().await.unwrap();
            assert_eq!(run.status(), ResourceStatus::Running);
        }
    }

    /// Test that verifies deletion works when bucket_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_bucket_name_succeeds() {
        let storage = basic_storage();

        // Create a controller with no bucket name set (simulating early creation failure)
        let controller = AwsStorageController {
            state: AwsStorageState::CreateFailed,
            bucket_name: None, // This is the key - no bucket name set
            requested_bucket_name: None,
            ownership_verified: false,
            read_cors_applied: false,
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

    /// The error a deployment saves when S3 rejects the bucket's lifecycle rules. It is replayed
    /// from a local server through the real S3 client, so it has the same layers a failed
    /// deployment stores. The dashboard, the deploy view and the CLI show a layer only while no
    /// internal layer sits above it; this pins S3's own message inside that visible part.
    #[tokio::test]
    async fn rejected_lifecycle_rules_keep_s3s_message_user_facing() {
        let server = MockServer::start_async().await;
        let lifecycle = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/test-lifecycle-storage")
                    .query_param_exists("lifecycle");
                then.status(400).body(
                    "<Error><Code>InvalidArgument</Code><Message>'Days' for Expiration action must be a positive integer</Message><ArgumentName>Days</ArgumentName><ArgumentValue>0</ArgumentValue><RequestId>W18WKWW5VRW8J6NJ</RequestId></Error>",
                );
            })
            .await;
        let s3: Arc<dyn S3Api> = Arc::new(S3Client::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(AwsClientConfig {
                account_id: "123456789012".into(),
                region: "eu-west-1".into(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "test-access".into(),
                    secret_access_key: "test-secret".into(),
                    session_token: None,
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: HashMap::from([("s3".into(), server.base_url())]),
                }),
            }),
        ));
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(s3.clone()));
        let storage = Storage::new("lifecycle-storage".to_string())
            .lifecycle_rules(vec![AlienLifecycleRule {
                prefix: None,
                days: 0,
            }])
            .build();
        let mut executor = SingleControllerExecutor::builder()
            .resource(storage)
            .controller(AwsStorageController {
                state: AwsStorageState::ConfiguringLifecycle,
                bucket_name: Some("test-lifecycle-storage".to_string()),
                requested_bucket_name: None,
                ownership_verified: true,
                read_cors_applied: false,
                _internal_stay_count: None,
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // The executor saves exactly this error, as `into_generic()`, on the resource.
        let saved = executor
            .step()
            .await
            .expect_err("S3 rejects the lifecycle rules")
            .into_generic();
        // The transport retries the 400 before giving up, so S3 may see it more than once.
        assert!(lifecycle.hits_async().await >= 1);

        assert_eq!(saved.code, "CLOUD_PLATFORM_ERROR");
        assert_eq!(
            saved.message,
            "Cloud platform operation failed: Failed to configure lifecycle rules for S3 bucket 'test-lifecycle-storage'"
        );
        assert!(
            !saved.internal,
            "the controller's summary is shown: {saved:?}"
        );
        // Unchanged retry behavior: the rejection still goes through the retry budget.
        assert!(saved.retryable);

        let rejection = saved.source.as_deref().expect("S3 rejection layer");
        assert_eq!(rejection.code, "REMOTE_REQUEST_REJECTED");
        assert_eq!(
            rejection.message,
            "S3 rejected the request (InvalidArgument): 'Days' for Expiration action must be a positive integer"
        );
        assert!(!rejection.internal, "S3's message is shown: {rejection:?}");

        let transport = rejection.source.as_deref().expect("transport layer");
        assert_eq!(transport.code, "HTTP_RESPONSE_ERROR");
        assert!(
            transport.internal,
            "the raw exchange stays hidden: {transport:?}"
        );
        assert!(transport.source.is_none());
    }
}

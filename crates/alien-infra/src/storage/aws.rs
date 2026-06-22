use std::{collections::HashMap, fmt::Debug, time::Duration};
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    standard_resource_tags, AwsS3StorageHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus, Storage, StorageHeartbeatData, StorageHeartbeatStatus, StorageOutputs,
};
use alien_error::AlienError;
use alien_error::{Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use aws_sdk_s3::{
    error::ProvideErrorMetadata,
    operation::{
        create_bucket::CreateBucketError, delete_bucket::DeleteBucketError,
        delete_bucket_lifecycle::DeleteBucketLifecycleError,
        delete_bucket_policy::DeleteBucketPolicyError, get_bucket_acl::GetBucketAclError,
        get_bucket_encryption::GetBucketEncryptionError,
        get_bucket_lifecycle_configuration::GetBucketLifecycleConfigurationError,
        get_bucket_policy::GetBucketPolicyError,
        get_public_access_block::GetPublicAccessBlockError,
        list_object_versions::ListObjectVersionsError, list_objects_v2::ListObjectsV2Error,
    },
    types::{
        BucketLifecycleConfiguration, BucketLocationConstraint, BucketVersioningStatus,
        CreateBucketConfiguration, Delete, ExpirationStatus, LifecycleExpiration, LifecycleRule,
        LifecycleRuleFilter, ObjectIdentifier, PublicAccessBlockConfiguration, Tag, Tagging,
        VersioningConfiguration,
    },
    Client as S3Client,
};
use chrono::Utc;

/// Generates the full, prefixed AWS bucket name.
fn get_aws_bucket_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

#[derive(Debug, Clone)]
struct BucketMetadata {
    region: String,
    versioning_status: Option<BucketVersioningStatus>,
    lifecycle_rule_count: Option<u64>,
    encryption_rule_count: Option<u64>,
    public_access_block: Option<PublicAccessBlockConfiguration>,
    bucket_policy_present: Option<bool>,
    bucket_acl_present: Option<bool>,
}

async fn create_s3_bucket(client: &S3Client, bucket_name: &str) -> Result<()> {
    let mut request = client.create_bucket().bucket(bucket_name);
    let region = client
        .config()
        .region()
        .map(|region| region.as_ref().to_string())
        .unwrap_or_else(|| "us-east-1".to_string());

    if region != "us-east-1" {
        let configuration = CreateBucketConfiguration::builder()
            .location_constraint(BucketLocationConstraint::from(region.as_str()))
            .build();
        request = request.create_bucket_configuration(configuration);
    }

    match request.send().await {
        Ok(_) => Ok(()),
        Err(err) if is_s3_create_bucket_already_owned(&err) => Ok(()),
        Err(err) => Err(err
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 CreateBucket API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })),
    }
}

async fn put_s3_bucket_abac_tags(
    client: &S3Client,
    bucket_name: &str,
    tags: &HashMap<String, String>,
) -> Result<()> {
    let tag_set = tags
        .iter()
        .map(|(key, value)| {
            Tag::builder()
                .key(key)
                .value(value)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to build S3 tag for bucket '{bucket_name}'"),
                    resource_id: None,
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let tagging = Tagging::builder()
        .set_tag_set(Some(tag_set))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 tagging for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    client
        .put_bucket_tagging()
        .bucket(bucket_name)
        .tagging(tagging)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 PutBucketTagging API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

async fn put_s3_bucket_versioning(
    client: &S3Client,
    bucket_name: &str,
    status: BucketVersioningStatus,
) -> Result<()> {
    let versioning_configuration = VersioningConfiguration::builder().status(status).build();

    client
        .put_bucket_versioning()
        .bucket(bucket_name)
        .versioning_configuration(versioning_configuration)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 PutBucketVersioning API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

async fn put_s3_public_access_block(
    client: &S3Client,
    bucket_name: &str,
    config: PublicAccessBlockConfiguration,
) -> Result<()> {
    client
        .put_public_access_block()
        .bucket(bucket_name)
        .public_access_block_configuration(config)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 PutPublicAccessBlock API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

async fn put_s3_bucket_policy(client: &S3Client, bucket_name: &str, policy: &str) -> Result<()> {
    client
        .put_bucket_policy()
        .bucket(bucket_name)
        .policy(policy)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 PutBucketPolicy API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

async fn delete_s3_bucket_policy(client: &S3Client, bucket_name: &str) -> Result<()> {
    match client
        .delete_bucket_policy()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(_) => Ok(()),
        Err(err) if is_s3_delete_bucket_policy_not_found(&err) => Ok(()),
        Err(err) => Err(err
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 DeleteBucketPolicy API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })),
    }
}

async fn put_s3_bucket_lifecycle_configuration(
    client: &S3Client,
    bucket_name: &str,
    rules: Vec<LifecycleRule>,
) -> Result<()> {
    let configuration = BucketLifecycleConfiguration::builder()
        .set_rules(Some(rules))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build S3 lifecycle configuration for bucket '{bucket_name}'"
            ),
            resource_id: None,
        })?;

    client
        .put_bucket_lifecycle_configuration()
        .bucket(bucket_name)
        .lifecycle_configuration(configuration)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "S3 PutBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
            ),
            resource_id: None,
        })?;

    Ok(())
}

async fn delete_s3_bucket_lifecycle(client: &S3Client, bucket_name: &str) -> Result<()> {
    match client
        .delete_bucket_lifecycle()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(_) => Ok(()),
        Err(err) if is_s3_delete_bucket_lifecycle_not_found(&err) => Ok(()),
        Err(err) => Err(err
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 DeleteBucketLifecycle API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })),
    }
}

async fn get_s3_bucket_metadata(client: &S3Client, bucket_name: &str) -> Result<BucketMetadata> {
    let location = match client
        .get_bucket_location()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 GetBucketLocation API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                }));
        }
    };

    let versioning = match client
        .get_bucket_versioning()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 GetBucketVersioning API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                }));
        }
    };

    let lifecycle_rule_count = match client
        .get_bucket_lifecycle_configuration()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => Some(output.rules().len() as u64),
        Err(err) if is_s3_get_lifecycle_not_found(&err) => None,
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 GetBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                }));
        }
    };

    let encryption_rule_count = match client
        .get_bucket_encryption()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => output
            .server_side_encryption_configuration()
            .map(|configuration| configuration.rules().len() as u64),
        Err(err) if is_s3_get_encryption_not_found(&err) => None,
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 GetBucketEncryption API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                }));
        }
    };

    let public_access_block = match client
        .get_public_access_block()
        .bucket(bucket_name)
        .send()
        .await
    {
        Ok(output) => output.public_access_block_configuration,
        Err(err) if is_s3_get_public_access_block_not_found(&err) => None,
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 GetPublicAccessBlock API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                }));
        }
    };

    let bucket_policy_present = match client.get_bucket_policy().bucket(bucket_name).send().await {
        Ok(output) => Some(
            output
                .policy()
                .is_some_and(|policy| !policy.trim().is_empty()),
        ),
        Err(err) if is_s3_get_bucket_policy_not_found(&err) => Some(false),
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 GetBucketPolicy API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                }));
        }
    };

    let bucket_acl_present = match client.get_bucket_acl().bucket(bucket_name).send().await {
        Ok(output) => Some(output.owner().is_some() || !output.grants().is_empty()),
        Err(err) if is_s3_get_bucket_acl_not_found(&err) => Some(false),
        Err(err) => {
            return Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 GetBucketAcl API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                }));
        }
    };

    Ok(BucketMetadata {
        region: s3_bucket_location_region(location.location_constraint().map(|c| c.as_str())),
        versioning_status: versioning.status().cloned(),
        lifecycle_rule_count,
        encryption_rule_count,
        public_access_block,
        bucket_policy_present,
        bucket_acl_present,
    })
}

async fn empty_s3_bucket(client: &S3Client, bucket_name: &str) -> Result<()> {
    let mut key_marker = None;
    let mut version_id_marker = None;

    loop {
        match client
            .list_object_versions()
            .bucket(bucket_name)
            .set_key_marker(key_marker.clone())
            .set_version_id_marker(version_id_marker.clone())
            .max_keys(1000)
            .send()
            .await
        {
            Ok(output) => {
                let mut objects =
                    Vec::with_capacity(output.versions().len() + output.delete_markers().len());
                for version in output.versions() {
                    if let (Some(key), Some(version_id)) = (version.key(), version.version_id()) {
                        objects.push(s3_object_identifier(key, Some(version_id))?);
                    }
                }
                for marker in output.delete_markers() {
                    if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id()) {
                        objects.push(s3_object_identifier(key, Some(version_id))?);
                    }
                }

                if !objects.is_empty() {
                    delete_s3_objects(client, bucket_name, objects).await?;
                }

                if output.is_truncated().unwrap_or(false) {
                    key_marker = output.next_key_marker().map(ToString::to_string);
                    version_id_marker = output.next_version_id_marker().map(ToString::to_string);
                    continue;
                }

                break;
            }
            Err(err) if is_s3_list_versions_bucket_not_found(&err) => return Ok(()),
            Err(err) if is_s3_list_versions_invalid_argument(&err) => break,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 ListObjectVersions API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        }
    }

    let mut continuation_token = None;
    loop {
        let output = match client
            .list_objects_v2()
            .bucket(bucket_name)
            .set_continuation_token(continuation_token.clone())
            .max_keys(1000)
            .send()
            .await
        {
            Ok(output) => output,
            Err(err) if is_s3_list_objects_bucket_not_found(&err) => return Ok(()),
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("S3 ListObjectsV2 API failed for bucket '{bucket_name}'"),
                        resource_id: None,
                    }));
            }
        };

        let objects = output
            .contents()
            .iter()
            .filter_map(|object| object.key())
            .map(|key| s3_object_identifier(key, None))
            .collect::<Result<Vec<_>>>()?;

        if !objects.is_empty() {
            delete_s3_objects(client, bucket_name, objects).await?;
        }

        if output.is_truncated().unwrap_or(false) {
            continuation_token = output.next_continuation_token().map(ToString::to_string);
        } else {
            break;
        }
    }

    Ok(())
}

async fn delete_s3_bucket(client: &S3Client, bucket_name: &str) -> Result<bool> {
    match client.delete_bucket().bucket(bucket_name).send().await {
        Ok(_) => Ok(true),
        Err(err) if is_s3_delete_bucket_not_found(&err) => Ok(false),
        Err(err) => Err(err
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 DeleteBucket API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })),
    }
}

fn is_s3_create_bucket_already_owned(
    error: &aws_sdk_s3::error::SdkError<CreateBucketError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(CreateBucketError::is_bucket_already_owned_by_you)
}

fn is_s3_delete_bucket_not_found(error: &aws_sdk_s3::error::SdkError<DeleteBucketError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_delete_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_delete_bucket_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketLifecycleError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketLifecycleConfigurationError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_encryption_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketEncryptionError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &[
            "NoSuchBucket",
            "ServerSideEncryptionConfigurationNotFoundError",
        ],
    )
}

fn is_s3_get_public_access_block_not_found(
    error: &aws_sdk_s3::error::SdkError<GetPublicAccessBlockError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchPublicAccessBlockConfiguration"],
    )
}

fn is_s3_get_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_get_bucket_acl_not_found(error: &aws_sdk_s3::error::SdkError<GetBucketAclError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_invalid_argument(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["InvalidArgument"])
}

fn is_s3_list_objects_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectsV2Error>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(ListObjectsV2Error::is_no_such_bucket)
}

fn s3_error_code<E>(error: Option<&E>, codes: &[&str]) -> bool
where
    E: ProvideErrorMetadata,
{
    error
        .and_then(ProvideErrorMetadata::code)
        .is_some_and(|code| codes.contains(&code))
}

fn s3_bucket_location_region(location_constraint: Option<&str>) -> String {
    match location_constraint {
        None | Some("") => "us-east-1".to_string(),
        Some("EU") => "eu-west-1".to_string(),
        Some(region) => region.to_string(),
    }
}

fn s3_object_identifier(key: &str, version_id: Option<&str>) -> Result<ObjectIdentifier> {
    ObjectIdentifier::builder()
        .key(key)
        .set_version_id(version_id.map(ToString::to_string))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 object identifier for key '{key}'"),
            resource_id: None,
        })
}

async fn delete_s3_objects(
    client: &S3Client,
    bucket_name: &str,
    objects: Vec<ObjectIdentifier>,
) -> Result<()> {
    let delete = Delete::builder()
        .set_objects(Some(objects))
        .quiet(true)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 DeleteObjects request for '{bucket_name}'"),
            resource_id: None,
        })?;

    client
        .delete_objects()
        .bucket(bucket_name)
        .delete(delete)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 DeleteObjects API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
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
        self.bucket_name = Some(bucket_name.clone());

        info!(name=%config.id, bucket=%bucket_name, "Creating S3 bucket");

        create_s3_bucket(&client, &bucket_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create S3 bucket '{}'", bucket_name),
                resource_id: Some(config.id.clone()),
            })?;

        put_s3_bucket_abac_tags(
            &client,
            &bucket_name,
            &standard_resource_tags(ctx.resource_prefix, &config.id),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to tag S3 bucket '{}'", bucket_name),
            resource_id: Some(config.id.clone()),
        })?;

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

            put_s3_bucket_versioning(&client, bucket_name, BucketVersioningStatus::Enabled)
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

            let public_access_config = public_access_block_config(false);

            put_s3_public_access_block(&client, bucket_name, public_access_config)
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

            put_s3_bucket_policy(&client, bucket_name, &policy.to_string())
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

            put_s3_bucket_lifecycle_configuration(
                &client,
                bucket_name,
                lifecycle_rule_configs(config)?,
            )
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

            let metadata = get_s3_bucket_metadata(&client, bucket_name).await.context(
                ErrorData::CloudPlatformError {
                    message: "Failed to collect S3 bucket metadata during heartbeat".to_string(),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            emit_aws_s3_storage_heartbeat(ctx, &config.id, bucket_name, metadata);

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

            let status = if config.versioning {
                BucketVersioningStatus::Enabled
            } else {
                BucketVersioningStatus::Suspended
            };

            put_s3_bucket_versioning(&client, bucket_name, status)
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
                let public_access_config = public_access_block_config(false);

                put_s3_public_access_block(&client, bucket_name, public_access_config)
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
                let public_access_config = public_access_block_config(true);

                put_s3_public_access_block(&client, bucket_name, public_access_config)
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

                put_s3_bucket_policy(&client, bucket_name, &policy.to_string())
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
                delete_s3_bucket_policy(&client, bucket_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to remove bucket policy for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                info!(bucket=%bucket_name, "Bucket policy removed successfully");
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
                delete_s3_bucket_lifecycle(&client, bucket_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to remove lifecycle configuration for S3 bucket '{}'",
                            bucket_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                info!(bucket=%bucket_name, "Lifecycle rules removed successfully");
            } else {
                put_s3_bucket_lifecycle_configuration(
                    &client,
                    bucket_name,
                    lifecycle_rule_configs(config)?,
                )
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
        match empty_s3_bucket(&client, bucket_name).await {
            Ok(_) => {
                info!(bucket=%bucket_name, "Bucket emptied successfully");
            }
            Err(e) => {
                // Log but continue - bucket might not exist or might already be empty
                info!(bucket=%bucket_name, error=?e, "Could not empty bucket, continuing with deletion attempt");
            }
        }

        match delete_s3_bucket(&client, bucket_name).await {
            Ok(true) => {
                info!(bucket=%bucket_name, "S3 bucket deleted successfully");
            }
            Ok(false) => {
                info!(bucket=%bucket_name, "Bucket already deleted or never existed");
            }
            Err(e) => {
                info!(bucket=%bucket_name, error=?e, "Could not delete bucket, considering deletion complete");
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
    metadata: BucketMetadata,
) {
    let versioning_enabled = Some(matches!(
        metadata.versioning_status.as_ref(),
        Some(BucketVersioningStatus::Enabled)
    ));
    let versioning_status = metadata
        .versioning_status
        .map(|status| status.as_str().to_string());
    let lifecycle_present = metadata
        .lifecycle_rule_count
        .map(|count| count > 0)
        .unwrap_or(false);
    let encryption_config_present = metadata.encryption_rule_count.is_some();
    let encryption_enabled = metadata.encryption_rule_count.map(|count| count > 0);
    let public_access_block_present = metadata.public_access_block.is_some();

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
                region: Some(metadata.region.clone()),
                bucket_location: Some(metadata.region),
                versioning_status,
                versioning_enabled,
                lifecycle_present,
                lifecycle_rule_count: metadata.lifecycle_rule_count,
                encryption_config_present,
                encryption_enabled,
                public_access_block_present,
                block_public_acls: metadata
                    .public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.block_public_acls()),
                ignore_public_acls: metadata
                    .public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.ignore_public_acls()),
                block_public_policy: metadata
                    .public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.block_public_policy()),
                restrict_public_buckets: metadata
                    .public_access_block
                    .as_ref()
                    .and_then(|configuration| configuration.restrict_public_buckets()),
                bucket_policy_present: metadata.bucket_policy_present,
                bucket_acl_present: metadata.bucket_acl_present,
            },
        )),
        raw: vec![],
    });
}

fn public_access_block_config(blocked: bool) -> PublicAccessBlockConfiguration {
    PublicAccessBlockConfiguration::builder()
        .block_public_acls(blocked)
        .ignore_public_acls(blocked)
        .block_public_policy(blocked)
        .restrict_public_buckets(blocked)
        .build()
}

fn lifecycle_rule_configs(config: &Storage) -> Result<Vec<LifecycleRule>> {
    config
        .lifecycle_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let rule_id = format!("Rule{}", index + 1);
            let days = i32::try_from(rule.days).map_err(|_| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Lifecycle rule '{}' expiration days exceeds the S3 API limit",
                        rule_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            let expiration = LifecycleExpiration::builder().days(days).build();
            let filter = LifecycleRuleFilter::builder()
                .set_prefix(rule.prefix.clone())
                .build();

            LifecycleRule::builder()
                .id(rule_id)
                .status(ExpirationStatus::Enabled)
                .filter(filter)
                .expiration(expiration)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build S3 lifecycle rule".to_string(),
                    resource_id: Some(config.id.clone()),
                })
        })
        .collect()
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

    use alien_core::{
        LifecycleRule as AlienLifecycleRule, Platform, ResourceStatus, Storage, StorageOutputs,
    };
    use aws_sdk_s3::{
        error::ErrorMetadata as S3ErrorMetadata,
        operation::{
            create_bucket::CreateBucketOutput, delete_bucket::DeleteBucketOutput,
            delete_bucket_lifecycle::DeleteBucketLifecycleOutput,
            delete_bucket_policy::DeleteBucketPolicyOutput,
            list_object_versions::ListObjectVersionsError, list_objects_v2::ListObjectsV2Output,
            put_bucket_lifecycle_configuration::PutBucketLifecycleConfigurationOutput,
            put_bucket_policy::PutBucketPolicyOutput, put_bucket_tagging::PutBucketTaggingOutput,
            put_bucket_versioning::PutBucketVersioningOutput,
            put_public_access_block::PutPublicAccessBlockOutput,
        },
        types::ExpirationStatus,
        Client as S3Client,
    };
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use aws_smithy_mocks::{mock, mock_client, Rule, RuleMode};
    use rstest::{fixture, rstest};

    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
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

    fn s3_generic_error(code: &str, message: &str) -> S3ErrorMetadata {
        S3ErrorMetadata::builder()
            .code(code)
            .message(message)
            .build()
    }

    fn basic_s3_creation_rules() -> Vec<Rule> {
        let create_bucket_rule = mock!(S3Client::create_bucket)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| CreateBucketOutput::builder().build());
        let tag_rule = mock!(S3Client::put_bucket_tagging)
            .match_requests(|request| {
                request.bucket().is_some()
                    && request
                        .tagging()
                        .is_some_and(|tagging| !tagging.tag_set().is_empty())
            })
            .then_output(|| PutBucketTaggingOutput::builder().build());
        let versioning_rule = mock!(S3Client::put_bucket_versioning)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| PutBucketVersioningOutput::builder().build());
        let public_access_rule = mock!(S3Client::put_public_access_block)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| PutPublicAccessBlockOutput::builder().build());
        let policy_rule = mock!(S3Client::put_bucket_policy)
            .match_requests(|request| request.bucket().is_some() && request.policy().is_some())
            .then_output(|| PutBucketPolicyOutput::builder().build());
        let lifecycle_rule = mock!(S3Client::put_bucket_lifecycle_configuration)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| PutBucketLifecycleConfigurationOutput::builder().build());

        vec![
            create_bucket_rule,
            tag_rule,
            versioning_rule,
            public_access_rule,
            policy_rule,
            lifecycle_rule,
        ]
    }

    fn deletion_s3_rules() -> Vec<Rule> {
        let list_versions_rule = mock!(S3Client::list_object_versions)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| {
                aws_sdk_s3::operation::list_object_versions::ListObjectVersionsOutput::builder()
                    .build()
            });
        let list_objects_rule = mock!(S3Client::list_objects_v2)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| ListObjectsV2Output::builder().build());
        let delete_bucket_rule = mock!(S3Client::delete_bucket)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| DeleteBucketOutput::builder().build());

        vec![list_versions_rule, list_objects_rule, delete_bucket_rule]
    }

    fn setup_mock_client_for_creation_and_deletion(_bucket_name: &str) -> S3Client {
        let mut rules = basic_s3_creation_rules();
        rules.extend(deletion_s3_rules());
        let rule_refs = rules.iter().collect::<Vec<_>>();
        mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            rule_refs.as_slice(),
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_client_for_creation_and_update(_bucket_name: &str) -> S3Client {
        let mut rules = basic_s3_creation_rules();
        let delete_policy_rule = mock!(S3Client::delete_bucket_policy)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| DeleteBucketPolicyOutput::builder().build());
        let delete_lifecycle_rule = mock!(S3Client::delete_bucket_lifecycle)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| DeleteBucketLifecycleOutput::builder().build());
        rules.push(delete_policy_rule);
        rules.push(delete_lifecycle_rule);
        let rule_refs = rules.iter().collect::<Vec<_>>();
        mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            rule_refs.as_slice(),
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_client_for_best_effort_deletion(_bucket_name: &str) -> S3Client {
        let list_versions_rule = mock!(S3Client::list_object_versions)
            .match_requests(|request| request.bucket().is_some())
            .then_error(|| {
                ListObjectVersionsError::generic(s3_generic_error(
                    "InternalError",
                    "forced list failure",
                ))
            });
        let delete_bucket_rule = mock!(S3Client::delete_bucket)
            .match_requests(|request| request.bucket().is_some())
            .then_error(|| {
                aws_sdk_s3::operation::delete_bucket::DeleteBucketError::generic(s3_generic_error(
                    "NoSuchBucket",
                    "bucket missing",
                ))
            });
        mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            [&list_versions_rule, &delete_bucket_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        )
    }

    fn setup_mock_service_provider(mock_s3: S3Client) -> Arc<MockPlatformServiceProvider> {
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

        let list_versions_rule = mock!(S3Client::list_object_versions)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| {
                aws_sdk_s3::operation::list_object_versions::ListObjectVersionsOutput::builder()
                    .build()
            });
        let list_objects_rule = mock!(S3Client::list_objects_v2)
            .match_requests(|request| request.bucket().is_some())
            .then_output(|| ListObjectsV2Output::builder().build());
        let delete_bucket_rule = mock!(S3Client::delete_bucket)
            .match_requests(|request| request.bucket().is_some())
            .then_error(|| {
                aws_sdk_s3::operation::delete_bucket::DeleteBucketError::generic(s3_generic_error(
                    "InternalError",
                    "forced delete failure",
                ))
            });
        let s3_client = mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            [&list_versions_rule, &list_objects_rule, &delete_bucket_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(s3_client);

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

        let create_bucket_rule = mock!(S3Client::create_bucket)
            .match_requests(|request| request.bucket() == Some("test-my-awesome-storage"))
            .then_output(|| CreateBucketOutput::builder().build());
        let tag_rule = mock!(S3Client::put_bucket_tagging)
            .match_requests(|request| request.bucket() == Some("test-my-awesome-storage"))
            .then_output(|| PutBucketTaggingOutput::builder().build());
        let s3_client = mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            [&create_bucket_rule, &tag_rule],
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(s3_client);

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

        let mut rules = basic_s3_creation_rules();
        let lifecycle_rule = mock!(S3Client::put_bucket_lifecycle_configuration)
            .match_requests(|request| {
                let Some(configuration) = request.lifecycle_configuration() else {
                    return false;
                };
                let lifecycle_rules = configuration.rules();
                if lifecycle_rules.len() != 2 {
                    eprintln!("Expected 2 lifecycle rules, got {}", lifecycle_rules.len());
                    return false;
                }

                let rule1 = &lifecycle_rules[0];
                if rule1.id() != Some("Rule1") {
                    eprintln!("Expected rule ID 'Rule1', got {:?}", rule1.id());
                    return false;
                }
                let rule1_prefix = rule1.filter().and_then(|filter| filter.prefix());
                if rule1_prefix != Some("logs/") {
                    eprintln!("Expected prefix 'logs/', got {:?}", rule1_prefix);
                    return false;
                }
                let rule1_days = rule1.expiration().and_then(|expiration| expiration.days());
                if rule1_days != Some(30) {
                    eprintln!("Expected 30 days, got {:?}", rule1_days);
                    return false;
                }
                if rule1.status() != &ExpirationStatus::Enabled {
                    eprintln!("Expected enabled status, got {:?}", rule1.status());
                    return false;
                }

                let rule2 = &lifecycle_rules[1];
                if rule2.id() != Some("Rule2") {
                    eprintln!("Expected rule ID 'Rule2', got {:?}", rule2.id());
                    return false;
                }
                let rule2_prefix = rule2.filter().and_then(|filter| filter.prefix());
                if rule2_prefix.is_some() {
                    eprintln!("Expected no prefix, got {:?}", rule2_prefix);
                    return false;
                }
                let rule2_days = rule2.expiration().and_then(|expiration| expiration.days());
                if rule2_days != Some(365) {
                    eprintln!("Expected 365 days, got {:?}", rule2_days);
                    return false;
                }
                if rule2.status() != &ExpirationStatus::Enabled {
                    eprintln!("Expected enabled status, got {:?}", rule2.status());
                    return false;
                }

                true
            })
            .then_output(|| PutBucketLifecycleConfigurationOutput::builder().build());
        rules.insert(0, lifecycle_rule);
        let rule_refs = rules.iter().collect::<Vec<_>>();
        let s3_client = mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            rule_refs.as_slice(),
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(s3_client);

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

        let mut rules = basic_s3_creation_rules();
        let public_access_rule = mock!(S3Client::put_public_access_block)
            .match_requests(|request| {
                let Some(config) = request.public_access_block_configuration() else {
                    return false;
                };
                config.block_public_acls() == Some(false)
                    && config.block_public_policy() == Some(false)
                    && config.ignore_public_acls() == Some(false)
                    && config.restrict_public_buckets() == Some(false)
            })
            .then_output(|| PutPublicAccessBlockOutput::builder().build());

        // Validate bucket policy for public read access
        let policy_rule = mock!(S3Client::put_bucket_policy)
            .match_requests(|request| {
                let Some(bucket_name) = request.bucket() else {
                    return false;
                };
                let Some(policy) = request.policy() else {
                    return false;
                };
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
            .then_output(|| PutBucketPolicyOutput::builder().build());
        rules.insert(0, policy_rule);
        rules.insert(0, public_access_rule);
        let rule_refs = rules.iter().collect::<Vec<_>>();
        let s3_client = mock_client!(
            aws_sdk_s3,
            RuleMode::MatchAny,
            rule_refs.as_slice(),
            |config| config.sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        );
        let mock_provider = setup_mock_service_provider(s3_client);

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

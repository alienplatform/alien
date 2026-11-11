//! Cloud writes for a resource of the signed stack that is not a workload: a bucket policy, a
//! queue's attributes and the like. They decide who can reach the install's data, so a
//! controller builds the same request it builds for an unprotected install and hands it to the
//! verifier, which checks it against the signed stack and sends it unchanged.
//!
//! The check is always the same for who may reach the resource: everything a request allows must
//! be what the signed stack declares for that resource (see [`alien_permissions::aws_bucket_policy`]),
//! never management, the verifier or the setup identity.

use std::collections::BTreeMap;

use alien_core::{Queue, ResourceEntry, Stack, Storage};
use alien_permissions::{
    aws_bucket_policy::{check_bucket_policy, check_public_access_block},
    aws_queue_policy::check_queue_policy,
    aws_resource_policies,
};
use alien_signing::SignedStatement;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::refused;
use crate::error::Result;
use crate::function::StillNewest;
use crate::plan::Place;
use crate::trust::Accepted;

/// Apply the cloud writes a controller built for a resource of the signed stack, in order. Every
/// write is checked before any is sent, so a refusal sends none.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyResourceWrite {
    /// The release, as the vendor signed it.
    pub release: SignedStatement,
    /// The signer list chain, from version 1.
    pub signer_lists: Vec<SignedStatement>,
    /// This install's config, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    /// The resource's id in the signed stack.
    pub resource: String,
    /// The writes, each exactly as the controller would send it to the cloud.
    pub writes: Vec<ResourceWrite>,
}

/// One cloud write for a stack resource, as the cloud client sends it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ResourceWrite {
    /// S3 PutBucketPolicy, with the policy document as the controller sends it.
    #[serde(rename_all = "camelCase")]
    PutBucketPolicy { bucket: String, policy: String },
    /// S3 PutPublicAccessBlock, with the configuration as the cloud client serialises it.
    #[serde(rename_all = "camelCase")]
    PutPublicAccessBlock {
        bucket: String,
        configuration: Value,
    },
    /// SQS CreateQueue, as the controller sends it: no attributes.
    #[serde(rename_all = "camelCase")]
    CreateQueue {
        queue_name: String,
        #[serde(default)]
        attributes: BTreeMap<String, String>,
        #[serde(default)]
        tags: BTreeMap<String, String>,
    },
    /// SQS SetQueueAttributes: the visibility timeout and the queue policy.
    #[serde(rename_all = "camelCase")]
    SetQueueAttributes {
        queue_url: String,
        attributes: BTreeMap<String, String>,
    },
}

impl ResourceWrite {
    /// The cloud operation, for messages.
    pub fn operation(&self) -> &'static str {
        match self {
            ResourceWrite::PutBucketPolicy { .. } => "PutBucketPolicy",
            ResourceWrite::PutPublicAccessBlock { .. } => "PutPublicAccessBlock",
            ResourceWrite::CreateQueue { .. } => "CreateQueue",
            ResourceWrite::SetQueueAttributes { .. } => "SetQueueAttributes",
        }
    }
}

/// What the verifier knows about the resource a write is for.
pub struct ResourceFacts<'a> {
    pub place: &'a Place,
    pub accepted: &'a Accepted,
    /// The signed stack.
    pub stack: Stack,
    /// The resource's id.
    pub resource_id: &'a str,
}

impl ResourceFacts<'_> {
    /// The resource as the signed stack declares it.
    pub fn entry(&self) -> Result<&ResourceEntry> {
        self.stack.resources.get(self.resource_id).ok_or_else(|| {
            refused(format!(
                "resource '{}' is not in the signed stack",
                self.resource_id
            ))
        })
    }

    /// The resource, which must be a storage of the signed stack.
    pub fn storage(&self) -> Result<&Storage> {
        self.entry()?
            .config
            .downcast_ref::<Storage>()
            .ok_or_else(|| {
                refused(format!(
                    "resource '{}' is not a storage in the signed stack",
                    self.resource_id
                ))
            })
    }
}

/// The signed stack of an accepted release.
pub fn signed_stack(accepted: &Accepted, place: &Place) -> Result<Stack> {
    let platform: alien_core::Platform = place
        .platform()
        .parse()
        .map_err(|error: String| refused(error))?;
    alien_core::release_statement::signed_stack(&accepted.release.body, platform)
        .map_err(|error| refused(format!("the signed stack can't be read: {error}")))
}

/// The ARN partition of an AWS region.
pub fn aws_partition(region: &str) -> &'static str {
    if region.starts_with("us-gov-") {
        "aws-us-gov"
    } else if region.starts_with("cn-") {
        "aws-cn"
    } else {
        "aws"
    }
}

impl ResourceFacts<'_> {
    /// The resource, which must be a queue of the signed stack.
    pub fn queue(&self) -> Result<&Queue> {
        self.entry()?.config.downcast_ref::<Queue>().ok_or_else(|| {
            refused(format!(
                "resource '{}' is not a queue in the signed stack",
                self.resource_id
            ))
        })
    }
}

/// The largest visibility timeout SQS accepts, in seconds.
const MAX_VISIBILITY_TIMEOUT: u32 = 43_200;

/// The storage a bucket write is for: the write must name the storage's own bucket.
fn own_bucket<'a>(
    facts: &'a ResourceFacts<'_>,
    bucket: &str,
    operation: &str,
) -> Result<&'a Storage> {
    let storage = facts.storage()?;
    let own =
        aws_resource_policies::aws_prefixed_resource_name(&facts.place.prefix, facts.resource_id);
    if bucket != own {
        return Err(refused(format!(
            "{operation} names bucket {bucket}, not {own}, the name this install gives storage '{}'",
            facts.resource_id
        )));
    }
    Ok(storage)
}

/// Check a write for an AWS resource: it must name the resource's own cloud name, and everything
/// it allows must be what the signed stack declares.
pub fn check_aws_write(
    facts: &ResourceFacts<'_>,
    write: &ResourceWrite,
    region: &str,
) -> Result<()> {
    match write {
        ResourceWrite::PutBucketPolicy { bucket, policy } => {
            let storage = own_bucket(facts, bucket, "PutBucketPolicy")?;
            let document: Value = serde_json::from_str(policy)
                .map_err(|error| refused(format!("the bucket policy is not JSON: {error}")))?;
            let arn = format!("arn:{}:s3:::{bucket}", aws_partition(region));
            check_bucket_policy(&document, &arn, storage.public_read).map_err(refused)
        }
        ResourceWrite::PutPublicAccessBlock {
            bucket,
            configuration,
        } => {
            let storage = own_bucket(facts, bucket, "PutPublicAccessBlock")?;
            check_public_access_block(configuration, storage.public_read).map_err(refused)
        }
        ResourceWrite::CreateQueue {
            queue_name,
            attributes,
            ..
        } => {
            facts.queue()?;
            let own = own_queue_name(facts);
            if *queue_name != own {
                return Err(refused(format!(
                    "CreateQueue names queue {queue_name}, not {own}, the name this install \
                     gives queue '{}'",
                    facts.resource_id
                )));
            }
            if let Some(name) = attributes.keys().next() {
                return Err(refused(format!(
                    "CreateQueue sets attribute {name}; a queue is created without any, and its \
                     policy is set by SetQueueAttributes"
                )));
            }
            Ok(())
        }
        ResourceWrite::SetQueueAttributes {
            queue_url,
            attributes,
        } => {
            facts.queue()?;
            let (account, region) = aws_account_and_region(facts)?;
            let own = own_queue_name(facts);
            let expected_url = format!("https://sqs.{region}.amazonaws.com/{account}/{own}");
            if *queue_url != expected_url {
                return Err(refused(format!(
                    "SetQueueAttributes names queue {queue_url}, not {expected_url}, the queue \
                     this install gives '{}'",
                    facts.resource_id
                )));
            }
            for (name, value) in attributes {
                match name.as_str() {
                    "VisibilityTimeout" => {
                        let seconds: u32 = value.parse().map_err(|_| {
                            refused(format!("VisibilityTimeout '{value}' is not a number"))
                        })?;
                        if seconds > MAX_VISIBILITY_TIMEOUT {
                            return Err(refused(format!(
                                "VisibilityTimeout {seconds} is over the {MAX_VISIBILITY_TIMEOUT} \
                                 seconds SQS allows"
                            )));
                        }
                    }
                    // An empty policy removes it.
                    "Policy" if value.is_empty() => {}
                    "Policy" => {
                        let partition = aws_partition(&region);
                        let document: Value = serde_json::from_str(value).map_err(|error| {
                            refused(format!("the queue policy is not JSON: {error}"))
                        })?;
                        let buckets: Vec<String> =
                            alien_core::storage_notifications::storages_notifying_queue(
                                &facts.stack,
                                facts.resource_id,
                            )
                            .iter()
                            .map(|storage| {
                                format!(
                                    "arn:{partition}:s3:::{}",
                                    aws_resource_policies::aws_prefixed_resource_name(
                                        &facts.place.prefix,
                                        storage
                                    )
                                )
                            })
                            .collect();
                        check_queue_policy(
                            &document,
                            alien_core::storage_notifications::AWS_QUEUE_INGRESS_SID,
                            &format!("arn:{partition}:sqs:{region}:{account}:{own}"),
                            &account,
                            &buckets,
                        )
                        .map_err(refused)?;
                    }
                    other => {
                        return Err(refused(format!(
                            "SetQueueAttributes sets attribute {other}, which is not one the \
                             verifier applies"
                        )))
                    }
                }
            }
            Ok(())
        }
    }
}

fn own_queue_name(facts: &ResourceFacts<'_>) -> String {
    aws_resource_policies::aws_prefixed_resource_name(&facts.place.prefix, facts.resource_id)
}

/// The install's account and region.
fn aws_account_and_region(facts: &ResourceFacts<'_>) -> Result<(String, String)> {
    match &facts.place.install {
        alien_signing::InstallId::Aws {
            account, region, ..
        } => Ok((account.clone(), region.clone())),
        _ => Err(refused("this is not an AWS install")),
    }
}

/// Send a checked AWS write unchanged, after confirming no newer release was accepted.
pub async fn forward_aws_write(
    s3: &dyn alien_aws_clients::s3::S3Api,
    sqs: &dyn alien_aws_clients::sqs::SqsApi,
    write: &ResourceWrite,
    newest: &dyn StillNewest,
) -> Result<Value> {
    use alien_error::Context;
    newest.check().await?;
    match write {
        ResourceWrite::PutBucketPolicy { bucket, policy } => {
            s3.put_bucket_policy(bucket, policy).await.context(
                crate::error::ErrorData::CloudCallFailed {
                    operation: format!("s3:PutBucketPolicy {bucket}"),
                },
            )?;
            Ok(json!({ "bucket": bucket }))
        }
        ResourceWrite::PutPublicAccessBlock {
            bucket,
            configuration,
        } => {
            let configuration: alien_aws_clients::s3::PublicAccessBlockConfiguration =
                serde_json::from_value(configuration.clone()).map_err(|error| {
                    refused(format!("the public access block does not parse: {error}"))
                })?;
            s3.put_public_access_block(bucket, configuration)
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("s3:PutPublicAccessBlock {bucket}"),
                })?;
            Ok(json!({ "bucket": bucket }))
        }
        ResourceWrite::CreateQueue {
            queue_name, tags, ..
        } => {
            let created = sqs
                .create_queue(
                    alien_aws_clients::sqs::CreateQueueRequest::builder()
                        .queue_name(queue_name.clone())
                        .tags(tags.clone().into_iter().collect())
                        .build(),
                )
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("sqs:CreateQueue {queue_name}"),
                })?;
            Ok(json!({ "queueUrl": created.create_queue_result.queue_url }))
        }
        ResourceWrite::SetQueueAttributes {
            queue_url,
            attributes,
        } => {
            sqs.set_queue_attributes(
                queue_url,
                alien_aws_clients::sqs::SetQueueAttributesRequest::builder()
                    .attributes(attributes.clone().into_iter().collect())
                    .build(),
            )
            .await
            .context(crate::error::ErrorData::CloudCallFailed {
                operation: format!("sqs:SetQueueAttributes {queue_url}"),
            })?;
            Ok(json!({ "queueUrl": queue_url }))
        }
    }
}

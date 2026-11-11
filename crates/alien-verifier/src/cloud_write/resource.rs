//! Cloud writes for a resource of the signed stack that is not a workload: a bucket policy, a
//! queue's attributes and the like. They decide who can reach the install's data, so a
//! controller builds the same request it builds for an unprotected install and hands it to the
//! verifier, which checks it against the signed stack and sends it unchanged.
//!
//! The check is always the same for who may reach the resource: everything a request allows must
//! be what the signed stack declares for that resource (see [`alien_permissions::aws_bucket_policy`]),
//! never management, the verifier or the setup identity.

use alien_core::{ResourceEntry, Stack, Storage};
use alien_permissions::{aws_bucket_policy::check_bucket_policy, aws_resource_policies};
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
}

impl ResourceWrite {
    /// The cloud operation, for messages.
    pub fn operation(&self) -> &'static str {
        match self {
            ResourceWrite::PutBucketPolicy { .. } => "PutBucketPolicy",
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

/// Check a write for an AWS resource: it must name the resource's own cloud name, and everything
/// it allows must be what the signed stack declares.
pub fn check_aws_write(
    facts: &ResourceFacts<'_>,
    write: &ResourceWrite,
    region: &str,
) -> Result<()> {
    match write {
        ResourceWrite::PutBucketPolicy { bucket, policy } => {
            let storage = facts.storage()?;
            let own = aws_resource_policies::aws_prefixed_resource_name(
                &facts.place.prefix,
                facts.resource_id,
            );
            if *bucket != own {
                return Err(refused(format!(
                    "PutBucketPolicy names bucket {bucket}, not {own}, the name this install gives \
                     storage '{}'",
                    facts.resource_id
                )));
            }
            let document: Value = serde_json::from_str(policy)
                .map_err(|error| refused(format!("the bucket policy is not JSON: {error}")))?;
            let arn = format!("arn:{}:s3:::{bucket}", aws_partition(region));
            check_bucket_policy(&document, &arn, storage.public_read).map_err(refused)
        }
    }
}

/// Send a checked AWS write unchanged, after confirming no newer release was accepted.
pub async fn forward_aws_write(
    s3: &dyn alien_aws_clients::s3::S3Api,
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
    }
}

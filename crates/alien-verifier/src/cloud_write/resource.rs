//! Cloud writes for a resource of the signed stack that is not a workload: a bucket policy, a
//! queue's attributes and the like. They decide who can reach the install's data, so a
//! controller builds the same request it builds for an unprotected install and hands it to the
//! verifier, which checks it against the signed stack and sends it unchanged.
//!
//! The check is always the same for who may reach the resource: everything a request allows must
//! be what the signed stack declares for that resource (see [`alien_permissions::aws_bucket_policy`]),
//! never management, the verifier or the setup identity.

use std::collections::BTreeMap;

use alien_aws_clients::{
    lambda::{CreateEventSourceMappingRequest, UpdateEventSourceMappingRequest},
    s3::{NotificationConfiguration, QueueConfiguration},
};
use alien_core::{Queue, ResourceEntry, Stack, Storage};
use alien_permissions::{
    aws_bucket_policy::{check_bucket_policy, check_public_access_block},
    aws_queue_policy::check_queue_policy,
    aws_resource_policies,
    gcp_bucket_iam::{check_bucket_iam, check_public_access_prevention},
};
use alien_signing::SignedStatement;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::refused;
use crate::error::Result;
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

/// Basic credentials for a private registry. Never printed.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryCredentials {
    pub username: String,
    pub token: String,
}

impl std::fmt::Debug for RegistryCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryCredentials")
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .finish()
    }
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
    /// S3 PutBucketNotificationConfiguration: the bucket's whole notification document.
    #[serde(rename_all = "camelCase")]
    PutBucketNotification {
        bucket: String,
        configuration: Value,
    },
    /// S3 PutBucketNotificationConfiguration of the signed storage `storage`'s bucket, written
    /// for the worker the write is about: the document may add only that worker's own function
    /// entry for its signed storage trigger.
    #[serde(rename_all = "camelCase")]
    PutWorkerBucketNotification {
        storage: String,
        bucket: String,
        configuration: Value,
    },
    /// EventBridge PutTargets of one of a worker's schedule rules: the only target is the
    /// worker's own function.
    #[serde(rename_all = "camelCase")]
    PutScheduleTargets { rule: String, targets: Value },
    /// Lambda CreateEventSourceMapping, with the request as the cloud client serialises it.
    #[serde(rename_all = "camelCase")]
    CreateEventSourceMapping { request: Value },
    /// Lambda UpdateEventSourceMapping of the mapping `uuid`.
    #[serde(rename_all = "camelCase")]
    UpdateEventSourceMapping { uuid: String, request: Value },
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
    /// Cloud Storage setIamPolicy of a bucket: the bucket's whole policy, as the client sends it.
    #[serde(rename_all = "camelCase")]
    SetBucketIamPolicy { bucket: String, policy: Value },
    /// Cloud Storage patch of a bucket's public access prevention (`enforced` or `inherited`).
    #[serde(rename_all = "camelCase")]
    SetPublicAccessPrevention { bucket: String, value: String },
    /// Pub/Sub create topic, with the topic as the client sends it (no fields).
    #[serde(rename_all = "camelCase")]
    CreateTopic { topic: String, body: Value },
    /// Pub/Sub create subscription, with the subscription as the client sends it (its topic only).
    #[serde(rename_all = "camelCase")]
    CreateSubscription { subscription: String, body: Value },
    /// Pub/Sub create of a worker's commands push subscription: it delivers to the worker's own
    /// Cloud Run service, as the worker's own service account when the worker is private.
    #[serde(rename_all = "camelCase")]
    CreateCommandSubscription { subscription: String, body: Value },
    /// Cloud Scheduler create of one of a worker's schedule jobs: it calls the worker's own Cloud
    /// Run service on the signed cron, as the worker's own service account.
    #[serde(rename_all = "camelCase")]
    CreateSchedulerJob { job: String, body: Value },
    /// Pub/Sub setIamPolicy of a topic: its whole policy.
    #[serde(rename_all = "camelCase")]
    SetTopicIamPolicy { topic: String, policy: Value },
    /// Pub/Sub setIamPolicy of a subscription: its whole policy.
    #[serde(rename_all = "camelCase")]
    SetSubscriptionIamPolicy { subscription: String, policy: Value },
    /// Azure create (PUT) of a blob container in the install's storage account, with the
    /// container as the client sends it.
    #[serde(rename_all = "camelCase")]
    CreateBlobContainer {
        account: String,
        container: String,
        body: Value,
    },
    /// Azure update (PATCH) of a blob container in the install's storage account.
    #[serde(rename_all = "camelCase")]
    UpdateBlobContainer {
        account: String,
        container: String,
        body: Value,
    },
    /// Azure create or update (PUT) of an Event Grid subscription on the install's storage
    /// account: it delivers one notification rule of the signed storage to its queue.
    #[serde(rename_all = "camelCase")]
    PutEventSubscription {
        account: String,
        name: String,
        body: Value,
    },
    /// Azure sandbox data plane: build a disk image from a registry image in the install's
    /// sandbox group for the signed sandbox. The image must be the signed one.
    #[serde(rename_all = "camelCase")]
    CreateSandboxDiskImage {
        group: String,
        base: String,
        labels: std::collections::BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry_credentials: Option<RegistryCredentials>,
    },
    /// Azure sandbox data plane: create a sandbox in the install's sandbox group, as the client
    /// sends it. Its image, size, egress and lifecycle must be what the signed sandbox declares;
    /// only the caller's environment variables are free.
    #[serde(rename_all = "camelCase")]
    CreateSandbox {
        group: String,
        base: String,
        body: Value,
    },
    /// Writes the bundle the signed sandbox's MicroVM image is built from, into the install's
    /// bundle bucket. The verifier renders it from `image`, which must be the signed one.
    #[serde(rename_all = "camelCase")]
    PutSandboxBundle { image: String },
    /// Lambda CreateMicrovmImage for the signed sandbox, as the client serialises it.
    #[serde(rename_all = "camelCase")]
    CreateMicrovmImage { request: Value },
    /// Lambda UpdateMicrovmImage: a new version of the signed sandbox's image.
    #[serde(rename_all = "camelCase")]
    UpdateMicrovmImage { image: String, request: Value },
    /// Agent Platform create reasoning engine for the signed sandbox: the engine its templates
    /// and sessions live under. Only the display name is sent.
    #[serde(rename_all = "camelCase")]
    CreateSandboxEngine { display_name: String },
    /// Agent Platform create sandbox template under `engine`, with the template as the client
    /// sends it. The image a sandbox boots is part of it, so it is the signed sandbox's.
    #[serde(rename_all = "camelCase")]
    CreateSandboxTemplate { engine: String, body: Value },
}

impl ResourceWrite {
    /// The cloud operation, for messages.
    pub fn operation(&self) -> &'static str {
        match self {
            ResourceWrite::PutBucketPolicy { .. } => "PutBucketPolicy",
            ResourceWrite::PutPublicAccessBlock { .. } => "PutPublicAccessBlock",
            ResourceWrite::PutBucketNotification { .. } => "PutBucketNotification",
            ResourceWrite::PutWorkerBucketNotification { .. } => "PutWorkerBucketNotification",
            ResourceWrite::PutScheduleTargets { .. } => "PutScheduleTargets",
            ResourceWrite::CreateEventSourceMapping { .. } => "CreateEventSourceMapping",
            ResourceWrite::UpdateEventSourceMapping { .. } => "UpdateEventSourceMapping",
            ResourceWrite::CreateQueue { .. } => "CreateQueue",
            ResourceWrite::SetQueueAttributes { .. } => "SetQueueAttributes",
            ResourceWrite::SetBucketIamPolicy { .. } => "SetBucketIamPolicy",
            ResourceWrite::SetPublicAccessPrevention { .. } => "SetPublicAccessPrevention",
            ResourceWrite::CreateTopic { .. } => "CreateTopic",
            ResourceWrite::CreateSubscription { .. } => "CreateSubscription",
            ResourceWrite::CreateCommandSubscription { .. } => "CreateCommandSubscription",
            ResourceWrite::CreateSchedulerJob { .. } => "CreateSchedulerJob",
            ResourceWrite::SetTopicIamPolicy { .. } => "SetTopicIamPolicy",
            ResourceWrite::SetSubscriptionIamPolicy { .. } => "SetSubscriptionIamPolicy",
            ResourceWrite::CreateBlobContainer { .. } => "CreateBlobContainer",
            ResourceWrite::PutEventSubscription { .. } => "PutEventSubscription",
            ResourceWrite::UpdateBlobContainer { .. } => "UpdateBlobContainer",
            ResourceWrite::CreateSandboxDiskImage { .. } => "CreateSandboxDiskImage",
            ResourceWrite::CreateSandbox { .. } => "CreateSandbox",
            ResourceWrite::CreateSandboxEngine { .. } => "CreateSandboxEngine",
            ResourceWrite::CreateSandboxTemplate { .. } => "CreateSandboxTemplate",
            ResourceWrite::PutSandboxBundle { .. } => "PutSandboxBundle",
            ResourceWrite::CreateMicrovmImage { .. } => "CreateMicrovmImage",
            ResourceWrite::UpdateMicrovmImage { .. } => "UpdateMicrovmImage",
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
    /// The resource, which must be a sandbox of the signed stack.
    pub fn sandbox(&self) -> Result<&alien_core::Sandbox> {
        self.entry()?
            .config
            .downcast_ref::<alien_core::Sandbox>()
            .ok_or_else(|| {
                refused(format!(
                    "resource '{}' is not a sandbox in the signed stack",
                    self.resource_id
                ))
            })
    }

    /// The resource, which must be a postgres database of the signed stack.
    pub fn postgres(&self) -> Result<&alien_core::Postgres> {
        self.entry()?
            .config
            .downcast_ref::<alien_core::Postgres>()
            .ok_or_else(|| {
                refused(format!(
                    "resource '{}' is not a postgres database in the signed stack",
                    self.resource_id
                ))
            })
    }

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
        ResourceWrite::PutBucketNotification {
            bucket,
            configuration,
        } => {
            own_bucket(facts, bucket, "PutBucketNotification")?;
            notification_document(configuration)?;
            Ok(())
        }
        ResourceWrite::PutWorkerBucketNotification {
            storage,
            bucket,
            configuration,
        } => {
            worker_trigger_storage(facts, storage)?;
            let own =
                aws_resource_policies::aws_prefixed_resource_name(&facts.place.prefix, storage);
            if *bucket != own {
                return Err(refused(format!(
                    "PutWorkerBucketNotification names bucket {bucket}, not {own}, the bucket of \
                     storage '{storage}'"
                )));
            }
            notification_document(configuration)?;
            Ok(())
        }
        ResourceWrite::PutScheduleTargets { rule, targets } => {
            check_schedule_targets(facts, rule, targets)
        }
        ResourceWrite::CreateEventSourceMapping { request } => {
            let request: CreateEventSourceMappingRequest = parse_exact(request)?;
            check_function_name(facts, &request.function_name, "CreateEventSourceMapping")?;
            let (account, region) = aws_account_and_region(facts)?;
            let allowed = worker_queue_arns(facts, &region, &account)?;
            if !allowed.contains(&request.event_source_arn) {
                return Err(refused(format!(
                    "CreateEventSourceMapping reads from {}, which is not a queue the signed \
                     stack triggers worker '{}' from",
                    request.event_source_arn, facts.resource_id
                )));
            }
            Ok(())
        }
        ResourceWrite::UpdateEventSourceMapping { request, .. } => {
            let request: UpdateEventSourceMappingRequest = parse_exact(request)?;
            facts.worker()?;
            // A mapping's source can't change; what it delivers to can.
            if let Some(name) = &request.function_name {
                check_function_name(facts, name, "UpdateEventSourceMapping")?;
            }
            Ok(())
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
        ResourceWrite::PutSandboxBundle { image } => {
            super::sandbox_image::check_bundle(&super::sandbox_image::Signed::of(facts)?, image)?;
            Ok(())
        }
        ResourceWrite::CreateMicrovmImage { request } => {
            let (account, region) = aws_account_and_region(facts)?;
            super::sandbox_image::check_create(
                &super::sandbox_image::Signed::of(facts)?,
                &region,
                &account,
                request,
            )?;
            Ok(())
        }
        ResourceWrite::UpdateMicrovmImage { image, request } => {
            let (account, region) = aws_account_and_region(facts)?;
            super::sandbox_image::check_update(
                &super::sandbox_image::Signed::of(facts)?,
                &region,
                &account,
                image,
                request,
            )?;
            Ok(())
        }
        ResourceWrite::SetBucketIamPolicy { .. }
        | ResourceWrite::SetPublicAccessPrevention { .. }
        | ResourceWrite::CreateTopic { .. }
        | ResourceWrite::CreateSubscription { .. }
        | ResourceWrite::CreateCommandSubscription { .. }
        | ResourceWrite::CreateSchedulerJob { .. }
        | ResourceWrite::SetTopicIamPolicy { .. }
        | ResourceWrite::SetSubscriptionIamPolicy { .. }
        | ResourceWrite::CreateBlobContainer { .. }
        | ResourceWrite::UpdateBlobContainer { .. }
        | ResourceWrite::PutEventSubscription { .. }
        | ResourceWrite::CreateSandboxDiskImage { .. }
        | ResourceWrite::CreateSandbox { .. }
        | ResourceWrite::CreateSandboxEngine { .. }
        | ResourceWrite::CreateSandboxTemplate { .. } => {
            Err(refused(format!("AWS takes no {} write", write.operation())))
        }
    }
}

/// Parse a request the way the verifier will send it, refusing fields it would drop.
fn parse_exact<T: serde::de::DeserializeOwned + Serialize>(request: &Value) -> Result<T> {
    let parsed: T = serde_json::from_value(request.clone())
        .map_err(|error| refused(format!("the request does not parse: {error}")))?;
    let sent = serde_json::to_value(&parsed)
        .map_err(|error| refused(format!("the request does not serialize: {error}")))?;
    if sent != *request {
        return Err(refused(
            "the request has fields the verifier does not apply".to_string(),
        ));
    }
    Ok(parsed)
}

/// A schedule rule is `<function>-cron-<index>` for a signed Schedule trigger of this worker, and
/// its one target is the worker's own function.
fn check_schedule_targets(facts: &ResourceFacts<'_>, rule: &str, targets: &Value) -> Result<()> {
    let worker = facts.worker()?;
    let own = facts.place.function_name(facts.resource_id)?;
    let index = rule
        .strip_prefix(&format!("{own}-cron-"))
        .and_then(|index| index.parse::<usize>().ok())
        .filter(|index| index.to_string() == rule[own.len() + 6..])
        .ok_or_else(|| {
            refused(format!(
                "PutScheduleTargets names rule {rule}, not a {own}-cron-<index> rule"
            ))
        })?;
    if !matches!(
        worker.triggers.get(index),
        Some(alien_core::WorkerTrigger::Schedule { .. })
    ) {
        return Err(refused(format!(
            "the signed stack has no schedule trigger {index} on worker '{}'",
            facts.resource_id
        )));
    }
    let (account, region) = aws_account_and_region(facts)?;
    let own_arn = format!(
        "arn:{}:lambda:{region}:{account}:function:{own}",
        aws_partition(&region)
    );
    let expected = json!([{ "Id": "1", "Arn": own_arn }]);
    if *targets != expected {
        return Err(refused(format!(
            "PutScheduleTargets must have the single target {own_arn} with id 1, got {targets}"
        )));
    }
    Ok(())
}

fn check_function_name(facts: &ResourceFacts<'_>, named: &str, operation: &str) -> Result<()> {
    facts.worker()?;
    let own = facts.place.function_name(facts.resource_id)?;
    // A function is named by its name or by its ARN.
    let own_arn = aws_account_and_region(facts)
        .map(|(account, region)| {
            format!(
                "arn:{}:lambda:{region}:{account}:function:{own}",
                aws_partition(&region)
            )
        })
        .ok();
    if named != own && Some(named) != own_arn.as_deref() {
        return Err(refused(format!(
            "{operation} names function {named}, not {own}, the name this install gives worker '{}'",
            facts.resource_id
        )));
    }
    Ok(())
}

/// The ARNs of the queues the signed worker is triggered from.
fn worker_queue_arns(
    facts: &ResourceFacts<'_>,
    region: &str,
    account: &str,
) -> Result<Vec<String>> {
    let partition = aws_partition(region);
    Ok(facts
        .worker()?
        .triggers
        .iter()
        .filter_map(|trigger| match trigger {
            alien_core::WorkerTrigger::Queue { queue, .. } => Some(format!(
                "arn:{partition}:sqs:{region}:{account}:{}",
                aws_resource_policies::aws_prefixed_resource_name(&facts.place.prefix, &queue.id)
            )),
            _ => None,
        })
        .collect())
}

impl ResourceFacts<'_> {
    /// The resource, which must be a worker of the signed stack.
    pub fn worker(&self) -> Result<&alien_core::Worker> {
        self.entry()?
            .config
            .downcast_ref::<alien_core::Worker>()
            .ok_or_else(|| {
                refused(format!(
                    "resource '{}' is not a worker in the signed stack",
                    self.resource_id
                ))
            })
    }
}

fn notification_document(configuration: &Value) -> Result<NotificationConfiguration> {
    serde_json::from_value(configuration.clone()).map_err(|error| {
        refused(format!(
            "the bucket notification document does not parse: {error}"
        ))
    })
}

/// The queue entries the signed stack declares for its storage `facts.resource_id`.
fn desired_queue_entries(
    facts: &ResourceFacts<'_>,
    region: &str,
    account: &str,
) -> Result<Vec<QueueConfiguration>> {
    let storage = facts.storage()?;
    let partition = aws_partition(region);
    Ok(storage
        .notifications
        .iter()
        .enumerate()
        .map(|(index, notification)| {
            QueueConfiguration::for_rule(
                notification,
                format!(
                    "arn:{partition}:sqs:{region}:{account}:{}",
                    aws_resource_policies::aws_prefixed_resource_name(
                        &facts.place.prefix,
                        &notification.queue.id
                    )
                ),
                Some(alien_core::storage_notifications::aws_notification_id(
                    &facts.place.prefix,
                    facts.resource_id,
                    index,
                )),
            )
        })
        .collect())
}

/// A bucket's notification document sends the bucket's events to destinations, so it decides who
/// receives the data: every entry it holds must be one the bucket already has, or one of the
/// storage's own the signed stack declares. Reads the bucket's current document to tell.
pub async fn check_notification_state(
    lambda: &dyn alien_aws_clients::lambda::LambdaApi,
    s3: &dyn alien_aws_clients::s3::S3Api,
    facts: &ResourceFacts<'_>,
    write: &ResourceWrite,
) -> Result<()> {
    if let ResourceWrite::UpdateEventSourceMapping { uuid, .. } = write {
        return check_mapping_is_own(lambda, facts, uuid).await;
    }
    let (bucket, configuration, desired, desired_functions) = match write {
        ResourceWrite::PutBucketNotification {
            bucket,
            configuration,
        } => {
            let (account, region) = aws_account_and_region(facts)?;
            (
                bucket,
                configuration,
                desired_queue_entries(facts, &region, &account)?,
                Vec::new(),
            )
        }
        ResourceWrite::PutWorkerBucketNotification {
            storage,
            bucket,
            configuration,
        } => {
            let (account, region) = aws_account_and_region(facts)?;
            let events = worker_trigger_storage(facts, storage)?;
            let function = facts.place.function_name(facts.resource_id)?;
            let entry = alien_aws_clients::s3::LambdaFunctionConfiguration {
                id: Some(
                    alien_core::storage_notifications::aws_trigger_notification_id(
                        &function, storage,
                    ),
                ),
                lambda_function_arn: format!(
                    "arn:{}:lambda:{region}:{account}:function:{function}",
                    aws_partition(&region)
                ),
                events,
                filter: None,
            };
            (bucket, configuration, Vec::new(), vec![entry])
        }
        _ => return Ok(()),
    };
    use alien_error::Context;
    let current = s3
        .get_bucket_notification_configuration(bucket)
        .await
        .context(crate::error::ErrorData::CloudCallFailed {
            operation: format!("s3:GetBucketNotification {bucket}"),
        })?;
    check_notification_document(
        &notification_document(configuration)?,
        &current,
        &desired,
        &desired_functions,
        facts.resource_id,
    )
}

/// The events of the signed worker's storage trigger on `storage`; refused if it has none.
fn worker_trigger_storage(facts: &ResourceFacts<'_>, storage: &str) -> Result<Vec<String>> {
    facts
        .worker()?
        .triggers
        .iter()
        .find_map(|trigger| match trigger {
            alien_core::WorkerTrigger::Storage {
                storage: reference,
                events,
            } if reference.id == storage => Some(
                alien_core::storage_notifications::aws_trigger_events(events),
            ),
            _ => None,
        })
        .ok_or_else(|| {
            refused(format!(
                "the signed stack has no storage trigger of worker '{}' on '{storage}'",
                facts.resource_id
            ))
        })
}

/// An update may only change a mapping that already reads from a queue the signed stack triggers
/// this worker from and feeds this worker's own function, so it can't touch another worker's.
pub(crate) async fn check_mapping_is_own(
    lambda: &dyn alien_aws_clients::lambda::LambdaApi,
    facts: &ResourceFacts<'_>,
    uuid: &str,
) -> Result<()> {
    use alien_error::Context;
    let (account, region) = aws_account_and_region(facts)?;
    let mapping = lambda.get_event_source_mapping(uuid).await.context(
        crate::error::ErrorData::CloudCallFailed {
            operation: format!("lambda:GetEventSourceMapping {uuid}"),
        },
    )?;
    let source = mapping.event_source_arn.unwrap_or_default();
    if !worker_queue_arns(facts, &region, &account)?.contains(&source) {
        return Err(refused(format!(
            "mapping {uuid} reads from {source}, not a queue the signed stack triggers worker '{}' from",
            facts.resource_id
        )));
    }
    let own = facts.place.function_name(facts.resource_id)?;
    let own_arn = format!(
        "arn:{}:lambda:{region}:{account}:function:{own}",
        aws_partition(&region)
    );
    let function = mapping.function_arn.unwrap_or_default();
    if function != own_arn {
        return Err(refused(format!(
            "mapping {uuid} feeds {function}, not {own_arn}, the function of worker '{}'",
            facts.resource_id
        )));
    }
    Ok(())
}

/// The rule behind [`check_notification_state`]: no entry of `new` may be one `current` lacks
/// unless it is in `desired`.
fn check_notification_document(
    new: &NotificationConfiguration,
    current: &NotificationConfiguration,
    desired: &[QueueConfiguration],
    desired_functions: &[alien_aws_clients::s3::LambdaFunctionConfiguration],
    resource_id: &str,
) -> Result<()> {
    let allowed_queue = |entry: &QueueConfiguration| {
        current.queue_configurations.contains(entry) || desired.contains(entry)
    };
    if let Some(entry) = new.queue_configurations.iter().find(|e| !allowed_queue(e)) {
        return Err(refused(format!(
            "PutBucketNotification adds a queue destination {} that is neither already on the              bucket nor one the signed stack declares for '{}'",
            entry.queue_arn, resource_id
        )));
    }
    if let Some(entry) = new.lambda_function_configurations.iter().find(|entry| {
        !current.lambda_function_configurations.contains(entry)
            && !desired_functions.contains(entry)
    }) {
        return Err(refused(format!(
            "PutBucketNotification adds a function destination {}",
            entry.lambda_function_arn
        )));
    }
    if let Some(entry) = new
        .topic_configurations
        .iter()
        .find(|entry| !current.topic_configurations.contains(entry))
    {
        return Err(refused(format!(
            "PutBucketNotification adds a topic destination {}",
            entry.topic_arn
        )));
    }
    if new.event_bridge_configuration.is_some() && current.event_bridge_configuration.is_none() {
        return Err(refused(
            "PutBucketNotification turns on EventBridge delivery".to_string(),
        ));
    }
    Ok(())
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
    facts: &ResourceFacts<'_>,
    microvms: &dyn alien_aws_clients::lambda_microvms::LambdaMicrovmsApi,
    lambda: &dyn alien_aws_clients::lambda::LambdaApi,
    s3: &dyn alien_aws_clients::s3::S3Api,
    sqs: &dyn alien_aws_clients::sqs::SqsApi,
    eventbridge: &dyn alien_aws_clients::eventbridge::EventBridgeApi,
    write: &ResourceWrite,
) -> Result<Value> {
    use alien_error::Context;
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
        ResourceWrite::PutBucketNotification {
            bucket,
            configuration,
        }
        | ResourceWrite::PutWorkerBucketNotification {
            bucket,
            configuration,
            ..
        } => {
            let document = notification_document(configuration)?;
            s3.put_bucket_notification_configuration(bucket, &document)
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("s3:PutBucketNotification {bucket}"),
                })?;
            Ok(json!({ "bucket": bucket }))
        }
        ResourceWrite::PutScheduleTargets { rule, targets } => {
            let targets: Vec<alien_aws_clients::eventbridge::EventBridgeTarget> =
                serde_json::from_value(targets.clone())
                    .map_err(|error| refused(format!("the targets do not parse: {error}")))?;
            eventbridge
                .put_targets(alien_aws_clients::eventbridge::PutTargetsRequest {
                    rule: rule.clone(),
                    targets,
                })
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("events:PutTargets {rule}"),
                })?;
            Ok(json!({ "rule": rule }))
        }
        ResourceWrite::CreateEventSourceMapping { request } => {
            let request: CreateEventSourceMappingRequest = parse_exact(request)?;
            let function = request.function_name.clone();
            let created = lambda.create_event_source_mapping(request).await.context(
                crate::error::ErrorData::CloudCallFailed {
                    operation: format!("lambda:CreateEventSourceMapping {function}"),
                },
            )?;
            Ok(json!({ "uuid": created.uuid }))
        }
        ResourceWrite::UpdateEventSourceMapping { uuid, request } => {
            let request: UpdateEventSourceMappingRequest = parse_exact(request)?;
            lambda
                .update_event_source_mapping(uuid, request)
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("lambda:UpdateEventSourceMapping {uuid}"),
                })?;
            Ok(json!({ "uuid": uuid }))
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
        ResourceWrite::PutSandboxBundle { image } => {
            let bundle = super::sandbox_image::check_bundle(
                &super::sandbox_image::Signed::of(facts)?,
                image,
            )?;
            s3.put_object(&alien_aws_clients::s3::PutObjectRequest {
                bucket: bundle.bucket.clone(),
                key: bundle.key.clone(),
                body: bundle.zip.clone(),
                content_type: Some("application/zip".to_string()),
                content_md5: None,
                server_side_encryption: None,
                storage_class: None,
            })
            .await
            .context(crate::error::ErrorData::CloudCallFailed {
                operation: format!("s3:PutObject {}", bundle.uri()),
            })?;
            Ok(json!({ "uri": bundle.uri() }))
        }
        ResourceWrite::CreateMicrovmImage { request } => {
            let (account, region) = aws_account_and_region(facts)?;
            let derived = super::sandbox_image::check_create(
                &super::sandbox_image::Signed::of(facts)?,
                &region,
                &account,
                request,
            )?;
            verify_bundle(s3, &super::sandbox_image::Signed::of(facts)?, request).await?;
            let created = microvms.create_microvm_image(derived).await.context(
                crate::error::ErrorData::CloudCallFailed {
                    operation: "lambda:CreateMicrovmImage".to_string(),
                },
            )?;
            Ok(json!({
                "imageArn": created.image_arn,
                "imageVersion": created.image_version,
                "name": created.name,
                "state": created.state,
            }))
        }
        ResourceWrite::UpdateMicrovmImage { image, request } => {
            let (account, region) = aws_account_and_region(facts)?;
            let derived = super::sandbox_image::check_update(
                &super::sandbox_image::Signed::of(facts)?,
                &region,
                &account,
                image,
                request,
            )?;
            verify_bundle(s3, &super::sandbox_image::Signed::of(facts)?, request).await?;
            let rolled = microvms
                .update_microvm_image(image, derived)
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("lambda:UpdateMicrovmImage {image}"),
                })?;
            Ok(json!({
                "imageArn": rolled.image_arn,
                "imageVersion": rolled.image_version,
                "name": rolled.name,
                "state": rolled.state,
            }))
        }
        ResourceWrite::SetBucketIamPolicy { .. }
        | ResourceWrite::SetPublicAccessPrevention { .. }
        | ResourceWrite::CreateTopic { .. }
        | ResourceWrite::CreateSubscription { .. }
        | ResourceWrite::CreateCommandSubscription { .. }
        | ResourceWrite::CreateSchedulerJob { .. }
        | ResourceWrite::SetTopicIamPolicy { .. }
        | ResourceWrite::SetSubscriptionIamPolicy { .. }
        | ResourceWrite::CreateBlobContainer { .. }
        | ResourceWrite::UpdateBlobContainer { .. }
        | ResourceWrite::PutEventSubscription { .. }
        | ResourceWrite::CreateSandboxDiskImage { .. }
        | ResourceWrite::CreateSandbox { .. }
        | ResourceWrite::CreateSandboxEngine { .. }
        | ResourceWrite::CreateSandboxTemplate { .. } => {
            Err(refused(format!("AWS takes no {} write", write.operation())))
        }
    }
}

/// Reads the bundle a build names and refuses it unless it is the one rendered from the signed
/// image: a saved bundle of an older, since revoked image has the same prefix.
async fn verify_bundle(
    s3: &dyn alien_aws_clients::s3::S3Api,
    signed: &super::sandbox_image::Signed<'_>,
    request: &Value,
) -> Result<()> {
    use alien_error::Context;
    let (bucket, key) = super::sandbox_image::bundle_location(signed, request)?;
    let uri = format!("s3://{bucket}/{key}");
    let object = s3
        .get_object(&alien_aws_clients::s3::GetObjectRequest {
            bucket,
            key,
            version_id: None,
            range: None,
        })
        .await
        .context(crate::error::ErrorData::CloudCallFailed {
            operation: format!("s3:GetObject {uri}"),
        })?;
    super::sandbox_image::check_bundle_contents(signed, &uri, &object.body)
}

/// The bucket name this install gives storage `facts.resource_id` on GCP.
pub fn gcp_bucket_name(facts: &ResourceFacts<'_>) -> String {
    format!("{}-{}", facts.place.prefix, facts.resource_id)
}

/// Check a write for a GCP resource: it must name the resource's own cloud name, and everything
/// it grants must be what the signed stack declares.
pub fn check_gcp_write(facts: &ResourceFacts<'_>, write: &ResourceWrite) -> Result<()> {
    let own_bucket = |bucket: &str| -> Result<&Storage> {
        let storage = facts.storage()?;
        let own = gcp_bucket_name(facts);
        if bucket != own {
            return Err(refused(format!(
                "{} names bucket {bucket}, not {own}, the name this install gives storage '{}'",
                write.operation(),
                facts.resource_id
            )));
        }
        Ok(storage)
    };
    match write {
        ResourceWrite::SetBucketIamPolicy { bucket, policy } => {
            own_bucket(bucket)?;
            // What is granted is judged against the bucket's current policy in the state check;
            // here only the shape.
            if !policy.is_object() {
                return Err(refused("the bucket IAM policy is not a JSON object"));
            }
            Ok(())
        }
        ResourceWrite::SetPublicAccessPrevention { bucket, value } => {
            let storage = own_bucket(bucket)?;
            check_public_access_prevention(value, storage.public_read).map_err(refused)
        }
        ResourceWrite::CreateSandboxEngine { display_name } => {
            facts.sandbox()?;
            let own = gcp_sandbox_engine_name(facts);
            if *display_name != own {
                return Err(refused(format!(
                    "CreateSandboxEngine names {display_name}, not {own}"
                )));
            }
            Ok(())
        }
        ResourceWrite::CreateSandboxTemplate { engine, body } => {
            check_sandbox_template(facts.sandbox()?, &facts.place.prefix, engine, body)
        }
        ResourceWrite::CreateTopic { topic, body } => {
            facts.queue()?;
            own_topic(facts, topic, write.operation())?;
            if !body.as_object().is_some_and(|fields| fields.is_empty()) {
                return Err(refused(
                    "CreateTopic sets a field: a topic is created with none",
                ));
            }
            Ok(())
        }
        ResourceWrite::CreateSubscription { subscription, body } => {
            facts.queue()?;
            let topic = gcp_topic_name(facts);
            if *subscription != gcp_subscription_name(facts) {
                return Err(refused(format!(
                    "CreateSubscription names {subscription}, not {}",
                    gcp_subscription_name(facts)
                )));
            }
            let project = gcp_project(facts)?;
            let fields = body
                .as_object()
                .ok_or_else(|| refused("the subscription is not a JSON object"))?;
            if let Some(field) = fields
                .keys()
                .find(|key| !matches!(key.as_str(), "name" | "topic"))
            {
                return Err(refused(format!(
                    "CreateSubscription sets '{field}': a pull subscription has only its topic"
                )));
            }
            if fields.get("topic").and_then(Value::as_str)
                != Some(format!("projects/{project}/topics/{topic}").as_str())
            {
                return Err(refused(format!("the subscription's topic is not {topic}")));
            }
            if let Some(name) = fields.get("name") {
                if name.as_str()
                    != Some(format!("projects/{project}/subscriptions/{subscription}").as_str())
                {
                    return Err(refused("the subscription's name is not its own"));
                }
            }
            Ok(())
        }
        ResourceWrite::CreateCommandSubscription { subscription, body } => {
            check_command_subscription(facts, subscription, body, None)
        }
        ResourceWrite::CreateSchedulerJob { job, body } => {
            check_scheduler_job(facts, job, body, None, None)
        }
        ResourceWrite::SetTopicIamPolicy { topic, policy } => {
            facts.queue()?;
            own_topic(facts, topic, write.operation())?;
            if !policy.is_object() {
                return Err(refused("the topic IAM policy is not a JSON object"));
            }
            Ok(())
        }
        ResourceWrite::SetSubscriptionIamPolicy {
            subscription,
            policy,
        } => {
            facts.queue()?;
            if *subscription != gcp_subscription_name(facts) {
                return Err(refused(format!(
                    "SetSubscriptionIamPolicy names {subscription}, not {}",
                    gcp_subscription_name(facts)
                )));
            }
            if !policy.is_object() {
                return Err(refused("the subscription IAM policy is not a JSON object"));
            }
            Ok(())
        }
        other => Err(refused(format!("GCP takes no {} write", other.operation()))),
    }
}

/// The topic this install gives queue `facts.resource_id` on GCP.
pub fn gcp_topic_name(facts: &ResourceFacts<'_>) -> String {
    format!("{}-{}", facts.place.prefix, facts.resource_id)
}

/// The display name this install gives the Agent Platform engine of sandbox `facts.resource_id`.
pub fn gcp_sandbox_engine_name(facts: &ResourceFacts<'_>) -> String {
    format!(
        "{}-{}",
        facts.place.prefix,
        alien_core::GcpAgentPlatformEngine::id_for_sandbox(facts.resource_id)
    )
}

/// Check a sandbox template: the image is the signed sandbox's, the limits, egress and port are
/// what the signed sandbox declares, and nothing else is set.
pub fn check_sandbox_template(
    sandbox: &alien_core::Sandbox,
    prefix: &str,
    engine: &str,
    body: &Value,
) -> Result<()> {
    use alien_core::sandbox_image::GCP_AGENT_PLATFORM;
    // The engine id is assigned by the cloud: a bare segment is all that is known of it.
    if engine.is_empty()
        || !engine
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(refused(format!("'{engine}' is not an engine id")));
    }
    let signed_image = match &sandbox.code {
        alien_core::SandboxCode::Image { image, .. } => image,
        _ => return Err(refused("the signed sandbox has no built image")),
    };
    let signed_egress = sandbox.egress.internet_access_switch().ok_or_else(|| {
        refused("the signed sandbox's egress is not one Agent Platform expresses")
    })?;
    let limits = sandbox.resolved_limits();
    let own_name = format!("{prefix}-{}", sandbox.id());
    let template: alien_gcp_clients::gcp::agent_platform::SandboxEnvironmentTemplate =
        serde_json::from_value(body.clone())
            .map_err(|error| refused(format!("the template is not one: {error}")))?;
    if template.name.is_some() || template.state.is_some() || !template.extra.is_empty() {
        return Err(refused("the template sets a field the controller does not"));
    }
    if template.display_name.as_deref() != Some(own_name.as_str()) {
        return Err(refused(format!(
            "the template's display name is not {own_name}"
        )));
    }
    if template
        .egress_control_config
        .as_ref()
        .map(|e| (e.internet_access, e.extra.is_empty()))
        != Some((Some(signed_egress), true))
    {
        return Err(refused(format!(
            "the template's egress is not the signed internetAccess {signed_egress}"
        )));
    }
    let environment = template
        .custom_container_environment
        .ok_or_else(|| refused("the template has no custom container environment"))?;
    if !environment.extra.is_empty() {
        return Err(refused(
            "the template's environment sets a field the controller does not",
        ));
    }
    let spec = environment
        .custom_container_spec
        .ok_or_else(|| refused("the template names no image"))?;
    if !spec.extra.is_empty() {
        return Err(refused(
            "the template's container spec sets a field the controller does not",
        ));
    }
    if !sandbox_image_is_signed(signed_image, &spec.image_uri) {
        return Err(refused(format!(
            "image {} is not the signed sandbox image {signed_image}",
            spec.image_uri
        )));
    }
    let expected_ports = [(i32::from(GCP_AGENT_PLATFORM.port), Some("TCP".to_string()))];
    let ports: Vec<_> = environment
        .ports
        .iter()
        .map(|port| (port.port, port.protocol.clone()))
        .collect();
    if ports != expected_ports {
        return Err(refused(
            "the template's ports are not the agent's one TCP port",
        ));
    }
    let resources = environment.resources.unwrap_or_default();
    let expected_limits = std::collections::HashMap::from([
        ("cpu".to_string(), limits.cpu),
        ("memory".to_string(), limits.memory),
    ]);
    if resources.requests.is_some() || resources.limits != Some(expected_limits) {
        return Err(refused(
            "the template's resources are not the signed limits",
        ));
    }
    Ok(())
}

/// Whether `sent` names the signed sandbox image: the same reference, or the same path on an
/// Artifact Registry host, which is how Agent Platform is told to pull an image the release
/// pushed through the manager. Artifact Registry paths start with the registry's project, which
/// only its owner can push to.
fn sandbox_image_is_signed(signed: &str, sent: &str) -> bool {
    if signed == sent {
        return true;
    }
    match (signed.split_once('/'), sent.split_once('/')) {
        (Some((_, signed_path)), Some((sent_host, sent_path))) => {
            sent_host.ends_with("-docker.pkg.dev") && signed_path == sent_path
        }
        _ => false,
    }
}

/// The service a GCP worker runs as: the name setup gives its Cloud Run service.
pub fn gcp_worker_service_name(facts: &ResourceFacts<'_>) -> Result<String> {
    facts.place.function_name(facts.resource_id)
}

/// Check the push subscription a worker's commands arrive on. It must be the worker's own
/// (`<service>-rq-sub` on `<service>-rq`), deliver to the worker's own Cloud Run service and, for
/// a private worker, carry an OIDC token of the worker's own service account for that service.
/// `service_uri` is what Cloud Run reports for the service; without it (the shape check) the
/// endpoint is only required to be an https URL.
pub fn check_command_subscription(
    facts: &ResourceFacts<'_>,
    subscription: &str,
    body: &Value,
    service_uri: Option<&str>,
) -> Result<()> {
    check_command_subscription_of(
        facts.worker()?,
        facts.place,
        facts.resource_id,
        subscription,
        body,
        service_uri,
    )
}

fn check_command_subscription_of(
    worker: &alien_core::Worker,
    place: &Place,
    resource_id: &str,
    subscription: &str,
    body: &Value,
    service_uri: Option<&str>,
) -> Result<()> {
    if !worker.commands_enabled {
        return Err(refused(format!(
            "worker '{resource_id}' has no commands in the signed stack"
        )));
    }
    let service = place.function_name(resource_id)?;
    let own = format!("{service}-rq-sub");
    if subscription != own {
        return Err(refused(format!(
            "CreateCommandSubscription names {subscription}, not {own}"
        )));
    }
    let project = match &place.install {
        alien_signing::InstallId::Gcp { project, .. } => project.as_str(),
        _ => return Err(refused("this install is not a GCP install")),
    };
    let fields = body
        .as_object()
        .ok_or_else(|| refused("the subscription is not a JSON object"))?;
    const ALLOWED: [&str; 8] = [
        "name",
        "topic",
        "pushConfig",
        "ackDeadlineSeconds",
        "retainAckedMessages",
        "labels",
        "enableMessageOrdering",
        "detached",
    ];
    if let Some(field) = fields.keys().find(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(refused(format!(
            "CreateCommandSubscription sets '{field}': only the topic, push delivery and the \
             controller's own settings are allowed"
        )));
    }
    if fields.get("topic").and_then(Value::as_str)
        != Some(format!("projects/{project}/topics/{service}-rq").as_str())
    {
        return Err(refused(format!(
            "the subscription's topic is not {service}-rq"
        )));
    }
    if let Some(name) = fields.get("name") {
        if name.as_str() != Some(format!("projects/{project}/subscriptions/{own}").as_str()) {
            return Err(refused("the subscription's name is not its own"));
        }
    }
    if fields
        .get("retainAckedMessages")
        .is_some_and(|v| v != &json!(false))
        || fields
            .get("enableMessageOrdering")
            .is_some_and(|v| v != &json!(false))
        || fields.get("detached").is_some_and(|v| v != &json!(false))
    {
        return Err(refused(
            "the subscription retains messages, orders them or is detached",
        ));
    }
    if let Some(ack) = fields.get("ackDeadlineSeconds") {
        if !ack
            .as_i64()
            .is_some_and(|seconds| (10..=600).contains(&seconds))
        {
            return Err(refused("ackDeadlineSeconds is not between 10 and 600"));
        }
    }
    if let Some(labels) = fields.get("labels") {
        let labels = labels
            .as_object()
            .ok_or_else(|| refused("the subscription's labels are not an object"))?;
        for (key, value) in labels {
            let expected = match key.as_str() {
                "commands" => resource_id,
                "deployment" => place.prefix.as_str(),
                other => return Err(refused(format!("the subscription carries label '{other}'"))),
            };
            if value.as_str() != Some(expected) {
                return Err(refused(format!("label '{key}' is not '{expected}'")));
            }
        }
    }
    let push = fields
        .get("pushConfig")
        .and_then(Value::as_object)
        .ok_or_else(|| refused("the commands subscription has no push delivery"))?;
    if let Some(field) = push
        .keys()
        .find(|key| !matches!(key.as_str(), "pushEndpoint" | "attributes" | "oidcToken"))
    {
        return Err(refused(format!("pushConfig sets '{field}'")));
    }
    if push
        .get("attributes")
        .is_some_and(|attributes| attributes.as_object().is_none_or(|map| !map.is_empty()))
    {
        return Err(refused("pushConfig carries attributes"));
    }
    let endpoint = push
        .get("pushEndpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("pushConfig has no endpoint"))?;
    let audience = endpoint.trim_end_matches('/');
    if !endpoint.starts_with("https://") || !endpoint.ends_with('/') {
        return Err(refused(
            "the push endpoint is not an https URL ending in '/'",
        ));
    }
    if let Some(uri) = service_uri {
        if audience != uri.trim_end_matches('/') {
            return Err(refused(format!(
                "the push endpoint is not the URL of Cloud Run service {service}"
            )));
        }
    }
    let oidc = push.get("oidcToken");
    if worker.public_endpoints.is_empty() {
        let install = alien_permissions::gcp_resource_grants::GcpInstall {
            prefix: &place.prefix,
            project_id: project,
            project_number: None,
            region: "",
        };
        let account = alien_permissions::gcp_resource_grants::profile_service_account_email(
            &install,
            worker.get_permissions(),
        );
        let oidc = oidc
            .and_then(Value::as_object)
            .ok_or_else(|| refused("a private worker's subscription has no OIDC token"))?;
        if oidc
            .keys()
            .any(|key| !matches!(key.as_str(), "serviceAccountEmail" | "audience"))
        {
            return Err(refused("the OIDC token sets a field it should not"));
        }
        if oidc.get("serviceAccountEmail").and_then(Value::as_str) != Some(account.as_str()) {
            return Err(refused(format!(
                "the OIDC token is not for the worker's own service account {account}"
            )));
        }
        if oidc.get("audience").and_then(Value::as_str) != Some(audience) {
            return Err(refused("the OIDC audience is not the service URL"));
        }
    } else if oidc.is_some() {
        return Err(refused(
            "a public worker's subscription carries an OIDC token",
        ));
    }
    Ok(())
}

/// Check one Cloud Scheduler job of a worker. The job is `<service>-cron-<index>` for the trigger
/// at that index, and runs exactly the signed cron (UTC) as an HTTP POST to the worker's own Cloud
/// Run service, authenticated as the worker's own service account with the service URL as the
/// audience. It carries no body, headers or other target. `region` and `service_uri` are what the
/// verifier knows from its own place and from Cloud Run; without them (the shape check) the name's
/// location and the URL are only required to be well formed.
pub fn check_scheduler_job(
    facts: &ResourceFacts<'_>,
    job: &str,
    body: &Value,
    region: Option<&str>,
    service_uri: Option<&str>,
) -> Result<()> {
    check_scheduler_job_of(
        facts.worker()?,
        facts.place,
        facts.resource_id,
        job,
        body,
        region,
        service_uri,
    )
}

fn check_scheduler_job_of(
    worker: &alien_core::Worker,
    place: &Place,
    resource_id: &str,
    job: &str,
    body: &Value,
    region: Option<&str>,
    service_uri: Option<&str>,
) -> Result<()> {
    let service = place.function_name(resource_id)?;
    let project = match &place.install {
        alien_signing::InstallId::Gcp { project, .. } => project.as_str(),
        _ => return Err(refused("this install is not a GCP install")),
    };
    let index = job
        .strip_prefix(&format!("{service}-cron-"))
        .and_then(|rest| rest.parse::<usize>().ok())
        .filter(|index| job == format!("{service}-cron-{index}"))
        .ok_or_else(|| {
            refused(format!(
                "CreateSchedulerJob names {job}, not a {service}-cron-<index> job"
            ))
        })?;
    let cron = match worker.triggers.get(index) {
        Some(alien_core::WorkerTrigger::Schedule { cron }) => cron,
        _ => {
            return Err(refused(format!(
                "trigger {index} of worker '{}' is not a schedule in the signed stack",
                resource_id
            )))
        }
    };
    let fields = body
        .as_object()
        .ok_or_else(|| refused("the scheduler job is not a JSON object"))?;
    if let Some(field) = fields.keys().find(|key| {
        !matches!(
            key.as_str(),
            "name" | "description" | "schedule" | "timeZone" | "httpTarget"
        )
    }) {
        return Err(refused(format!(
            "CreateSchedulerJob sets '{field}': only the schedule and the call to the worker are allowed"
        )));
    }
    let name_tail = format!("/jobs/{job}");
    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("the scheduler job has no name"))?;
    let name_head = format!("projects/{project}/locations/");
    let location = name
        .strip_prefix(&name_head)
        .and_then(|rest| rest.strip_suffix(&name_tail))
        .filter(|location| !location.is_empty() && !location.contains('/'))
        .ok_or_else(|| refused("the scheduler job's name is not its own"))?;
    if region.is_some_and(|region| region != location) {
        return Err(refused("the scheduler job is not in the install's region"));
    }
    if fields.get("schedule").and_then(Value::as_str) != Some(cron.as_str()) {
        return Err(refused(
            "the scheduler job's schedule is not the signed cron",
        ));
    }
    if fields
        .get("timeZone")
        .is_some_and(|zone| zone != &json!("UTC"))
    {
        return Err(refused("the scheduler job's time zone is not UTC"));
    }
    if fields
        .get("description")
        .is_some_and(|description| !description.is_string())
    {
        return Err(refused("the scheduler job's description is not text"));
    }
    let target = fields
        .get("httpTarget")
        .and_then(Value::as_object)
        .ok_or_else(|| refused("the scheduler job has no HTTP target"))?;
    if let Some(field) = target
        .keys()
        .find(|key| !matches!(key.as_str(), "uri" | "httpMethod" | "oidcToken"))
    {
        return Err(refused(format!("the scheduler target sets '{field}'")));
    }
    if target
        .get("httpMethod")
        .is_some_and(|method| method != &json!("POST"))
    {
        return Err(refused("the scheduler target's method is not POST"));
    }
    let uri = target
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| uri.starts_with("https://"))
        .ok_or_else(|| refused("the scheduler target is not an https URL"))?;
    if let Some(service_uri) = service_uri {
        if uri != service_uri {
            return Err(refused(format!(
                "the scheduler target is not the URL of Cloud Run service {service}"
            )));
        }
    }
    let install = alien_permissions::gcp_resource_grants::GcpInstall {
        prefix: &place.prefix,
        project_id: project,
        project_number: None,
        region: "",
    };
    let account = alien_permissions::gcp_resource_grants::profile_service_account_email(
        &install,
        worker.get_permissions(),
    );
    let oidc = target
        .get("oidcToken")
        .and_then(Value::as_object)
        .ok_or_else(|| refused("the scheduler target has no OIDC token"))?;
    if oidc
        .keys()
        .any(|key| !matches!(key.as_str(), "serviceAccountEmail" | "audience"))
    {
        return Err(refused("the OIDC token sets a field it should not"));
    }
    if oidc.get("serviceAccountEmail").and_then(Value::as_str) != Some(account.as_str()) {
        return Err(refused(format!(
            "the OIDC token is not for the worker's own service account {account}"
        )));
    }
    if oidc.get("audience").and_then(Value::as_str) != Some(uri) {
        return Err(refused("the OIDC audience is not the service URL"));
    }
    Ok(())
}

/// The pull subscription this install gives the queue.
pub fn gcp_subscription_name(facts: &ResourceFacts<'_>) -> String {
    format!("{}-sub", gcp_topic_name(facts))
}

fn gcp_project<'a>(facts: &'a ResourceFacts<'_>) -> Result<&'a str> {
    match &facts.place.install {
        alien_signing::InstallId::Gcp { project, .. } => Ok(project),
        _ => Err(refused("this install is not a GCP install")),
    }
}

fn own_topic(facts: &ResourceFacts<'_>, topic: &str, operation: &str) -> Result<()> {
    let own = gcp_topic_name(facts);
    if topic != own {
        return Err(refused(format!(
            "{operation} names topic {topic}, not {own}, the name this install gives queue '{}'",
            facts.resource_id
        )));
    }
    Ok(())
}

/// Check a GCP write against `current`, the IAM policy the bucket holds now: a new policy may
/// grant only what the bucket already holds, what the signed stack grants this bucket (derived
/// from the stack and this install alone) or the public read the stack declares.
pub fn check_gcp_state(
    facts: &ResourceFacts<'_>,
    write: &ResourceWrite,
    current: &Value,
    region: &str,
    storage_agent: Option<&str>,
) -> Result<()> {
    use alien_permissions::generators::gcp_runtime::{
        GcpBindingResourceKind, GcpBindingTargetScope, GcpIamBinding,
    };
    let project = gcp_project(facts)?;
    let install = alien_permissions::gcp_resource_grants::GcpInstall {
        prefix: &facts.place.prefix,
        project_id: project,
        project_number: None,
        region,
    };
    let derive = |resource_type: &str, name: &str| {
        alien_permissions::gcp_resource_grants::gcp_resource_scoped_bindings(
            &facts.stack,
            &install,
            facts.resource_id,
            resource_type,
            name,
        )
        .map_err(|error| refused(format!("the signed stack's grants do not derive: {error}")))
    };
    match write {
        ResourceWrite::SetBucketIamPolicy { bucket, policy } => {
            let storage = facts.storage()?;
            let derived = derive("storage", bucket)?;
            check_bucket_iam(policy, current, &derived, storage.public_read).map_err(refused)
        }
        ResourceWrite::SetTopicIamPolicy { topic, policy } => {
            let mut derived: Vec<GcpIamBinding> = derive("queue", topic)?
                .into_iter()
                .filter(|binding| {
                    binding.resource_kind == Some(GcpBindingResourceKind::PubsubTopic)
                })
                .collect();
            // The one outside publisher a topic may have is the Cloud Storage service agent, and
            // only when a storage of the signed stack notifies this queue.
            if alien_core::storage_notifications::queue_receives_storage_notifications(
                &facts.stack,
                facts.resource_id,
            ) {
                if let Some(agent) = storage_agent {
                    derived.push(GcpIamBinding {
                        role: "roles/pubsub.publisher".to_string(),
                        members: vec![format!("serviceAccount:{agent}")],
                        target: GcpBindingTargetScope::CurrentResource,
                        target_resource_name: None,
                        resource_kind: Some(GcpBindingResourceKind::PubsubTopic),
                        condition: None,
                    });
                }
            }
            check_bucket_iam(policy, current, &derived, false).map_err(refused)
        }
        ResourceWrite::SetSubscriptionIamPolicy {
            subscription,
            policy,
        } => {
            let derived: Vec<GcpIamBinding> = derive("queue", &gcp_topic_name(facts))?
                .into_iter()
                .filter(|binding| {
                    binding.resource_kind == Some(GcpBindingResourceKind::PubsubSubscription)
                })
                .collect();
            let _ = subscription;
            check_bucket_iam(policy, current, &derived, false).map_err(refused)
        }
        _ => Ok(()),
    }
}

/// The name this install gives the blob container of storage `id`.
pub fn azure_container_name(prefix: &str, id: &str) -> String {
    format!("{prefix}-{id}").to_lowercase().replace('_', "-")
}

/// Check a write for an Azure resource: the container must be the storage's own and carry only
/// the public access the signed stack declares. The storage account is named by setup and the
/// verifier only reaches the ones in its own resource group, but the account is still bound to the
/// one setup assigned (`azure_storage_account_name`), so a same-named container elsewhere is refused.
pub fn check_azure_write(facts: &ResourceFacts<'_>, write: &ResourceWrite) -> Result<()> {
    let (account, container, body) = match write {
        ResourceWrite::CreateBlobContainer {
            account,
            container,
            body,
        }
        | ResourceWrite::UpdateBlobContainer {
            account,
            container,
            body,
        } => (account, container, body),
        ResourceWrite::PutEventSubscription {
            account,
            name,
            body,
        } => return check_event_subscription(facts, account, name, body),
        ResourceWrite::CreateSandbox { group, base, body } => {
            return check_sandbox_create(
                facts.sandbox()?,
                &facts.place.prefix,
                group,
                base,
                body,
                None,
            )
        }
        ResourceWrite::CreateSandboxDiskImage {
            group,
            base,
            labels,
            ..
        } => {
            return check_sandbox_disk_image(
                facts.sandbox()?,
                &facts.place.prefix,
                group,
                base,
                labels,
            )
        }
        other => {
            return Err(refused(format!(
                "Azure takes no {} write",
                other.operation()
            )));
        }
    };
    let storage = facts.storage()?;
    let own = azure_container_name(&facts.place.prefix, facts.resource_id);
    if *container != own {
        return Err(refused(format!(
            "{} names container {container}, not {own}, the name this install gives storage '{}'",
            write.operation(),
            facts.resource_id
        )));
    }
    check_account_name(facts, write.operation(), account)?;
    check_blob_container_body(body, storage.public_read).map_err(refused)
}

/// The sandbox group setup gives sandbox `id`: its Terraform name, `lower(<prefix>-<id>)` with
/// underscores as hyphens.
pub fn azure_sandbox_group_name(prefix: &str, id: &str) -> String {
    format!("{prefix}-{id}").to_lowercase().replace('_', "-")
}

/// A disk image build must name the install's own sandbox group and a base image that is the
/// signed sandbox's image: the same digest, wherever the carrying registry is. The only label is
/// the one that finds the image again.
pub fn check_sandbox_disk_image(
    sandbox: &alien_core::Sandbox,
    prefix: &str,
    group: &str,
    base: &str,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    use alien_core::{azure_disk_image_label, release_statement, AZURE_DISK_IMAGE_LABEL};
    let id = sandbox.id();
    let own = azure_sandbox_group_name(prefix, id);
    if group != own {
        return Err(refused(format!(
            "CreateSandboxDiskImage names sandbox group {group}, not {own}, the group this \
             install gives sandbox '{}'",
            id
        )));
    }
    let signed = release_statement::sandbox_pinned_image(id, sandbox)
        .map_err(|error| refused(error.message))?;
    let pinned = release_statement::image_digest(signed);
    if pinned.is_none() || release_statement::image_digest(base) != pinned {
        return Err(refused(format!(
            "CreateSandboxDiskImage builds from {base}, which is not the image {signed} the \
             release signed for sandbox '{}'",
            id
        )));
    }
    let expected = std::collections::BTreeMap::from([(
        AZURE_DISK_IMAGE_LABEL.to_string(),
        azure_disk_image_label(base),
    )]);
    if *labels != expected {
        return Err(refused(
            "CreateSandboxDiskImage carries labels other than the one that finds the image again"
                .to_string(),
        ));
    }
    Ok(())
}

/// A sandbox create must be the one the signed sandbox describes: it starts from the disk image
/// built from `base`, a reference with the signed digest (`image_id`, which the state check finds
/// in the install's own group by the label of `base`), at the signed size, under the signed egress policy and idle behaviour, with no group
/// network connection. The caller's environment variables are free: they go to code that holds no
/// cloud identity. Labels are limited to the two the provider sets itself. The body is rebuilt
/// with the client's own function and must equal the request, so nothing else can ride along.
pub fn check_sandbox_create(
    sandbox: &alien_core::Sandbox,
    prefix: &str,
    group: &str,
    base: &str,
    body: &Value,
    image_id: Option<&str>,
) -> Result<()> {
    use alien_azure_clients::azure::sandbox_data_plane::{
        create_body, egress_policy, CreateSandbox, AGENT_TRANSPORT_LABEL, AGENT_TRANSPORT_VERSION,
        DEFAULT_CPU, DEFAULT_MEMORY,
    };
    let id = sandbox.id();
    let own = azure_sandbox_group_name(prefix, id);
    if group != own {
        return Err(refused(format!(
            "CreateSandbox names sandbox group {group}, not {own}, the group this install gives \
             sandbox '{id}'"
        )));
    }
    let signed = alien_core::release_statement::sandbox_pinned_image(id, sandbox)
        .map_err(|error| refused(error.message))?;
    let pinned = alien_core::release_statement::image_digest(signed);
    if pinned.is_none() || alien_core::release_statement::image_digest(base) != pinned {
        return Err(refused(format!(
            "CreateSandbox starts from {base}, which is not the image {signed} the release \
             signed for sandbox '{id}'"
        )));
    }
    let fields = body
        .as_object()
        .ok_or_else(|| refused("the sandbox create is not a JSON object"))?;
    let string_map = |field: &str| -> Result<std::collections::BTreeMap<String, String>> {
        let Some(value) = fields.get(field) else {
            return Ok(Default::default());
        };
        value
            .as_object()
            .and_then(|map| {
                map.iter()
                    .map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                    .collect()
            })
            .ok_or_else(|| refused(format!("the sandbox's {field} is not a map of text")))
    };
    let environment = string_map("environment")?;
    let labels = string_map("labels")?;
    for (key, value) in &labels {
        let allowed = (key == alien_core::SANDBOX_EXPIRY_LABEL && value.parse::<u64>().is_ok())
            || (key == AGENT_TRANSPORT_LABEL && value == AGENT_TRANSPORT_VERSION);
        if !allowed {
            return Err(refused(format!("the sandbox carries label '{key}'")));
        }
    }
    let disk_image_id = fields
        .get("sourcesRef")
        .and_then(|source| source["diskImage"]["id"].as_str())
        .ok_or_else(|| refused("the sandbox does not start from a built disk image"))?
        .to_string();
    if image_id.is_some_and(|image| image != disk_image_id) {
        return Err(refused(
            "the sandbox starts from a disk image other than the one built from the signed image",
        ));
    }
    let limits = sandbox.limits.as_ref();
    let expected = create_body(&CreateSandbox {
        disk_image: String::new(),
        disk_image_id: Some(disk_image_id),
        cpu: limits.map_or_else(|| DEFAULT_CPU.to_string(), |l| l.cpu.clone()),
        memory: limits.map_or_else(|| DEFAULT_MEMORY.to_string(), |l| l.memory.clone()),
        disk: limits.map(|l| l.disk.clone()),
        environment,
        egress: egress_policy(&sandbox.egress),
        customer_vnet_connection_name: None,
        idle_pause_seconds: sandbox.lifecycle.idle_pause_seconds,
        labels,
    });
    if &expected != body {
        return Err(refused(format!(
            "the sandbox create differs from what the signed sandbox '{id}' declares: its image, \
             size, egress, lifecycle or another field is not the signed one"
        )));
    }
    Ok(())
}

/// The storage account setup gives an install: Terraform's
/// `substr(replace(lower("${local.resource_prefix}default"), "-", ""), 0, 24)`
/// (`storage_account_name_local`).
pub fn azure_storage_account_name(prefix: &str) -> String {
    format!("{prefix}default")
        .to_lowercase()
        .replace('-', "")
        .chars()
        .take(24)
        .collect()
}

/// A write may only name the one storage account setup assigned this install. A container of the
/// same name in any other account of the resource group is not this storage.
fn check_account_name(facts: &ResourceFacts<'_>, operation: &str, account: &str) -> Result<()> {
    let own = azure_storage_account_name(&facts.place.prefix);
    if account == own {
        Ok(())
    } else {
        Err(refused(format!(
            "{operation} names storage account '{account}', not {own}, the account setup assigned this install"
        )))
    }
}

/// An Event Grid subscription on the install's storage account must be the one the signed
/// storage declares for one of its notification rules: its name is the rule's, it delivers
/// only blob-created events of this storage's container and the rule's prefix and suffix, to
/// the Service Bus queue of the rule's queue in this install's resource group, and it sets
/// nothing else. Where a storage account's events go is who can read them.
fn check_event_subscription(
    facts: &ResourceFacts<'_>,
    account: &str,
    name: &str,
    body: &Value,
) -> Result<()> {
    let operation = "PutEventSubscription";
    check_account_name(facts, operation, account)?;
    let alien_signing::InstallId::Azure {
        subscription,
        resource_group,
    } = &facts.place.install
    else {
        return Err(refused(format!("{operation} is for an Azure install")));
    };
    let storage = facts.storage()?;
    let container = azure_container_name(&facts.place.prefix, facts.resource_id);
    let rule = storage
        .notifications
        .iter()
        .find(|rule| {
            alien_core::storage_notifications::azure_subscription_name(&container, rule) == name
        })
        .ok_or_else(|| {
            refused(format!(
                "{operation} names subscription {name}, which is not a notification rule of storage '{}' in the signed stack",
                facts.resource_id
            ))
        })?;
    let queue = format!("{}-{}", facts.place.prefix, rule.queue.id);
    let queue_prefix = format!(
        "/subscriptions/{subscription}/resourceGroups/{resource_group}/providers/Microsoft.ServiceBus/namespaces/"
    );
    let destination = body
        .pointer("/properties/destination/properties/resourceId")
        .and_then(Value::as_str)
        .ok_or_else(|| refused(format!("{operation} has no destination queue")))?;
    let namespace = destination
        .strip_prefix(&queue_prefix)
        .and_then(|rest| rest.strip_suffix(&format!("/queues/{queue}")))
        .filter(|namespace| !namespace.is_empty() && !namespace.contains('/'));
    if namespace.is_none() {
        return Err(refused(format!(
            "{operation} delivers to {destination}, not queue {queue} in this install's Service Bus"
        )));
    }
    let mut filter = json!({
        "includedEventTypes": ["Microsoft.Storage.BlobCreated"],
        "subjectBeginsWith": format!(
            "/blobServices/default/containers/{container}/blobs/{}",
            rule.prefix.as_deref().unwrap_or_default()
        ),
        "isSubjectCaseSensitive": true,
    });
    if let Some(suffix) = &rule.suffix {
        filter["subjectEndsWith"] = json!(suffix);
    }
    let expected = json!({"properties": {
        "destination": {
            "endpointType": "ServiceBusQueue",
            "properties": {"resourceId": destination},
        },
        "filter": filter,
        "eventDeliverySchema": "CloudEventSchemaV1_0",
    }});
    if *body == expected {
        Ok(())
    } else {
        Err(refused(format!(
            "{operation} for {name} is not what the signed stack declares for that rule: expected {expected}"
        )))
    }
}

/// A blob container's fields: only `properties.publicAccess`, `Blob` when the signed storage is
/// public and `None` otherwise (an absent value is `None`).
pub fn check_blob_container_body(
    body: &Value,
    public_read: bool,
) -> std::result::Result<(), String> {
    let object = body
        .as_object()
        .ok_or("the blob container is not a JSON object")?;
    if let Some(field) = object.keys().find(|key| key.as_str() != "properties") {
        return Err(format!(
            "the blob container sets '{field}'; only its properties may be set"
        ));
    }
    let properties = match object.get("properties") {
        None | Some(Value::Null) => return check_public_access(None, public_read),
        Some(Value::Object(properties)) => properties,
        Some(_) => return Err("the blob container's properties are not an object".to_string()),
    };
    if let Some(field) = properties.keys().find(|key| key.as_str() != "publicAccess") {
        return Err(format!(
            "the blob container sets properties.{field}; only publicAccess may be set"
        ));
    }
    check_public_access(properties.get("publicAccess"), public_read)
}

fn check_public_access(
    value: Option<&Value>,
    public_read: bool,
) -> std::result::Result<(), String> {
    let access = match value {
        None | Some(Value::Null) => "None",
        Some(Value::String(access)) => access.as_str(),
        Some(_) => return Err("publicAccess is not a string".to_string()),
    };
    let allowed = if public_read { "Blob" } else { "None" };
    if access == allowed {
        Ok(())
    } else {
        Err(format!(
            "publicAccess '{access}' is not '{allowed}', what the signed stack declares for this storage"
        ))
    }
}

/// Send a checked Azure write unchanged.
pub async fn forward_azure_write(
    azure: &crate::azure::Azure,
    write: &ResourceWrite,
) -> Result<Value> {
    match write {
        ResourceWrite::CreateBlobContainer {
            account,
            container,
            body,
        } => {
            azure
                .write_blob_container(reqwest::Method::PUT, account, container, body)
                .await?;
            Ok(json!({ "container": container }))
        }
        ResourceWrite::UpdateBlobContainer {
            account,
            container,
            body,
        } => {
            azure
                .write_blob_container(reqwest::Method::PATCH, account, container, body)
                .await?;
            Ok(json!({ "container": container }))
        }
        ResourceWrite::PutEventSubscription {
            account,
            name,
            body,
        } => {
            azure.write_event_subscription(account, name, body).await?;
            Ok(json!({ "subscription": name }))
        }
        ResourceWrite::CreateSandboxDiskImage {
            group,
            base,
            labels,
            registry_credentials,
        } => {
            let body = alien_azure_clients::azure::sandbox_data_plane::disk_image_body(
                &alien_azure_clients::azure::sandbox_data_plane::CreateDiskImage {
                    base: base.clone(),
                    labels: labels.clone(),
                    registry_credentials: registry_credentials.as_ref().map(|credentials| {
                        (credentials.username.clone(), credentials.token.clone())
                    }),
                },
            );
            azure.create_disk_image(group, &body).await
        }
        ResourceWrite::CreateSandbox { group, body, .. } => azure.create_sandbox(group, body).await,
        other => Err(refused(format!(
            "Azure takes no {} write",
            other.operation()
        ))),
    }
}

/// Send a checked GCP write unchanged.
pub async fn forward_gcp_write(gcp: &crate::gcp::Gcp, write: &ResourceWrite) -> Result<Value> {
    match write {
        ResourceWrite::SetBucketIamPolicy { bucket, policy } => {
            gcp.set_bucket_iam_policy(bucket, policy).await?;
            Ok(json!({ "bucket": bucket }))
        }
        ResourceWrite::SetPublicAccessPrevention { bucket, value } => {
            gcp.set_public_access_prevention(bucket, value).await?;
            Ok(json!({ "bucket": bucket }))
        }
        ResourceWrite::CreateSandboxEngine { display_name } => {
            gcp.create_agent_platform("reasoningEngines", &json!({ "displayName": display_name }))
                .await
        }
        ResourceWrite::CreateSandboxTemplate { engine, body } => {
            gcp.create_agent_platform(
                &format!("reasoningEngines/{engine}/sandboxEnvironmentTemplates"),
                body,
            )
            .await
        }
        ResourceWrite::CreateTopic { topic, body } => {
            gcp.create_pubsub("topics", topic, body).await?;
            Ok(json!({ "topic": topic }))
        }
        ResourceWrite::CreateSubscription { subscription, body } => {
            gcp.create_pubsub("subscriptions", subscription, body)
                .await?;
            Ok(json!({ "subscription": subscription }))
        }
        ResourceWrite::CreateCommandSubscription { subscription, body } => {
            gcp.create_pubsub("subscriptions", subscription, body)
                .await?;
            Ok(json!({ "subscription": subscription }))
        }
        ResourceWrite::CreateSchedulerJob { job, body } => {
            gcp.create_scheduler_job(body).await?;
            Ok(json!({ "job": job }))
        }
        ResourceWrite::SetTopicIamPolicy { topic, policy } => {
            gcp.set_pubsub_iam_policy("topics", topic, policy).await?;
            Ok(json!({ "topic": topic }))
        }
        ResourceWrite::SetSubscriptionIamPolicy {
            subscription,
            policy,
        } => {
            gcp.set_pubsub_iam_policy("subscriptions", subscription, policy)
                .await?;
            Ok(json!({ "subscription": subscription }))
        }
        other => Err(refused(format!("GCP takes no {} write", other.operation()))),
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;
    use alien_aws_clients::s3::{
        EventBridgeConfiguration, LambdaFunctionConfiguration, TopicConfiguration,
    };

    fn queue(id: &str, arn: &str) -> QueueConfiguration {
        QueueConfiguration {
            id: Some(id.into()),
            queue_arn: arn.into(),
            events: vec!["s3:ObjectCreated:*".into()],
            filter: None,
        }
    }

    fn check(
        new: &NotificationConfiguration,
        current: &NotificationConfiguration,
        desired: &[QueueConfiguration],
    ) -> Result<()> {
        check_notification_document(new, current, desired, &[], "assets")
    }

    #[test]
    fn only_the_workers_own_function_entry_is_added() {
        let own = LambdaFunctionConfiguration {
            id: Some("acme-worker-s3-assets".into()),
            lambda_function_arn: "arn:aws:lambda:us-east-1:1:function:acme-worker".into(),
            events: vec!["s3:ObjectCreated:*".into()],
            filter: None,
        };
        let current = NotificationConfiguration::default();
        let with = |entry: LambdaFunctionConfiguration| NotificationConfiguration {
            lambda_function_configurations: vec![entry],
            ..Default::default()
        };
        let run = |new: &NotificationConfiguration| {
            check_notification_document(new, &current, &[], std::slice::from_ref(&own), "worker")
        };
        run(&with(own.clone())).expect("the worker's own entry is accepted");
        for (name, entry) in [
            (
                "other function",
                LambdaFunctionConfiguration {
                    lambda_function_arn: "arn:aws:lambda:us-east-1:9:function:evil".into(),
                    ..own.clone()
                },
            ),
            (
                "other events",
                LambdaFunctionConfiguration {
                    events: vec!["s3:ObjectRemoved:*".into()],
                    ..own.clone()
                },
            ),
            (
                "other id",
                LambdaFunctionConfiguration {
                    id: Some("x".into()),
                    ..own.clone()
                },
            ),
        ] {
            assert!(run(&with(entry)).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn the_signed_entries_and_the_buckets_own_are_accepted_unchanged() {
        let signed = queue("acme-assets-0", "arn:aws:sqs:us-east-1:1:acme-jobs");
        let hand_made = queue("theirs", "arn:aws:sqs:us-east-1:1:other");
        let current = NotificationConfiguration {
            queue_configurations: vec![hand_made.clone()],
            ..Default::default()
        };
        let new = NotificationConfiguration {
            queue_configurations: vec![hand_made, signed.clone()],
            ..Default::default()
        };
        check(&new, &current, &[signed]).expect("accepted");
        check(&NotificationConfiguration::default(), &current, &[]).expect("removal is accepted");
    }

    #[test]
    fn a_destination_the_stack_does_not_declare_is_refused() {
        let signed = queue("acme-assets-0", "arn:aws:sqs:us-east-1:1:acme-jobs");
        let current = NotificationConfiguration::default();
        let foreign_queue = NotificationConfiguration {
            queue_configurations: vec![queue("acme-assets-0", "arn:aws:sqs:us-east-1:9:evil")],
            ..Default::default()
        };
        let changed_filter = NotificationConfiguration {
            queue_configurations: vec![QueueConfiguration {
                events: vec!["s3:ObjectRemoved:*".into()],
                ..signed.clone()
            }],
            ..Default::default()
        };
        let function = NotificationConfiguration {
            lambda_function_configurations: vec![LambdaFunctionConfiguration {
                id: None,
                lambda_function_arn: "arn:aws:lambda:us-east-1:9:function:evil".into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: None,
            }],
            ..Default::default()
        };
        let topic = NotificationConfiguration {
            topic_configurations: vec![TopicConfiguration {
                id: None,
                topic_arn: "arn:aws:sns:us-east-1:9:evil".into(),
                events: vec!["s3:ObjectCreated:*".into()],
                filter: None,
            }],
            ..Default::default()
        };
        let event_bridge = NotificationConfiguration {
            event_bridge_configuration: Some(EventBridgeConfiguration {}),
            ..Default::default()
        };
        for (name, new) in [
            ("foreign queue", foreign_queue),
            ("changed events", changed_filter),
            ("function", function),
            ("topic", topic),
            ("event bridge", event_bridge),
        ] {
            assert!(
                check(&new, &current, std::slice::from_ref(&signed)).is_err(),
                "{name} was accepted"
            );
        }
    }
}

#[cfg(test)]
mod command_subscription_tests {
    use super::*;

    const URI: &str = "https://acme-api-abc123-uc.a.run.app";

    fn place() -> Place {
        Place {
            install: alien_signing::InstallId::Gcp {
                project: "acme-project".into(),
                prefix: "acme".into(),
            },
            prefix: "acme".into(),
            vpc: None,
            machine_network: None,
        }
    }

    fn worker(public: bool) -> alien_core::Worker {
        let mut worker = alien_core::Worker::new("api".to_string())
            .code(alien_core::WorkerCode::Image {
                image: format!("registry.example/api@sha256:{}", "a".repeat(64)),
            })
            .permissions("execution".to_string())
            .build();
        worker.commands_enabled = true;
        if public {
            worker.public_endpoints = vec![alien_core::WorkerPublicEndpoint {
                name: "api".into(),
                host_label: None,
                wildcard_subdomains: false,
            }];
        }
        worker
    }

    fn execution_account() -> String {
        alien_permissions::gcp_resource_grants::profile_service_account_email(
            &alien_permissions::gcp_resource_grants::GcpInstall {
                prefix: "acme",
                project_id: "acme-project",
                project_number: None,
                region: "",
            },
            "execution",
        )
    }

    fn signed_body() -> Value {
        json!({
            "name": "projects/acme-project/subscriptions/acme-api-rq-sub",
            "topic": "projects/acme-project/topics/acme-api-rq",
            "pushConfig": {
                "pushEndpoint": format!("{URI}/"),
                "attributes": {},
                "oidcToken": {
                    "serviceAccountEmail": execution_account(),
                    "audience": URI,
                },
            },
            "ackDeadlineSeconds": 60,
            "retainAckedMessages": false,
            "labels": {"commands": "api", "deployment": "acme"},
            "enableMessageOrdering": false,
            "detached": false,
        })
    }

    fn check(public: bool, body: &Value, uri: Option<&str>) -> Result<()> {
        check_command_subscription_of(
            &worker(public),
            &place(),
            "api",
            "acme-api-rq-sub",
            body,
            uri,
        )
    }

    #[test]
    fn the_signed_subscription_is_accepted_and_anything_else_refused() {
        let signed = signed_body();
        check(false, &signed, None).expect("shape");
        check(false, &signed, Some(URI)).expect("with the service URL");

        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut body = signed.clone();
            f(&mut body);
            body
        };
        let refusals: Vec<(&str, Value, Option<&str>)> = vec![
            (
                "endpoint elsewhere",
                mutate(&|b| b["pushConfig"]["pushEndpoint"] = json!("https://evil.example/")),
                Some(URI),
            ),
            (
                "another service's URL",
                signed.clone(),
                Some("https://other-abc123-uc.a.run.app"),
            ),
            (
                "http endpoint",
                mutate(&|b| b["pushConfig"]["pushEndpoint"] = json!("http://x/")),
                None,
            ),
            (
                "another service account",
                mutate(&|b| {
                    b["pushConfig"]["oidcToken"]["serviceAccountEmail"] =
                        json!("acme-management-sa@acme-project.iam.gserviceaccount.com")
                }),
                Some(URI),
            ),
            (
                "another audience",
                mutate(&|b| {
                    b["pushConfig"]["oidcToken"]["audience"] = json!("https://evil.example")
                }),
                Some(URI),
            ),
            (
                "no OIDC on a private worker",
                mutate(&|b| {
                    b["pushConfig"].as_object_mut().unwrap().remove("oidcToken");
                }),
                Some(URI),
            ),
            (
                "another topic",
                mutate(&|b| b["topic"] = json!("projects/acme-project/topics/other")),
                Some(URI),
            ),
            (
                "an extra field",
                mutate(&|b| b["deadLetterPolicy"] = json!({})),
                Some(URI),
            ),
            (
                "an attribute on the push config",
                mutate(&|b| b["pushConfig"]["attributes"] = json!({"x-goog-version": "v1"})),
                Some(URI),
            ),
            (
                "a foreign label",
                mutate(&|b| b["labels"]["owner"] = json!("me")),
                Some(URI),
            ),
        ];
        for (name, body, uri) in refusals {
            assert!(check(false, &body, uri).is_err(), "{name} must be refused");
        }

        // A public worker takes no OIDC token.
        assert!(check(true, &signed, Some(URI)).is_err());
        let mut public = signed.clone();
        public["pushConfig"]
            .as_object_mut()
            .unwrap()
            .remove("oidcToken");
        check(true, &public, Some(URI)).expect("public worker without OIDC");

        // Another subscription name is refused.
        assert!(check_command_subscription_of(
            &worker(false),
            &place(),
            "api",
            "acme-other-rq-sub",
            &signed,
            Some(URI)
        )
        .is_err());
    }

    fn scheduler_body(uri: &str) -> Value {
        json!({
            "name": "projects/acme-project/locations/us-central1/jobs/acme-api-cron-1",
            "description": "Schedule trigger for worker 'api' (index 1)",
            "schedule": "*/5 * * * *",
            "timeZone": "UTC",
            "httpTarget": {
                "uri": uri,
                "httpMethod": "POST",
                "oidcToken": {"serviceAccountEmail": execution_account(), "audience": uri},
            },
        })
    }

    fn scheduled_worker() -> alien_core::Worker {
        let mut worker = worker(false);
        worker.triggers = vec![
            alien_core::WorkerTrigger::Schedule {
                cron: "0 * * * *".into(),
            },
            alien_core::WorkerTrigger::Schedule {
                cron: "*/5 * * * *".into(),
            },
        ];
        worker
    }

    fn check_job(job: &str, body: &Value, uri: Option<&str>) -> Result<()> {
        check_scheduler_job_of(
            &scheduled_worker(),
            &place(),
            "api",
            job,
            body,
            Some("us-central1"),
            uri,
        )
    }

    #[test]
    fn a_scheduler_job_is_accepted_only_as_the_signed_cron_calling_the_workers_own_service() {
        let signed = scheduler_body(URI);
        check_job("acme-api-cron-1", &signed, None).expect("shape");
        check_job("acme-api-cron-1", &signed, Some(URI)).expect("with the service URL");

        let mutate = |f: &dyn Fn(&mut Value)| {
            let mut body = signed.clone();
            f(&mut body);
            body
        };
        let refusals: Vec<(&str, &str, Value, Option<&str>)> = vec![
            (
                "another cron",
                "acme-api-cron-1",
                mutate(&|b| b["schedule"] = json!("* * * * *")),
                Some(URI),
            ),
            (
                "the other trigger's name",
                "acme-api-cron-0",
                signed.clone(),
                Some(URI),
            ),
            (
                "a trigger that does not exist",
                "acme-api-cron-7",
                signed.clone(),
                Some(URI),
            ),
            (
                "a padded index",
                "acme-api-cron-01",
                signed.clone(),
                Some(URI),
            ),
            (
                "another service's job",
                "acme-other-cron-1",
                signed.clone(),
                Some(URI),
            ),
            (
                "a target elsewhere",
                "acme-api-cron-1",
                scheduler_body("https://evil.example"),
                Some(URI),
            ),
            (
                "another service URL",
                "acme-api-cron-1",
                signed.clone(),
                Some("https://other-abc123-uc.a.run.app"),
            ),
            (
                "another service account",
                "acme-api-cron-1",
                mutate(&|b| {
                    b["httpTarget"]["oidcToken"]["serviceAccountEmail"] =
                        json!("acme-management-sa@acme-project.iam.gserviceaccount.com")
                }),
                Some(URI),
            ),
            (
                "another audience",
                "acme-api-cron-1",
                mutate(&|b| {
                    b["httpTarget"]["oidcToken"]["audience"] = json!("https://evil.example")
                }),
                Some(URI),
            ),
            (
                "no OIDC token",
                "acme-api-cron-1",
                mutate(&|b| {
                    b["httpTarget"].as_object_mut().unwrap().remove("oidcToken");
                }),
                Some(URI),
            ),
            (
                "a body",
                "acme-api-cron-1",
                mutate(&|b| b["httpTarget"]["body"] = json!("e30=")),
                Some(URI),
            ),
            (
                "headers",
                "acme-api-cron-1",
                mutate(&|b| b["httpTarget"]["headers"] = json!({"a": "b"})),
                Some(URI),
            ),
            (
                "a method",
                "acme-api-cron-1",
                mutate(&|b| b["httpTarget"]["httpMethod"] = json!("DELETE")),
                Some(URI),
            ),
            (
                "an extra field",
                "acme-api-cron-1",
                mutate(&|b| b["pubsubTarget"] = json!({})),
                Some(URI),
            ),
            (
                "another region",
                "acme-api-cron-1",
                mutate(&|b| {
                    b["name"] =
                        json!("projects/acme-project/locations/europe-west1/jobs/acme-api-cron-1")
                }),
                Some(URI),
            ),
            (
                "another project",
                "acme-api-cron-1",
                mutate(&|b| {
                    b["name"] = json!("projects/other/locations/us-central1/jobs/acme-api-cron-1")
                }),
                Some(URI),
            ),
            (
                "another time zone",
                "acme-api-cron-1",
                mutate(&|b| b["timeZone"] = json!("Europe/Paris")),
                Some(URI),
            ),
        ];
        for (name, job, body, uri) in refusals {
            assert!(
                check_job(job, &body, uri).is_err(),
                "{name} must be refused"
            );
        }
    }
}

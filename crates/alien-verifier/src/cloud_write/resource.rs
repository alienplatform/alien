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
            ResourceWrite::CreateEventSourceMapping { .. } => "CreateEventSourceMapping",
            ResourceWrite::UpdateEventSourceMapping { .. } => "UpdateEventSourceMapping",
            ResourceWrite::CreateQueue { .. } => "CreateQueue",
            ResourceWrite::SetQueueAttributes { .. } => "SetQueueAttributes",
            ResourceWrite::SetBucketIamPolicy { .. } => "SetBucketIamPolicy",
            ResourceWrite::SetPublicAccessPrevention { .. } => "SetPublicAccessPrevention",
            ResourceWrite::CreateTopic { .. } => "CreateTopic",
            ResourceWrite::CreateSubscription { .. } => "CreateSubscription",
            ResourceWrite::SetTopicIamPolicy { .. } => "SetTopicIamPolicy",
            ResourceWrite::SetSubscriptionIamPolicy { .. } => "SetSubscriptionIamPolicy",
            ResourceWrite::CreateBlobContainer { .. } => "CreateBlobContainer",
            ResourceWrite::PutEventSubscription { .. } => "PutEventSubscription",
            ResourceWrite::UpdateBlobContainer { .. } => "UpdateBlobContainer",
            ResourceWrite::CreateSandboxEngine { .. } => "CreateSandboxEngine",
            ResourceWrite::CreateSandboxTemplate { .. } => "CreateSandboxTemplate",
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
        ResourceWrite::SetBucketIamPolicy { .. }
        | ResourceWrite::SetPublicAccessPrevention { .. }
        | ResourceWrite::CreateTopic { .. }
        | ResourceWrite::CreateSubscription { .. }
        | ResourceWrite::SetTopicIamPolicy { .. }
        | ResourceWrite::SetSubscriptionIamPolicy { .. }
        | ResourceWrite::CreateBlobContainer { .. }
        | ResourceWrite::UpdateBlobContainer { .. }
        | ResourceWrite::PutEventSubscription { .. }
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
    let ResourceWrite::PutBucketNotification {
        bucket,
        configuration,
    } = write
    else {
        return Ok(());
    };
    use alien_error::Context;
    let (account, region) = aws_account_and_region(facts)?;
    let desired = desired_queue_entries(facts, &region, &account)?;
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
        facts.resource_id,
    )
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
    if let Some(entry) = new
        .lambda_function_configurations
        .iter()
        .find(|entry| !current.lambda_function_configurations.contains(entry))
    {
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
    lambda: &dyn alien_aws_clients::lambda::LambdaApi,
    s3: &dyn alien_aws_clients::s3::S3Api,
    sqs: &dyn alien_aws_clients::sqs::SqsApi,
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
        } => {
            let document = notification_document(configuration)?;
            s3.put_bucket_notification_configuration(bucket, &document)
                .await
                .context(crate::error::ErrorData::CloudCallFailed {
                    operation: format!("s3:PutBucketNotification {bucket}"),
                })?;
            Ok(json!({ "bucket": bucket }))
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
        ResourceWrite::SetBucketIamPolicy { .. }
        | ResourceWrite::SetPublicAccessPrevention { .. }
        | ResourceWrite::CreateTopic { .. }
        | ResourceWrite::CreateSubscription { .. }
        | ResourceWrite::SetTopicIamPolicy { .. }
        | ResourceWrite::SetSubscriptionIamPolicy { .. }
        | ResourceWrite::CreateBlobContainer { .. }
        | ResourceWrite::UpdateBlobContainer { .. }
        | ResourceWrite::PutEventSubscription { .. }
        | ResourceWrite::CreateSandboxEngine { .. }
        | ResourceWrite::CreateSandboxTemplate { .. } => {
            Err(refused(format!("AWS takes no {} write", write.operation())))
        }
    }
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
    if engine.is_empty() || !engine.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(refused(format!("'{engine}' is not an engine id")));
    }
    let signed_image = match &sandbox.code {
        alien_core::SandboxCode::Image { image, .. } => image,
        _ => return Err(refused("the signed sandbox has no built image")),
    };
    let signed_egress = sandbox
        .egress
        .internet_access_switch()
        .ok_or_else(|| refused("the signed sandbox's egress is not one Agent Platform expresses"))?;
    let limits = sandbox.resolved_limits();
    let own_name = format!("{prefix}-{}", sandbox.id());
    let template: alien_gcp_clients::gcp::agent_platform::SandboxEnvironmentTemplate =
        serde_json::from_value(body.clone())
            .map_err(|error| refused(format!("the template is not one: {error}")))?;
    if template.name.is_some() || template.state.is_some() || !template.extra.is_empty() {
        return Err(refused("the template sets a field the controller does not"));
    }
    if template.display_name.as_deref() != Some(own_name.as_str()) {
        return Err(refused(format!("the template's display name is not {own_name}")));
    }
    if template.egress_control_config.as_ref().map(|e| (e.internet_access, e.extra.is_empty()))
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
        return Err(refused("the template's environment sets a field the controller does not"));
    }
    let spec = environment
        .custom_container_spec
        .ok_or_else(|| refused("the template names no image"))?;
    if !spec.extra.is_empty() {
        return Err(refused("the template's container spec sets a field the controller does not"));
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
        return Err(refused("the template's ports are not the agent's one TCP port"));
    }
    let resources = environment.resources.unwrap_or_default();
    let expected_limits = std::collections::HashMap::from([
        ("cpu".to_string(), limits.cpu),
        ("memory".to_string(), limits.memory),
    ]);
    if resources.requests.is_some() || resources.limits != Some(expected_limits) {
        return Err(refused("the template's resources are not the signed limits"));
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
/// verifier only reaches the ones in its own resource group, so the name is only checked to be
/// a plain account name.
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
    check_account_name(write, account)?;
    check_blob_container_body(body, storage.public_read).map_err(refused)
}

fn check_account_name(write: &ResourceWrite, account: &str) -> Result<()> {
    let plain = (3..=24).contains(&account.len())
        && account
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if plain {
        Ok(())
    } else {
        Err(refused(format!(
            "{} names storage account '{account}', which is not an Azure storage account name",
            write.operation()
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
    check_account_name(
        &ResourceWrite::PutEventSubscription {
            account: account.to_string(),
            name: name.to_string(),
            body: Value::Null,
        },
        account,
    )?;
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
        check_notification_document(new, current, desired, "assets")
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

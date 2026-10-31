//! A Storage's S3 queue configurations live in the bucket's notification document, next
//! to entries other tools own. The controller recognises its own entries by their Id
//! prefix and replaces only those, in one read-modify-write.

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    queue::aws::AwsQueueController,
};
use alien_aws_clients::s3::{
    FilterRule, NotificationConfiguration, NotificationFilter, QueueConfiguration, S3Api,
    S3KeyFilter,
};
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    storage_notifications::{aws_notification_id, aws_notification_id_prefix},
    Storage, StorageNotification,
};
use alien_error::{AlienError, ContextError};

const CREATED: &str = "s3:ObjectCreated:*";

fn cloud_error(id: &str, action: &str, bucket: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(id.into()),
        message: format!("{action} notification configuration of bucket '{bucket}'"),
    }
}

fn missing(error: &AlienError<ClientError>) -> bool {
    matches!(
        &error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    )
}

/// The configurations this Storage should own, in rule order.
fn desired_configurations(
    ctx: &ResourceControllerContext<'_>,
    storage: &Storage,
) -> Result<Vec<QueueConfiguration>> {
    let cfg = ctx.get_aws_config()?;
    let mut configurations = Vec::new();
    for (index, notification) in storage.notifications.iter().enumerate() {
        let queue = ctx.require_dependency::<AwsQueueController>(&notification.queue)?;
        let queue_name = queue.queue_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: storage.id.clone(),
                dependency_id: notification.queue.id.clone(),
            })
        })?;
        configurations.push(queue_configuration(
            notification,
            format!("arn:aws:sqs:{}:{}:{queue_name}", cfg.region, cfg.account_id),
            Some(aws_notification_id(ctx.resource_prefix, &storage.id, index)),
        ));
    }
    Ok(configurations)
}

/// The S3 queue configuration a rule becomes, as setup and the controller write it.
pub(super) fn queue_configuration(
    notification: &StorageNotification,
    queue_arn: String,
    id: Option<String>,
) -> QueueConfiguration {
    let filter_rules: Vec<FilterRule> = [
        ("prefix", &notification.prefix),
        ("suffix", &notification.suffix),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        value.as_ref().map(|value| FilterRule {
            name: name.into(),
            value: value.clone(),
        })
    })
    .collect();
    QueueConfiguration {
        id,
        queue_arn,
        events: vec![CREATED.into()],
        filter: (!filter_rules.is_empty()).then(|| NotificationFilter {
            key: S3KeyFilter { filter_rules },
        }),
    }
}

/// S3 returns filter names as `Prefix`/`Suffix` and may reorder rules and events.
fn normalized(configuration: &QueueConfiguration) -> impl PartialEq + std::fmt::Debug {
    (configuration.id.clone(), rule_of(configuration))
}

/// What an entry delivers, without its Id.
fn rule_of(configuration: &QueueConfiguration) -> impl PartialEq + std::fmt::Debug {
    let mut events = configuration.events.clone();
    events.sort();
    let mut filters: Vec<(String, String)> = configuration
        .filter
        .iter()
        .flat_map(|filter| &filter.key.filter_rules)
        .map(|rule| (rule.name.to_ascii_lowercase(), rule.value.clone()))
        .collect();
    filters.sort();
    (configuration.queue_arn.clone(), events, filters)
}

/// Which queue configurations a Storage owns: those whose Id starts with the owner
/// prefix, and on retention of a setup-owned bucket also those that are exactly one of
/// the rules setup wrote. CloudFormation can't set an Id on a queue configuration, and S3
/// gives each entry a random Id that changes on every stack update, so only the rule
/// identifies those entries. S3 rejects two entries that can match the same key, so a
/// document holds at most one entry with a given rule. Hand-made entries, including
/// ones to the same queue with other filters, are never owned.
struct Owned<'a> {
    prefix: &'a str,
    setup_rules: &'a [QueueConfiguration],
}

impl Owned<'_> {
    fn contains(&self, configuration: &QueueConfiguration) -> bool {
        configuration
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with(self.prefix))
            || self
                .setup_rules
                .iter()
                .any(|rule| rule_of(rule) == rule_of(configuration))
    }
}

/// Replace exactly the owned entries; returns whether anything changed. Every other
/// Lambda, queue, topic and EventBridge entry is kept as observed.
fn replace_owned(
    document: &mut NotificationConfiguration,
    owner: &Owned<'_>,
    desired: Vec<QueueConfiguration>,
) -> bool {
    let owned = |configuration: &QueueConfiguration| owner.contains(configuration);
    let mut observed: Vec<_> = document
        .queue_configurations
        .iter()
        .filter(|configuration| owned(configuration))
        .map(normalized)
        .collect();
    let mut wanted: Vec<_> = desired.iter().map(normalized).collect();
    let sort = |values: &mut Vec<_>| values.sort_by_key(|value| format!("{value:?}"));
    sort(&mut observed);
    sort(&mut wanted);
    if observed == wanted {
        return false;
    }
    document
        .queue_configurations
        .retain(|configuration| !owned(configuration));
    document.queue_configurations.extend(desired);
    true
}

/// Make the bucket's owned entries match the declaration (none while deleting).
pub(super) async fn reconcile(
    ctx: &ResourceControllerContext<'_>,
    bucket: &str,
    storage: &Storage,
    deleting: bool,
) -> Result<()> {
    let desired = if deleting {
        Vec::new()
    } else {
        desired_configurations(ctx, storage)?
    };
    let s3 = ctx
        .service_provider
        .get_aws_s3_client(ctx.get_aws_config()?)
        .await?;
    write_owned(
        s3.as_ref(),
        &Owned {
            prefix: &aws_notification_id_prefix(ctx.resource_prefix, &storage.id),
            setup_rules: &[],
        },
        bucket,
        &storage.id,
        desired,
        deleting,
    )
    .await
}

/// Retention keeps the bucket and its data but removes this Storage's entries.
/// `setup_rules` are the entries setup wrote for a setup-owned Storage (see [`Owned`]); a
/// Live Storage passes none, because its controller wrote an Id on every entry.
///
/// S3 validates every destination left in the document. Retention runs before setup
/// teardown deletes the queues, so this Storage's entries go while their queue exists. A
/// kept entry whose destination is already gone makes the write fail; that is reported,
/// and the entry is never dropped.
pub(super) async fn remove_owned(
    client: &dyn S3Api,
    prefix: &str,
    bucket: &str,
    id: &str,
    setup_rules: &[QueueConfiguration],
) -> Result<()> {
    write_owned(
        client,
        &Owned {
            prefix: &aws_notification_id_prefix(prefix, id),
            setup_rules,
        },
        bucket,
        id,
        Vec::new(),
        true,
    )
    .await
}

async fn write_owned(
    client: &dyn S3Api,
    owner: &Owned<'_>,
    bucket: &str,
    id: &str,
    desired: Vec<QueueConfiguration>,
    removing: bool,
) -> Result<()> {
    let mut document = match client.get_bucket_notification_configuration(bucket).await {
        Ok(document) => document,
        // A bucket that is already gone has no entries left to remove.
        Err(error) if removing && missing(&error) => return Ok(()),
        Err(error) => return Err(error.context(cloud_error(id, "Read", bucket))),
    };
    if !replace_owned(&mut document, owner, desired) {
        return Ok(());
    }
    match client
        .put_bucket_notification_configuration(bucket, &document)
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if removing && missing(&error) => Ok(()),
        Err(error) if removing => Err(error.context(ErrorData::CloudPlatformError {
            resource_id: Some(id.into()),
            message: format!(
                "Remove this Storage's queue notifications from bucket '{bucket}'. S3 validates \
                 every destination left in the document, and entries this deployment didn't \
                 create are kept: if one points at a queue, topic or function that no longer \
                 exists, fix or remove it, then retry"
            ),
        })),
        Err(error) => Err(error.context(cloud_error(id, "Write", bucket))),
    }
}

#[cfg(test)]
#[path = "aws_notifications_tests.rs"]
mod tests;

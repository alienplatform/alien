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
    Storage,
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
        configurations.push(QueueConfiguration {
            id: Some(aws_notification_id(ctx.resource_prefix, &storage.id, index)),
            queue_arn: format!("arn:aws:sqs:{}:{}:{queue_name}", cfg.region, cfg.account_id),
            events: vec![CREATED.into()],
            filter: (!filter_rules.is_empty()).then(|| NotificationFilter {
                key: S3KeyFilter { filter_rules },
            }),
        });
    }
    Ok(configurations)
}

/// S3 returns filter names as `Prefix`/`Suffix` and may reorder rules and events.
fn normalized(configuration: &QueueConfiguration) -> impl PartialEq + std::fmt::Debug {
    let mut events = configuration.events.clone();
    events.sort();
    let mut filters: Vec<(String, String)> = configuration
        .filter
        .iter()
        .flat_map(|filter| &filter.key.filter_rules)
        .map(|rule| (rule.name.to_ascii_lowercase(), rule.value.clone()))
        .collect();
    filters.sort();
    (
        configuration.id.clone(),
        configuration.queue_arn.clone(),
        events,
        filters,
    )
}

/// Replace exactly the entries whose Id starts with `owner`; returns whether anything
/// changed. Every other Lambda, queue, topic and EventBridge entry is kept as observed.
fn replace_owned(
    document: &mut NotificationConfiguration,
    owner: &str,
    desired: Vec<QueueConfiguration>,
) -> bool {
    let owned = |configuration: &QueueConfiguration| {
        configuration
            .id
            .as_deref()
            .is_some_and(|id| id.starts_with(owner))
    };
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
        &aws_notification_id_prefix(ctx.resource_prefix, &storage.id),
        bucket,
        &storage.id,
        desired,
        deleting,
    )
    .await
}

/// Retention keeps the bucket and its data but removes this Storage's entries.
pub(super) async fn remove_owned(
    client: &dyn S3Api,
    prefix: &str,
    bucket: &str,
    id: &str,
) -> Result<()> {
    write_owned(
        client,
        &aws_notification_id_prefix(prefix, id),
        bucket,
        id,
        Vec::new(),
        true,
    )
    .await
}

async fn write_owned(
    client: &dyn S3Api,
    owner: &str,
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
        Err(error) => Err(error.context(cloud_error(id, "Write", bucket))),
    }
}

#[cfg(test)]
#[path = "aws_notifications_tests.rs"]
mod tests;

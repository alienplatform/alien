//! Queue notification rules have deterministic identities, so a lost provider
//! response can be recovered by observation. Each step performs one mutation.

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    queue::aws::AwsQueueController,
};
use alien_aws_clients::{
    eventbridge::{
        EventBridgeApi, EventBridgeRule, EventBridgeTarget, InputTransformer, PutRuleRequest,
        PutTargetsRequest,
    },
    s3::EventBridgeConfiguration,
};
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    storage_notifications::{
        aws_eventbridge_pattern, aws_notification_input_paths, aws_notification_rule_name,
        AWS_NOTIFICATION_INPUT_TEMPLATE,
    },
    Storage,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};

fn cloud_error(id: &str, action: &str, name: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(id.into()),
        message: format!("{action} storage notification '{name}'"),
    }
}

fn description(bucket: &str) -> String {
    format!("Storage queue notifications for {bucket}")
}

fn missing(error: &AlienError<ClientError>) -> bool {
    matches!(
        &error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    )
}

async fn read_rule(
    client: &dyn EventBridgeApi,
    name: &str,
    bucket: &str,
    id: &str,
) -> Result<Option<EventBridgeRule>> {
    match client.describe_rule(name).await {
        Ok(rule) => {
            if rule.description.as_deref() != Some(description(bucket).as_str())
                || rule.managed_by.is_some()
            {
                return Err(AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
                    resource_id: id.into(),
                    object: format!("EventBridge rule '{name}'"),
                }));
            }
            Ok(Some(rule))
        }
        Err(error) if missing(&error) => Ok(None),
        Err(error) => Err(error.context(cloud_error(id, "Describe", name))),
    }
}

/// Return true only after every rule and target has been observed in desired state.
pub(super) async fn reconcile(
    ctx: &ResourceControllerContext<'_>,
    bucket: &str,
    storage: &Storage,
    deleting: bool,
) -> Result<bool> {
    let cfg = ctx.get_aws_config()?;
    if !deleting && !storage.notifications.is_empty() {
        let s3 = ctx.service_provider.get_aws_s3_client(cfg).await?;
        let mut configuration = s3
            .get_bucket_notification_configuration(bucket)
            .await
            .context(cloud_error(
                &storage.id,
                "Read bucket delivery configuration",
                bucket,
            ))?;
        if configuration.event_bridge_configuration.is_none() {
            configuration.event_bridge_configuration = Some(EventBridgeConfiguration {});
            s3.put_bucket_notification_configuration(bucket, &configuration)
                .await
                .context(cloud_error(
                    &storage.id,
                    "Enable EventBridge delivery",
                    bucket,
                ))?;
            return Ok(false);
        }
    }
    let client = ctx.service_provider.get_aws_eventbridge_client(cfg).await?;
    let count = if deleting {
        0
    } else {
        storage.notifications.len()
    };
    // The portable maximum bounds discovery, including rules created before a lost
    // checkpoint. No account-wide ListRules permission is needed.
    for index in 0..10 {
        let name = aws_notification_rule_name(ctx.resource_prefix, bucket, index);
        let observed = read_rule(client.as_ref(), &name, bucket, &storage.id).await?;
        if index >= count {
            if observed.is_some() && !remove_rule_step(client.as_ref(), &name, &storage.id).await? {
                return Ok(false);
            }
            continue;
        }
        let notification = &storage.notifications[index];
        let pattern = aws_eventbridge_pattern(notification, bucket);
        let observed_pattern = observed
            .as_ref()
            .and_then(|rule| rule.event_pattern.as_deref())
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .into_alien_error()
            .context(cloud_error(&storage.id, "Parse event pattern", &name))?;
        if observed.as_ref().is_none_or(|rule| {
            rule.state.as_deref() != Some("ENABLED") || rule.schedule_expression.is_some()
        }) || observed_pattern.as_ref() != Some(&pattern)
        {
            client
                .put_rule(
                    PutRuleRequest::builder()
                        .name(name.clone())
                        .event_pattern(pattern.to_string())
                        .state("ENABLED".into())
                        .description(description(bucket))
                        .build(),
                )
                .await
                .context(cloud_error(&storage.id, "Write rule", &name))?;
            return Ok(false);
        }
        let queue = ctx.require_dependency::<AwsQueueController>(&notification.queue)?;
        let queue_name = queue.queue_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: storage.id.clone(),
                dependency_id: notification.queue.id.clone(),
            })
        })?;
        let target = EventBridgeTarget {
            id: "queue".into(),
            arn: format!("arn:aws:sqs:{}:{}:{queue_name}", cfg.region, cfg.account_id),
            input_transformer: Some(InputTransformer {
                input_paths_map: aws_notification_input_paths(),
                input_template: AWS_NOTIFICATION_INPUT_TEMPLATE.into(),
            }),
        };
        let targets = client
            .list_targets_by_rule(&name)
            .await
            .context(cloud_error(&storage.id, "Read targets", &name))?;
        if !targets.iter().any(|observed| observed == &target) {
            client
                .put_targets(PutTargetsRequest {
                    rule: name.clone(),
                    targets: vec![target],
                })
                .await
                .context(cloud_error(&storage.id, "Write queue target", &name))?;
            return Ok(false);
        }
    }
    // EventBridge is a shared bucket switch: keep it enabled for independent rules.
    // Erasing the bucket removes it; retaining data removes only our owned rules.
    Ok(true)
}

async fn remove_rule_step(client: &dyn EventBridgeApi, name: &str, id: &str) -> Result<bool> {
    let targets = match client.list_targets_by_rule(name).await {
        Ok(targets) => targets,
        Err(error) if missing(&error) => return Ok(true),
        Err(error) => return Err(error.context(cloud_error(id, "Read targets", name))),
    };
    if targets.iter().any(|target| target.id != "queue") {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id:Some(id.into()), message:format!("EventBridge rule '{name}' has an independently managed target; remove that target before removing the storage notification"),
        }));
    }
    let result = if targets.is_empty() {
        client.delete_rule(name).await
    } else {
        client.remove_targets(name, vec!["queue".into()]).await
    };
    match result {
        Ok(()) => Ok(false),
        Err(error) if missing(&error) => Ok(true),
        Err(error) => Err(error.context(cloud_error(id, "Remove rule or target", name))),
    }
}

/// Retention cleanup uses the same bounded discovery and one-mutation boundary.
pub(super) async fn remove_owned_step(
    client: &dyn EventBridgeApi,
    prefix: &str,
    bucket: &str,
    id: &str,
) -> Result<bool> {
    for index in 0..10 {
        let name = aws_notification_rule_name(prefix, bucket, index);
        if read_rule(client, &name, bucket, id).await?.is_some()
            && !remove_rule_step(client, &name, id).await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "aws_notifications_tests.rs"]
mod tests;

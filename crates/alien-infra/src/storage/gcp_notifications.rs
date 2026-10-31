//! Additive GCS notification reconciliation. A stable custom attribute makes an
//! interrupted insert discoverable without adopting an out-of-band configuration.

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    queue::gcp::GcpQueueController,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    storage_notifications::{gcs_notification_configs, GCS_RULE_ATTRIBUTE},
    Storage,
};
use alien_error::{AlienError, Context};
use alien_gcp_clients::gcs::{GcsApi, GcsNotification};
use std::collections::HashMap;

/// Perform at most one mutation. The caller checkpoints and invokes this again
/// until it returns true, so failures never lose cleanup progress.
pub(super) async fn reconcile(
    ctx: &ResourceControllerContext<'_>,
    bucket: &str,
    storage: &Storage,
    deleting: bool,
) -> Result<bool> {
    let cfg = ctx.get_gcp_config()?;
    let client = ctx.service_provider.get_gcp_gcs_client(cfg)?;
    let owner = format!("{bucket}/");
    let mut desired = Vec::new();
    if !deleting {
        let configs =
            gcs_notification_configs(storage).context(ErrorData::ResourceConfigInvalid {
                resource_id: Some(storage.id.clone()),
                message: "Storage notifications cannot be expressed as GCS configs".to_string(),
            })?;
        for (index, config) in configs.into_iter().enumerate() {
            let queue = ctx.require_dependency::<GcpQueueController>(config.queue)?;
            let topic = queue.topic_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: storage.id.clone(),
                    dependency_id: config.queue.id.clone(),
                })
            })?;
            let mut custom_attributes =
                HashMap::from([(GCS_RULE_ATTRIBUTE.to_string(), format!("{owner}{index}"))]);
            custom_attributes.extend(config.suffix_attributes);
            desired.push(GcsNotification {
                id: None,
                topic: Some(format!("projects/{}/topics/{topic}", cfg.project_id)),
                event_types: vec!["OBJECT_FINALIZE".into()],
                payload_format: Some("JSON_API_V1".into()),
                object_name_prefix: config.prefix.map(str::to_string),
                custom_attributes,
            });
        }
    }
    let existing = match client.list_notifications(bucket.into()).await {
        Ok(list) => list.items,
        Err(error)
            if deleting
                && matches!(
                    &error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
        {
            return Ok(true)
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                resource_id: Some(storage.id.clone()),
                message: format!("List notifications on bucket '{bucket}'"),
            })
        }
    };
    // Remove obsolete rules before inserting to respect the per-event quota.
    // Remove duplicate owned entries too, including any from an ambiguous POST.
    let mut matched = vec![false; desired.len()];
    for notification in &existing {
        if !notification
            .custom_attributes
            .get(GCS_RULE_ATTRIBUTE)
            .is_some_and(|value| value.starts_with(&owner))
        {
            continue;
        }
        if let Some((index, _)) = desired
            .iter()
            .enumerate()
            .find(|(index, rule)| !matched[*index] && matches(notification, rule))
        {
            matched[index] = true;
            continue;
        }
        let id = notification.id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(storage.id.clone()),
                message: format!("GCS returned a notification without an ID on '{bucket}'"),
            })
        })?;
        match client.delete_notification(bucket.into(), id.clone()).await {
            Ok(()) => {}
            Err(error)
                if matches!(
                    &error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    resource_id: Some(storage.id.clone()),
                    message: format!("Delete notification '{id}' on bucket '{bucket}'"),
                })
            }
        }
        return Ok(false);
    }
    if let Some((_, notification)) = desired
        .into_iter()
        .enumerate()
        .find(|(index, _)| !matched[*index])
    {
        client
            .insert_notification(bucket.into(), notification)
            .await
            .context(ErrorData::CloudPlatformError {
                resource_id: Some(storage.id.clone()),
                message: format!("Create queue notification on bucket '{bucket}'"),
            })?;
        return Ok(false);
    }
    Ok(true)
}

fn matches(existing: &GcsNotification, desired: &GcsNotification) -> bool {
    // GCS canonicalizes topics to //pubsub.googleapis.com/projects/… on reads.
    existing
        .topic
        .as_deref()
        .map(|topic| topic.trim_start_matches("//pubsub.googleapis.com/"))
        == desired.topic.as_deref()
        && existing.event_types == desired.event_types
        && existing.payload_format == desired.payload_format
        && existing.object_name_prefix.as_deref().unwrap_or("")
            == desired.object_name_prefix.as_deref().unwrap_or("")
        && existing.custom_attributes == desired.custom_attributes
}

/// Retention bypasses bucket deletion, but owned delivery rules must still be removed.
/// Each delete is idempotent; a repeated cleanup lists the remaining rules afresh.
pub(super) async fn remove_owned(
    client: &dyn GcsApi,
    bucket: &str,
    resource_id: &str,
) -> Result<()> {
    let existing = match client.list_notifications(bucket.into()).await {
        Ok(list) => list.items,
        Err(error)
            if matches!(
                &error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(())
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                resource_id: Some(resource_id.into()),
                message: format!("List retained bucket '{bucket}' notifications"),
            })
        }
    };
    for notification in existing {
        if !notification
            .custom_attributes
            .get(GCS_RULE_ATTRIBUTE)
            .is_some_and(|value| value.starts_with(&format!("{bucket}/")))
        {
            continue;
        }
        let id = notification.id.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(resource_id.into()),
                message: format!("GCS returned a notification without an ID on '{bucket}'"),
            })
        })?;
        match client.delete_notification(bucket.into(), id.clone()).await {
            Ok(()) => {}
            Err(error)
                if matches!(
                    &error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    resource_id: Some(resource_id.into()),
                    message: format!("Delete notification '{id}' from retained bucket '{bucket}'"),
                })
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider},
        queue::gcp::GcpQueueState,
        storage::{GcpStorageController, GcpStorageState},
    };
    use alien_core::{Platform, Queue, ResourceStatus, StorageNotification};
    use alien_gcp_clients::{
        gcs::{Bucket, ListNotificationsResponse, MockGcsApi},
        iam::MockIamApi,
    };
    use std::sync::{Arc, Mutex};

    fn storage(prefix: &str) -> Storage {
        Storage::new("uploads".into())
            .notifications(vec![
                StorageNotification::builder()
                    .queue((&Queue::new("work".into()).build()).into())
                    .prefix(prefix.into())
                    .suffix(".json".into())
                    .build(),
                StorageNotification::builder()
                    .queue((&Queue::new("work".into()).build()).into())
                    .prefix("other/".into())
                    .suffix(".data".into())
                    .build(),
            ])
            .build()
    }

    /// Right after the queue grants the storage service agent `pubsub.publisher`, GCS can
    /// still refuse to create the config (403) until IAM propagates. The step fails with a
    /// retryable error, so the executor retries it with bounded backoff, and the retry
    /// creates the config.
    #[tokio::test]
    async fn publisher_grant_propagation_denial_is_retried() {
        let remote = Arc::new(Mutex::new(Vec::<GcsNotification>::new()));
        let mut gcs = MockGcsApi::new();
        let read = remote.clone();
        gcs.expect_list_notifications().returning(move |_| {
            Ok(ListNotificationsResponse {
                items: read.lock().unwrap().clone(),
                ..Default::default()
            })
        });
        let attempts = Arc::new(Mutex::new(0));
        let counter = attempts.clone();
        let writes = remote.clone();
        gcs.expect_insert_notification()
            .returning(move |_, mut rule| {
                let mut attempts = counter.lock().unwrap();
                *attempts += 1;
                if *attempts == 1 {
                    return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                        resource_type: "notification".into(),
                        resource_name: "test-uploads".into(),
                    }));
                }
                rule.id = Some(attempts.to_string());
                rule.topic = rule
                    .topic
                    .map(|topic| format!("//pubsub.googleapis.com/{topic}"));
                writes.lock().unwrap().push(rule.clone());
                Ok(rule)
            });
        let gcs = Arc::new(gcs);
        let iam = Arc::new(MockIamApi::new());
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(gcs.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(GcpStorageController {
                state: GcpStorageState::CreatingNotifications,
                bucket_name: Some("test-uploads".into()),
                notifications_managed: true,
                ..Default::default()
            })
            .platform(Platform::Gcp)
            .service_provider(Arc::new(provider))
            .with_dependency(
                Queue::new("work".into()).build(),
                GcpQueueController {
                    state: GcpQueueState::Ready,
                    topic_name: Some("test-work".into()),
                    subscription_name: Some("test-work-sub".into()),
                    storage_publisher_member: Some("serviceAccount:storage".into()),
                    storage_publisher_pending: None,
                    _internal_stay_count: None,
                },
            )
            .build()
            .await
            .unwrap();
        let error = run
            .step()
            .await
            .expect_err("the first insert is denied while IAM propagates");
        assert!(error.retryable, "{error:?}");
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert_eq!(
            *attempts.lock().unwrap(),
            3,
            "one denied insert, then both configs"
        );
        assert_eq!(remote.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn reconcile_recovers_lost_insert_response_updates_and_removes_only_owned_rules() {
        let foreign = GcsNotification {
            id: Some("foreign".into()),
            topic: Some("projects/test/topics/manual".into()),
            event_types: vec!["OBJECT_FINALIZE".into()],
            payload_format: Some("JSON_API_V1".into()),
            object_name_prefix: Some("manual/".into()),
            ..Default::default()
        };
        let remote = Arc::new(Mutex::new(vec![foreign.clone()]));
        let mut gcs = MockGcsApi::new();
        gcs.expect_get_bucket().returning(|name| {
            Ok(Bucket {
                name: Some(name),
                ..Default::default()
            })
        });
        let read = remote.clone();
        gcs.expect_list_notifications().returning(move |_| {
            Ok(ListNotificationsResponse {
                items: read.lock().unwrap().clone(),
                ..Default::default()
            })
        });
        let writes = remote.clone();
        let mut next_id = 0;
        gcs.expect_insert_notification()
            .returning(move |_, mut rule| {
                next_id += 1;
                rule.id = Some(next_id.to_string());
                rule.topic = rule
                    .topic
                    .map(|topic| format!("//pubsub.googleapis.com/{topic}"));
                writes.lock().unwrap().push(rule.clone());
                if next_id == 1 {
                    Err(AlienError::new(CloudClientErrorData::Timeout {
                        message: "insert applied but response lost".into(),
                    }))
                } else {
                    Ok(rule)
                }
            });
        let deletes = remote.clone();
        gcs.expect_delete_notification().returning(move |_, id| {
            assert_ne!(id, "foreign");
            let mut rules = deletes.lock().unwrap();
            let old = rules.len();
            rules.retain(|rule| rule.id.as_ref() != Some(&id));
            assert_eq!(rules.len() + 1, old);
            Ok(())
        });
        let gcs = Arc::new(gcs);
        let iam = Arc::new(MockIamApi::new());
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_gcs_client()
            .returning(move |_| Ok(gcs.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let provider = Arc::new(provider);
        let queue = || GcpQueueController {
            state: GcpQueueState::Ready,
            topic_name: Some("test-work".into()),
            subscription_name: Some("test-work-sub".into()),
            storage_publisher_member: Some("serviceAccount:storage".into()),
            storage_publisher_pending: None,
            _internal_stay_count: None,
        };
        let checkpoint = GcpStorageController {
            state: GcpStorageState::CreatingNotifications,
            bucket_name: Some("test-uploads".into()),
            notifications_managed: true,
            ..Default::default()
        };
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(checkpoint.clone())
            .platform(Platform::Gcp)
            .service_provider(provider.clone())
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal()
            .await
            .expect_err("ambiguous insert must propagate");
        assert_eq!(remote.lock().unwrap().len(), 2);

        // Restore the last durable checkpoint, before the failed insert's response.
        let restored = serde_json::from_value::<GcpStorageController>(
            serde_json::to_value(checkpoint).unwrap(),
        )
        .unwrap();
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(restored)
            .platform(Platform::Gcp)
            .service_provider(provider.clone())
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let owned_ids: Vec<_> = remote
            .lock()
            .unwrap()
            .iter()
            .filter_map(|rule| rule.id.clone())
            .collect();
        assert_eq!(owned_ids, ["foreign", "1", "2"]);

        // Resume at the notification phase after the ordinary bucket update phases.
        let mut changed = run
            .internal_state::<GcpStorageController>()
            .unwrap()
            .clone();
        changed.state = GcpStorageState::UpdatingNotifications;
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("changed/"))
            .controller(changed)
            .platform(Platform::Gcp)
            .service_provider(provider.clone())
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let rules = remote.lock().unwrap().clone();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].id.as_deref(), Some("foreign"));
        assert_eq!(rules[1].id.as_deref(), Some("2"));
        assert_eq!(rules[2].object_name_prefix.as_deref(), Some("changed/"));
        assert_eq!(
            rules[2].custom_attributes["storageEventSuffixes0"],
            r#"[".json"]"#
        );

        let mut removed = run
            .internal_state::<GcpStorageController>()
            .unwrap()
            .clone();
        removed.state = GcpStorageState::UpdatingNotifications;
        let mut run = SingleControllerExecutor::builder()
            .resource(Storage::new("uploads".into()).build())
            .controller(removed)
            .platform(Platform::Gcp)
            .service_provider(provider)
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert!(
            !run.internal_state::<GcpStorageController>()
                .unwrap()
                .notifications_managed
        );
        assert_eq!(
            serde_json::to_value(&*remote.lock().unwrap()).unwrap(),
            serde_json::to_value(vec![foreign]).unwrap()
        );
    }
}

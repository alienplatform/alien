//! Account-scoped subscriptions share Azure's system topic without owning it.
//! Re-read subscriptions after every checkpoint so a lost PUT/DELETE response is
//! recoverable. Each reconciliation step performs at most one durable mutation.

use crate::{
    core::{
        verifier::{self, HttpVerifier, ResourceWrite},
        ResourceControllerContext,
    },
    error::{ErrorData, Result},
    infra_requirements::azure_utils,
    queue::azure::AzureQueueController,
};
use alien_azure_clients::event_grid::{
    EventGridApi, EventSubscription, EventSubscriptionFilter, EventSubscriptionRequest,
    EventSubscriptionRequestProperties, ServiceBusQueueDestination,
    ServiceBusQueueDestinationProperties,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    storage_notifications::{azure_subscription_name, azure_subscription_prefix},
    ReleaseStatements, SignedStatement, Storage, StorageNotification,
};
use alien_error::{AlienError, Context, IntoAlienError};
use std::collections::BTreeMap;

fn subscription_prefix(container: &str) -> String {
    azure_subscription_prefix(container)
}

pub(super) fn subscription_name(container: &str, rule: &StorageNotification) -> String {
    azure_subscription_name(container, rule)
}

/// How a protected install sends an Event Grid subscription: the verifier checks it against
/// the signed stack and applies it unchanged, because where a storage account's events go is
/// who can read them.
pub(super) struct ViaVerifier<'a> {
    verifier: HttpVerifier,
    statements: &'a ReleaseStatements,
    config: Option<&'a SignedStatement>,
    account: &'a str,
}

async fn put_subscription(
    client: &dyn EventGridApi,
    via: Option<&ViaVerifier<'_>>,
    source: &str,
    name: String,
    rule: EventSubscriptionRequest,
    storage: &str,
    action: &str,
) -> Result<()> {
    match via {
        Some(via) => {
            let body = serde_json::to_value(&rule).into_alien_error().context(
                ErrorData::CloudPlatformError {
                    message: "The Event Grid subscription does not serialize".into(),
                    resource_id: Some(storage.to_string()),
                },
            )?;
            verifier::apply_resource_write(
                &via.verifier,
                via.statements,
                via.config,
                storage,
                vec![ResourceWrite::PutEventSubscription {
                    account: via.account.to_string(),
                    name,
                    body,
                }],
            )
            .await?;
            Ok(())
        }
        None => client
            .create_or_update_event_subscription(source.into(), name, rule)
            .await
            .map(|_| ())
            .context(cloud_error(storage, action)),
    }
}

pub(super) fn source_id(subscription: &str, group: &str, account: &str) -> String {
    format!("/subscriptions/{subscription}/resourceGroups/{group}/providers/Microsoft.Storage/storageAccounts/{account}")
}

fn owned(name: &str, container: &str) -> bool {
    name.strip_prefix(&subscription_prefix(container))
        .is_some_and(|key| key.len() == 16 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn cloud_error(storage: &str, action: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(storage.to_string()),
        message: format!("{action} Azure storage queue notifications"),
    }
}

/// A successful PUT/DELETE can still be running. Never overwrite it or report
/// convergence until the provider reports Succeeded (or deletion is absent).
/// Where an owned subscription is in its provisioning.
enum Provisioning {
    Succeeded,
    /// An operation is still running; wait for it.
    InProgress,
    /// The last create or update failed. Azure keeps the subscription, and it delivers
    /// nothing until it is replaced.
    Failed,
}

fn provisioning(subscription: &EventSubscription, storage: &str) -> Result<Provisioning> {
    match subscription
        .properties
        .as_ref()
        .and_then(|p| p.provisioning_state.as_deref())
    {
        Some("Succeeded") => Ok(Provisioning::Succeeded),
        Some("Creating" | "Updating" | "Deleting") => Ok(Provisioning::InProgress),
        Some("Failed") => Ok(Provisioning::Failed),
        state => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id: Some(storage.into()),
            message: format!(
                "Event Grid subscription {:?} has unexpected provisioning state {state:?}",
                subscription.name
            ),
        })),
    }
}

fn matches(subscription: &EventSubscription, desired: &EventSubscriptionRequest) -> bool {
    let Some(p) = &subscription.properties else {
        return false;
    };
    let Some(destination) = &p.destination else {
        return false;
    };
    let Some(filter) = &p.filter else {
        return false;
    };
    let d = &desired.properties;
    destination.get("endpointType").and_then(|v| v.as_str()) == Some("ServiceBusQueue")
        && destination
            .pointer("/properties/resourceId")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.eq_ignore_ascii_case(&d.destination.properties.resource_id))
        && filter.get("includedEventTypes")
            == Some(&serde_json::json!(d.filter.included_event_types))
        && filter.get("subjectBeginsWith").and_then(|v| v.as_str())
            == Some(d.filter.subject_begins_with.as_str())
        && filter
            .get("subjectEndsWith")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            == d.filter.subject_ends_with.as_deref().unwrap_or_default()
        && filter
            .get("isSubjectCaseSensitive")
            .and_then(|v| v.as_bool())
            == Some(true)
        && filter
            .get("advancedFilters")
            .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
        && p.event_delivery_schema.as_deref() == Some(d.event_delivery_schema.as_str())
}

pub(super) async fn reconcile(
    ctx: &ResourceControllerContext<'_>,
    container: &str,
    storage: &Storage,
    deleting: bool,
) -> Result<bool> {
    let cfg = ctx.get_azure_config()?;
    let group = azure_utils::get_resource_group_name(ctx.state)?;
    let account = azure_utils::get_storage_account_name(ctx.state)?;
    let source = source_id(&cfg.subscription_id, &group, &account);
    let client = ctx.service_provider.get_azure_event_grid_client(cfg)?;
    let mut desired = BTreeMap::new();
    if !deleting {
        for rule in &storage.notifications {
            let queue = ctx.require_dependency::<AzureQueueController>(&rule.queue)?;
            let (Some(namespace), Some(queue_name)) = (&queue.namespace_name, &queue.queue_name)
            else {
                return Err(AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: storage.id.clone(),
                    dependency_id: rule.queue.id.clone(),
                }));
            };
            desired.insert(subscription_name(container, rule), EventSubscriptionRequest {
                properties: EventSubscriptionRequestProperties {
                    destination: ServiceBusQueueDestination {
                        endpoint_type: "ServiceBusQueue".into(),
                        properties: ServiceBusQueueDestinationProperties {
                            resource_id: format!("/subscriptions/{}/resourceGroups/{group}/providers/Microsoft.ServiceBus/namespaces/{namespace}/queues/{queue_name}",cfg.subscription_id),
                        },
                    },
                    filter: EventSubscriptionFilter {
                        included_event_types: vec!["Microsoft.Storage.BlobCreated".into()],
                        subject_begins_with: format!("/blobServices/default/containers/{container}/blobs/{}",rule.prefix.as_deref().unwrap_or_default()),
                        subject_ends_with: rule.suffix.clone(),
                        is_subject_case_sensitive: true,
                    },
                    event_delivery_schema: "CloudEventSchemaV1_0".into(),
                },
            });
        }
    }
    let via = match verifier::protected_statements(ctx, &storage.id)? {
        Some(statements) => Some(ViaVerifier {
            verifier: verifier::azure_verifier(ctx).await?,
            statements,
            config: ctx.deployment_config.signed_config.as_ref(),
            account: &account,
        }),
        None => None,
    };
    reconcile_subscriptions()
        .client(client.as_ref())
        .maybe_via(via.as_ref())
        .source(&source)
        .container(container)
        .storage(&storage.id)
        .desired(desired)
        .deleting(deleting)
        .call()
        .await
}

#[bon::builder]
async fn reconcile_subscriptions(
    client: &dyn EventGridApi,
    via: Option<&ViaVerifier<'_>>,
    source: &str,
    container: &str,
    storage: &str,
    mut desired: BTreeMap<String, EventSubscriptionRequest>,
    deleting: bool,
) -> Result<bool> {
    let existing = match client.list_event_subscriptions(source.into()).await {
        Ok(subscriptions) => subscriptions,
        Err(error)
            if deleting
                && matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
        {
            return Ok(true)
        }
        Err(error) => return Err(error).context(cloud_error(storage, "List")),
    };
    let mut obsolete = Vec::new();
    for subscription in &existing {
        let Some(name) = subscription.name.as_ref() else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(storage.into()),
                message: "Event Grid returned a subscription without a name".into(),
            }));
        };
        if !owned(name, container) {
            continue;
        }
        match provisioning(subscription, storage)? {
            Provisioning::InProgress => return Ok(false),
            // Delete a failed subscription whether or not its rule is still wanted. A
            // wanted rule is reported as failed now and created afresh on the next attempt,
            // so a persistent provider failure surfaces instead of being retried in a loop.
            Provisioning::Failed => {
                client
                    .delete_event_subscription(source.into(), name.clone())
                    .await
                    .context(cloud_error(storage, "Delete failed"))?;
                if desired.contains_key(name) {
                    return Err(AlienError::new(ErrorData::CloudResourceFailed {
                        resource_id: storage.into(),
                        operation: format!("Event Grid subscription '{name}' provisioning"),
                        reason:
                            "Failed; the subscription was deleted and is created again on retry"
                                .into(),
                    }));
                }
                return Ok(false);
            }
            Provisioning::Succeeded => {}
        }
        if let Some(rule) = desired.remove(name) {
            if !matches(subscription, &rule) {
                put_subscription(client, via, source, name.clone(), rule, storage, "Update")
                    .await?;
                return Ok(false);
            }
        } else {
            obsolete.push(name);
        }
    }
    // Create before delete: a changed rule has a new name, so its new subscription delivers
    // (possibly alongside the old one) before the old one goes.
    if let Some((name, rule)) = desired.into_iter().next() {
        put_subscription(client, via, source, name, rule, storage, "Create").await?;
        return Ok(false);
    }
    if let Some(name) = obsolete.first() {
        client
            .delete_event_subscription(source.into(), (*name).clone())
            .await
            .context(cloud_error(storage, "Delete"))?;
        return Ok(false);
    }
    Ok(true)
}

/// Retention runs outside the controller scheduler. Repeat through its caller
/// until all owned subscriptions are gone; never delete the shared system topic.
/// Returns false while Azure is still deleting owned subscriptions.
pub(super) async fn remove_owned(
    client: &dyn EventGridApi,
    source: &str,
    container: &str,
    storage: &str,
) -> Result<bool> {
    reconcile_subscriptions()
        .client(client)
        .source(source)
        .container(container)
        .storage(storage)
        .desired(BTreeMap::new())
        .deleting(true)
        .call()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        controller_test::SingleControllerExecutor, core::MockPlatformServiceProvider,
        queue::azure::AzureQueueState, AzureStorageController, AzureStorageState,
    };
    use alien_azure_clients::event_grid::MockEventGridApi;
    use alien_core::{
        InitialSetupAuthority, Platform, Queue, ResourceLifecycle, ResourceStatus,
        StorageNotification,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn storage(prefix: &str) -> Storage {
        let queue = Queue::new("work".into()).build();
        Storage::new("uploads".into())
            .notifications(vec![
                StorageNotification::builder()
                    .queue((&queue).into())
                    .prefix(prefix.into())
                    .suffix(".json".into())
                    .build(),
                StorageNotification::builder()
                    .queue((&queue).into())
                    .prefix("other/".into())
                    .suffix(".data".into())
                    .build(),
            ])
            .build()
    }

    fn queue() -> AzureQueueController {
        AzureQueueController {
            state: AzureQueueState::Ready,
            namespace_name: Some("test-bus".into()),
            queue_name: Some("test-work".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn lost_response_recovers_then_update_and_removal_preserve_foreign_subscription() {
        let foreign: EventSubscription = serde_json::from_value(json!({
            "name":"independent-webhook", "properties":{"provisioningState":"Succeeded",
                "destination":{"endpointType":"WebHook","properties":{"endpointUrl":"https://example.com/events"}}}
        })).unwrap();
        let remote = Arc::new(Mutex::new(vec![foreign.clone()]));
        let mut mock = MockEventGridApi::new();
        let read = remote.clone();
        mock.expect_list_event_subscriptions().returning(move |_| {
            let mut actual = read.lock().unwrap();
            let found = actual.clone();
            for item in actual.iter_mut().skip(1) {
                item.properties.as_mut().unwrap().provisioning_state = Some("Succeeded".into());
            }
            Ok(found)
        });
        let writes = remote.clone();
        let mut calls = 0;
        mock.expect_create_or_update_event_subscription()
            .times(3)
            .returning(move |_, name, request| {
                calls += 1;
                let mut body = serde_json::to_value(request).unwrap();
                body["name"] = json!(name);
                body["properties"]["provisioningState"] = json!("Creating");
                let created: EventSubscription = serde_json::from_value(body).unwrap();
                let mut actual = writes.lock().unwrap();
                actual.retain(|item| item.name.as_deref() != Some(&name));
                actual.push(created.clone());
                if calls == 1 {
                    Err(AlienError::new(CloudClientErrorData::Timeout {
                        message: "PUT applied, response lost".into(),
                    }))
                } else {
                    Ok(created)
                }
            });
        let deletes = remote.clone();
        mock.expect_delete_event_subscription()
            .times(3)
            .returning(move |_, name| {
                assert_ne!(name, "independent-webhook");
                let mut actual = deletes.lock().unwrap();
                let before = actual.len();
                actual.retain(|item| item.name.as_deref() != Some(&name));
                assert_eq!(actual.len() + 1, before);
                Ok(())
            });
        let mock = Arc::new(mock);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_event_grid_client()
            .returning(move |_| Ok(mock.clone()));
        let provider = Arc::new(provider);
        let checkpoint = AzureStorageController {
            state: AzureStorageState::CreatingNotifications,
            container_name: Some("test-uploads".into()),
            storage_account_name: Some("test-account".into()),
            notifications_managed: true,
            ..Default::default()
        };
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(checkpoint.clone())
            .platform(Platform::Azure)
            .service_provider(provider.clone())
            .with_test_dependencies()
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal()
            .await
            .expect_err("lost PUT response is surfaced");
        assert_eq!(remote.lock().unwrap().len(), 2);
        let restored: AzureStorageController =
            serde_json::from_value(serde_json::to_value(checkpoint).unwrap()).unwrap();
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("in/"))
            .controller(restored)
            .platform(Platform::Azure)
            .service_provider(provider.clone())
            .with_test_dependencies()
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert_eq!(remote.lock().unwrap().len(), 3);
        assert!(!run.needs_update().unwrap());
        let mut changed = run
            .internal_state::<AzureStorageController>()
            .unwrap()
            .clone();
        changed.state = AzureStorageState::UpdatingNotifications;
        let mut run = SingleControllerExecutor::builder()
            .resource(storage("changed/"))
            .controller(changed)
            .platform(Platform::Azure)
            .service_provider(provider.clone())
            .with_test_dependencies()
            .with_dependency(Queue::new("work".into()).build(), queue())
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        let actual = remote.lock().unwrap().clone();
        assert_eq!(actual.len(), 3);
        assert_eq!(actual[0], foreign);
        // The changed rule got a new subscription; the old one is gone.
        let name = |storage: Storage| subscription_name("test-uploads", &storage.notifications[0]);
        assert!(!actual
            .iter()
            .any(|item| item.name.as_deref() == Some(&name(storage("in/")))));
        let first = actual
            .iter()
            .find(|item| item.name.as_deref() == Some(&name(storage("changed/"))))
            .unwrap();
        let filter = first.properties.as_ref().unwrap().filter.as_ref().unwrap();
        assert_eq!(
            filter["subjectBeginsWith"],
            "/blobServices/default/containers/test-uploads/blobs/changed/"
        );
        assert_eq!(filter["subjectEndsWith"], ".json");
        assert_eq!(filter["isSubjectCaseSensitive"], true);
        let mut removed = run
            .internal_state::<AzureStorageController>()
            .unwrap()
            .clone();
        removed.state = AzureStorageState::UpdatingNotifications;
        let mut run = SingleControllerExecutor::builder()
            .resource(Storage::new("uploads".into()).build())
            .controller(removed)
            .platform(Platform::Azure)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        assert_eq!(*remote.lock().unwrap(), vec![foreign]);
        assert!(
            !run.internal_state::<AzureStorageController>()
                .unwrap()
                .notifications_managed
        );
        assert!(!run.needs_update().unwrap());
    }

    #[tokio::test]
    async fn ordinary_storage_and_frozen_handoff_do_not_call_event_grid() {
        for (notifying, lifecycle) in [
            (false, ResourceLifecycle::Live),
            (true, ResourceLifecycle::Frozen),
        ] {
            let config = if notifying {
                storage("in/")
            } else {
                Storage::new("uploads".into()).build()
            };
            let mut run = SingleControllerExecutor::builder()
                .resource(config)
                .controller(AzureStorageController {
                    state: AzureStorageState::CreatingNotifications,
                    container_name: Some("test-uploads".into()),
                    ..Default::default()
                })
                .platform(Platform::Azure)
                .resource_lifecycle(lifecycle)
                .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .with_test_dependencies()
                .with_dependency(Queue::new("work".into()).build(), queue())
                .build()
                .await
                .unwrap();
            assert!(!run.needs_update().unwrap());
            run.run_until_terminal().await.unwrap();
            assert_eq!(run.status(), ResourceStatus::Running);
            assert!(!run.needs_update().unwrap());
        }
    }

    /// Azure keeps a subscription whose create or update failed. Removal deletes it; a rule
    /// that is still wanted is deleted and reported, then created afresh on the next pass.
    #[tokio::test]
    async fn failed_subscriptions_are_deleted_and_wanted_ones_are_recreated_on_retry() {
        let name = subscription_name("test-uploads", &storage("in/").notifications[0]);
        let rule: EventSubscriptionRequest = serde_json::from_value(json!({"properties":{
            "destination":{"endpointType":"ServiceBusQueue","properties":{"resourceId":"queue"}},
            "filter":{"includedEventTypes":["Microsoft.Storage.BlobCreated"],
                "subjectBeginsWith":"/blobServices/default/containers/test-uploads/blobs/in/",
                "isSubjectCaseSensitive":true},
            "eventDeliverySchema":"CloudEventSchemaV1_0"}}))
        .unwrap();
        let remote = Arc::new(Mutex::new(Vec::<EventSubscription>::new()));
        let failed = |name: &str| -> EventSubscription {
            serde_json::from_value(json!({"name":name,"properties":{"provisioningState":"Failed"}}))
                .unwrap()
        };
        let mut mock = MockEventGridApi::new();
        let read = remote.clone();
        mock.expect_list_event_subscriptions()
            .returning(move |_| Ok(read.lock().unwrap().clone()));
        let deleted = remote.clone();
        mock.expect_delete_event_subscription()
            .returning(move |_, name| {
                deleted
                    .lock()
                    .unwrap()
                    .retain(|item| item.name.as_deref() != Some(&name));
                Ok(())
            });
        let created = Arc::new(Mutex::new(Vec::new()));
        let sink = created.clone();
        mock.expect_create_or_update_event_subscription()
            .returning(move |_, name, _| {
                sink.lock().unwrap().push(name.clone());
                Ok(serde_json::from_value(
                    json!({"name":name,"properties":{"provisioningState":"Creating"}}),
                )
                .unwrap())
            });
        let run = |desired: BTreeMap<String, EventSubscriptionRequest>, deleting: bool| {
            reconcile_subscriptions()
                .client(&mock)
                .source("source")
                .container("test-uploads")
                .storage("uploads")
                .desired(desired)
                .deleting(deleting)
                .call()
        };

        // Removal: the failed owned subscription goes, a foreign failed one stays.
        *remote.lock().unwrap() = vec![failed("independent"), failed(&name)];
        assert!(!run(BTreeMap::new(), true).await.unwrap());
        assert!(run(BTreeMap::new(), true).await.unwrap());
        assert_eq!(*remote.lock().unwrap(), vec![failed("independent")]);

        // A wanted rule: delete and report, then create on the next pass.
        remote.lock().unwrap().push(failed(&name));
        let wanted = BTreeMap::from([(name.clone(), rule)]);
        let error = run(wanted.clone(), false)
            .await
            .expect_err("a failed subscription must be reported");
        assert_eq!(error.code, "CLOUD_RESOURCE_FAILED");
        assert!(!error.retryable);
        assert_eq!(*remote.lock().unwrap(), vec![failed("independent")]);
        assert!(created.lock().unwrap().is_empty());
        assert!(!run(wanted, false).await.unwrap());
        assert_eq!(*created.lock().unwrap(), vec![name]);
    }

    /// Subscriptions are named by what their rule delivers, not its position: inserting a
    /// rule first and reordering the rest only creates the new one, and a changed rule's new
    /// subscription is created before the old one is deleted.
    #[tokio::test]
    async fn unchanged_rules_survive_edits_and_changed_rules_are_replaced_create_first() {
        let rule = |prefix: &str, suffix: &str| {
            StorageNotification::builder()
                .queue((&Queue::new("work".into()).build()).into())
                .prefix(prefix.into())
                .suffix(suffix.into())
                .build()
        };
        let request = |name: &str| -> EventSubscription {
            serde_json::from_value(
                json!({"name":name, "properties":{"provisioningState":"Succeeded"}}),
            )
            .unwrap()
        };
        let log = Arc::new(Mutex::new(Vec::new()));
        let remote = Arc::new(Mutex::new(Vec::<EventSubscription>::new()));
        let mut mock = MockEventGridApi::new();
        let read = remote.clone();
        mock.expect_list_event_subscriptions()
            .returning(move |_| Ok(read.lock().unwrap().clone()));
        let (writes, created) = (remote.clone(), log.clone());
        mock.expect_create_or_update_event_subscription()
            .returning(move |_, name, body| {
                created.lock().unwrap().push(format!("create {name}"));
                let mut value = serde_json::to_value(body).unwrap();
                value["name"] = json!(name);
                value["properties"]["provisioningState"] = json!("Succeeded");
                let subscription: EventSubscription = serde_json::from_value(value).unwrap();
                writes.lock().unwrap().push(subscription.clone());
                Ok(subscription)
            });
        let (deletes, deleted) = (remote.clone(), log.clone());
        mock.expect_delete_event_subscription()
            .returning(move |_, name| {
                deleted.lock().unwrap().push(format!("delete {name}"));
                deletes
                    .lock()
                    .unwrap()
                    .retain(|item| item.name.as_deref() != Some(&name));
                Ok(())
            });
        let name = |rule: &StorageNotification| subscription_name("test-uploads", rule);
        let run = |rules: Vec<StorageNotification>| {
            let mut desired = BTreeMap::new();
            for rule in &rules {
                desired.insert(
                    name(rule),
                    serde_json::from_value(json!({"properties":{
                        "destination":{"endpointType":"ServiceBusQueue","properties":{"resourceId":"queue"}},
                        "filter":{"includedEventTypes":["Microsoft.Storage.BlobCreated"],
                            "subjectBeginsWith":format!("/blobServices/default/containers/test-uploads/blobs/{}", rule.prefix.clone().unwrap()),
                            "subjectEndsWith":rule.suffix.clone(), "isSubjectCaseSensitive":true},
                        "eventDeliverySchema":"CloudEventSchemaV1_0"}}))
                    .unwrap(),
                );
            }
            desired
        };
        let converge = |desired: BTreeMap<String, EventSubscriptionRequest>| {
            let mock = &mock;
            async move {
                for _ in 0..10 {
                    if reconcile_subscriptions()
                        .client(mock)
                        .source("source")
                        .container("test-uploads")
                        .storage("uploads")
                        .desired(desired.clone())
                        .deleting(false)
                        .call()
                        .await
                        .unwrap()
                    {
                        return;
                    }
                }
                panic!("did not converge");
            }
        };
        let (a, b) = (rule("in/", ".json"), rule("logs/", ".csv"));
        converge(run(vec![a.clone(), b.clone()])).await;
        remote.lock().unwrap().push(request("independent"));
        log.lock().unwrap().clear();

        let c = rule("new/", ".bin");
        converge(run(vec![c.clone(), b.clone(), a.clone()])).await;
        assert_eq!(*log.lock().unwrap(), [format!("create {}", name(&c))]);
        log.lock().unwrap().clear();

        let changed = rule("logs/", ".txt");
        converge(run(vec![c.clone(), changed.clone(), a.clone()])).await;
        assert_eq!(
            *log.lock().unwrap(),
            [
                format!("create {}", name(&changed)),
                format!("delete {}", name(&b))
            ]
        );
        let names: Vec<_> = remote
            .lock()
            .unwrap()
            .iter()
            .filter_map(|item| item.name.clone())
            .collect();
        assert_eq!(
            names,
            [name(&a), "independent".into(), name(&c), name(&changed)]
        );
        assert!(names.iter().all(|name| name.len() <= 64));
    }

    #[tokio::test]
    async fn failed_delete_is_not_cleanup_success() {
        let name = subscription_name("test-uploads", &storage("in/").notifications[0]);
        let mut mock = MockEventGridApi::new();
        mock.expect_list_event_subscriptions().returning(move |_| {
            Ok(vec![serde_json::from_value(
                json!({"name":name,"properties":{"provisioningState":"Succeeded"}}),
            )
            .unwrap()])
        });
        mock.expect_delete_event_subscription()
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "event subscription".into(),
                    resource_name: "rule".into(),
                }))
            });
        let error = remove_owned(&mock, "source", "test-uploads", "uploads")
            .await
            .expect_err("denied delete must fail");
        assert!(error.to_string().contains("Delete"));
    }

    /// The verifier checks the body as the cloud client serialises it, so pin the shape the
    /// verifier's rule expects (its own test builds the same document).
    #[test]
    fn the_subscription_serialises_as_the_verifier_checks_it() {
        let request = EventSubscriptionRequest {
            properties: EventSubscriptionRequestProperties {
                destination: ServiceBusQueueDestination {
                    endpoint_type: "ServiceBusQueue".into(),
                    properties: ServiceBusQueueDestinationProperties {
                        resource_id: "/queue".into(),
                    },
                },
                filter: EventSubscriptionFilter {
                    included_event_types: vec!["Microsoft.Storage.BlobCreated".into()],
                    subject_begins_with: "/begin".into(),
                    subject_ends_with: Some(".json".into()),
                    is_subject_case_sensitive: true,
                },
                event_delivery_schema: "CloudEventSchemaV1_0".into(),
            },
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"properties": {
                "destination": {"endpointType": "ServiceBusQueue", "properties": {"resourceId": "/queue"}},
                "filter": {
                    "includedEventTypes": ["Microsoft.Storage.BlobCreated"],
                    "subjectBeginsWith": "/begin",
                    "subjectEndsWith": ".json",
                    "isSubjectCaseSensitive": true,
                },
                "eventDeliverySchema": "CloudEventSchemaV1_0",
            }})
        );
    }
}

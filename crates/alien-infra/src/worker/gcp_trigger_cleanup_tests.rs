use super::{GcpWorkerController, GcpWorkerState, GcsNotificationTracker};
use crate::core::controller_test::SingleControllerExecutor;
use crate::core::{MockPlatformServiceProvider, StackExecutor, StackResourceStateExt};
use crate::worker::fixtures::function_public_ingress;
use alien_client_core::ErrorData as ClientError;
use alien_core::{
    ClientConfig, GcpClientConfig, Platform, ResourceLifecycle, ResourceStatus, Stack,
};
use alien_error::AlienError;
use alien_gcp_clients::GcpClientConfigExt;
use alien_gcp_clients::cloudscheduler::MockCloudSchedulerApi;
use alien_gcp_clients::gcs::{GcsNotification, ListNotificationsResponse, MockGcsApi};
use crate::worker::gcp_notification_identity::GcpNotificationIntent;
use alien_gcp_clients::pubsub::MockPubSubApi;
use rstest::rstest;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug)]
enum Child {
    Notification,
    Subscription,
    Topic,
    Scheduler,
}

fn provider(
    child: Child,
    denied: bool,
    retry: bool,
    calls: Arc<Mutex<Vec<String>>>,
) -> Arc<MockPlatformServiceProvider> {
    let remove = move |name: String| {
        calls.lock().unwrap().push(name.clone());
        if retry {
            return Err(AlienError::new(ClientError::RemoteResourceNotFound {
                resource_type: "trigger".into(),
                resource_name: name,
            }));
        }
        if name == "first" {
            return Ok(());
        }
        Err(AlienError::new(if denied {
            ClientError::RemoteAccessDenied {
                resource_type: "trigger".into(),
                resource_name: name,
            }
        } else {
            ClientError::RemoteServiceUnavailable {
                message: "delete reply interrupted".into(),
            }
        }))
    };
    let mut provider = MockPlatformServiceProvider::new();
    match child {
        Child::Notification => {
            let mut client = MockGcsApi::new();
            client
                .expect_delete_notification()
                .returning(move |bucket, name| {
                    assert_eq!(bucket, "owned-bucket");
                    remove(name)
                });
            let client = Arc::new(client);
            provider
                .expect_get_gcp_gcs_client()
                .returning(move |_| Ok(client.clone()));
        }
        Child::Subscription => {
            let mut client = MockPubSubApi::new();
            client.expect_delete_subscription().returning(remove);
            let client = Arc::new(client);
            provider
                .expect_get_gcp_pubsub_client()
                .returning(move |_| Ok(client.clone()));
        }
        Child::Topic => {
            let mut client = MockPubSubApi::new();
            client.expect_delete_topic().returning(remove);
            let client = Arc::new(client);
            provider
                .expect_get_gcp_pubsub_client()
                .returning(move |_| Ok(client.clone()));
        }
        Child::Scheduler => {
            let mut client = MockCloudSchedulerApi::new();
            client.expect_delete_job().returning(remove);
            let client = Arc::new(client);
            provider
                .expect_get_gcp_cloud_scheduler_client()
                .returning(move |_| Ok(client.clone()));
        }
    }
    Arc::new(provider)
}

#[rstest]
#[case(Child::Subscription, false)]
#[case(Child::Subscription, true)]
#[case(Child::Notification, false)]
#[case(Child::Notification, true)]
#[case(Child::Topic, false)]
#[case(Child::Topic, true)]
#[case(Child::Scheduler, false)]
#[case(Child::Scheduler, true)]
#[tokio::test]
async fn failed_trigger_delete_preserves_remaining_child_across_reload(
    #[case] child: Child,
    #[case] denied: bool,
    #[values(false, true)] updating: bool,
) {
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = if updating {
        GcpWorkerState::UpdatePushSubscriptions
    } else {
        match child {
            Child::Scheduler => GcpWorkerState::DeletingSchedulerJobs,
            _ => GcpWorkerState::DeletingPushSubscriptions,
        }
    };
    let mut previous = function_public_ingress();
    previous.triggers.push(alien_core::WorkerTrigger::Schedule {
        cron: "* * * * *".into(),
    });
    for name in ["first", "second"] {
        match child {
            Child::Notification => controller
                .gcs_notification_ids
                .push(GcsNotificationTracker {
                    bucket_name: "owned-bucket".into(),
                    notification_id: name.into(),
                }),
            Child::Subscription => controller.push_subscriptions.push(name.into()),
            Child::Topic => controller.storage_notification_topics.push(name.into()),
            Child::Scheduler => controller.scheduler_job_names.push(name.into()),
        }
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut executor = SingleControllerExecutor::builder()
        .resource(function_public_ingress())
        .previous_resource(previous.clone())
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(provider(child, denied, false, calls.clone()))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    assert!(executor.step().await.is_err());
    assert_eq!(*calls.lock().unwrap(), ["first", "second"]);
    let saved =
        serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap();
    let controller: GcpWorkerController = serde_json::from_value(saved).unwrap();
    let remaining = match child {
        Child::Notification => controller
            .gcs_notification_ids
            .iter()
            .map(|v| v.notification_id.clone())
            .collect::<Vec<_>>(),
        Child::Subscription => controller.push_subscriptions.clone(),
        Child::Topic => controller.storage_notification_topics.clone(),
        Child::Scheduler => controller.scheduler_job_names.clone(),
    };
    assert_eq!(remaining, ["second"]);
    let retry_calls = Arc::new(Mutex::new(Vec::new()));
    let mut resumed = SingleControllerExecutor::builder()
        .resource(function_public_ingress())
        .previous_resource(previous.clone())
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(provider(child, denied, !updating, retry_calls.clone()))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    if updating {
        let error = resumed.step().await.unwrap_err();
        assert_eq!(error.code, "RESOURCE_CLEANUP_INCOMPLETE");
        assert_eq!(*retry_calls.lock().unwrap(), ["second"]);
        // No create expectations exist: a failed old-trigger deletion must not
        // reach replacement creation, including after serialization and retry.
        assert_eq!(
            resumed
                .internal_state::<GcpWorkerController>()
                .unwrap()
                .state,
            GcpWorkerState::UpdatePushSubscriptions
        );
        return;
    }
    resumed.step().await.unwrap();
    assert_eq!(*retry_calls.lock().unwrap(), ["second"]);
    let controller = resumed.internal_state::<GcpWorkerController>().unwrap();
    assert!(controller.push_subscriptions.is_empty());
    assert!(controller.gcs_notification_ids.is_empty());
    assert!(controller.storage_notification_topics.is_empty());
    assert!(controller.scheduler_job_names.is_empty());
    assert_eq!(
        controller.state,
        match child {
            Child::Scheduler => GcpWorkerState::DeletingCommandsInfrastructure,
            _ => GcpWorkerState::DeletingSchedulerJobs,
        }
    );
}

#[rstest]
#[case(Child::Subscription, false)]
#[case(Child::Subscription, true)]
#[case(Child::Notification, false)]
#[case(Child::Notification, true)]
#[case(Child::Topic, false)]
#[case(Child::Topic, true)]
#[case(Child::Scheduler, false)]
#[case(Child::Scheduler, true)]
#[tokio::test]
async fn stack_delete_retains_denied_trigger(#[case] child: Child, #[case] replacing: bool) {
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = match child {
        Child::Scheduler => GcpWorkerState::DeletingSchedulerJobs,
        _ => GcpWorkerState::DeletingPushSubscriptions,
    };
    match child {
        Child::Notification => controller
            .gcs_notification_ids
            .push(GcsNotificationTracker {
                bucket_name: "owned-bucket".into(),
                notification_id: "second".into(),
            }),
        Child::Subscription => controller.push_subscriptions.push("second".into()),
        Child::Topic => controller.storage_notification_topics.push("second".into()),
        Child::Scheduler => controller.scheduler_job_names.push("second".into()),
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let services = provider(child, true, false, calls.clone());
    let fixture = SingleControllerExecutor::builder()
        .resource(function_public_ingress())
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(services.clone())
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    let mut state = fixture.stack_state().clone();
    state.resources.retain(|id, _| id == "public-func");
    state
        .resources
        .get_mut("public-func")
        .unwrap()
        .dependencies
        .clear();
    let desired = if replacing {
        Stack::new("replace-worker".into())
            .add(function_public_ingress(), ResourceLifecycle::Live)
            .build()
    } else {
        Stack::new("remove-worker".into()).build()
    };
    let config = alien_core::DeploymentConfig::builder()
        .stack_settings(alien_core::StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let executor = StackExecutor::builder(
        &desired,
        ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
    )
    .deployment_config(&config)
    .service_provider(services)
    .build()
    .unwrap();
    let next = executor.step(state).await.unwrap().next_state;
    assert_eq!(*calls.lock().unwrap(), ["second"]);
    assert_eq!(
        next.resources["public-func"].status,
        ResourceStatus::DeleteFailed
    );
    let error = next.resources["public-func"].error.as_ref().unwrap();
    assert_eq!(
        error.code,
        if replacing {
            "REPLACE_DELETE_DENIED"
        } else {
            "RESOURCE_CLEANUP_INCOMPLETE"
        }
    );
    assert!(
        std::iter::successors(Some(error), |source| source.source.as_deref())
            .any(|source| source.code == "REMOTE_ACCESS_DENIED")
    );
    if replacing {
        assert!(executor.plan(&next).unwrap().replaces.is_empty());
    }
    let serialized = serde_json::to_vec(&next).unwrap();
    let mut restored: alien_core::StackState = serde_json::from_slice(&serialized).unwrap();
    let saved = restored.resources.get_mut("public-func").unwrap();
    assert!(saved.error.is_some());
    assert!(saved.retry_failed().unwrap());
    let controller = saved
        .get_internal_controller_typed::<GcpWorkerController>()
        .unwrap();
    match child {
        Child::Notification => {
            assert_eq!(controller.gcs_notification_ids[0].notification_id, "second")
        }
        Child::Subscription => assert_eq!(controller.push_subscriptions, ["second"]),
        Child::Topic => assert_eq!(controller.storage_notification_topics, ["second"]),
        Child::Scheduler => assert_eq!(controller.scheduler_job_names, ["second"]),
    }
    let retry_calls = Arc::new(Mutex::new(Vec::new()));
    let retry = StackExecutor::builder(
        &desired,
        ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
    )
    .deployment_config(&config)
    .service_provider(provider(child, true, true, retry_calls.clone()))
    .build()
    .unwrap();
    let next = retry.step(restored).await.unwrap().next_state;
    assert_eq!(*retry_calls.lock().unwrap(), ["second"]);
    let controller = next.resources["public-func"]
        .get_internal_controller_typed::<GcpWorkerController>()
        .unwrap();
    assert!(controller.push_subscriptions.is_empty());
    assert!(controller.gcs_notification_ids.is_empty());
    assert!(controller.storage_notification_topics.is_empty());
    assert!(controller.scheduler_job_names.is_empty());
}

#[tokio::test]
async fn trigger_update_restart_does_not_retire_newly_created_jobs() {
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let created = Arc::new(Mutex::new(Vec::new()));
    let delete_calls = deleted.clone();
    let create_calls = created.clone();
    let mut scheduler = MockCloudSchedulerApi::new();
    scheduler.expect_delete_job().returning(move |name| {
        delete_calls.lock().unwrap().push(name);
        Ok(())
    });
    scheduler.expect_get_job().returning(|name| Err(AlienError::new(ClientError::RemoteResourceNotFound {
        resource_type: "scheduler job".into(), resource_name: name,
    })));
    scheduler
        .expect_create_job()
        .returning(move |region, name, mut job| {
            job.name = Some(format!("projects/{}/locations/{region}/jobs/{name}", GcpClientConfig::mock().project_id));
            let mut calls = create_calls.lock().unwrap();
            calls.push(name);
            if calls.len() == 2 {
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "second create interrupted".into(),
                }))
            } else {
                Ok(job)
            }
        });
    let scheduler = Arc::new(scheduler);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_cloud_scheduler_client()
        .returning(move |_| Ok(scheduler.clone()));
    let services = Arc::new(services);
    let mut desired = function_public_ingress();
    desired.triggers = vec![
        alien_core::WorkerTrigger::Schedule {
            cron: "* * * * *".into(),
        },
        alien_core::WorkerTrigger::Schedule {
            cron: "*/5 * * * *".into(),
        },
    ];
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::UpdatePushSubscriptions;
    controller.scheduler_job_names = vec!["old-job".into()];
    for step in 0..6 {
        let mut executor = SingleControllerExecutor::builder()
            .resource(desired.clone())
            .previous_resource(function_public_ingress())
            .controller(controller)
            .platform(Platform::Gcp)
            .service_provider(services.clone())
            .with_test_dependencies()
            .build()
            .await
            .unwrap();
        let result = executor.step().await;
        if step == 3 {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        controller = serde_json::from_value(
            serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(*deleted.lock().unwrap(), ["old-job"]);
        match step {
            0 => assert_eq!(
                controller.state,
                GcpWorkerState::UpdateCreatingPushSubscriptions
            ),
            1 => assert_eq!(
                controller.state,
                GcpWorkerState::UpdateCreatingSchedulerJobs
            ),
            2 => {
                assert!(created.lock().unwrap().is_empty());
                assert!(controller.scheduler_job_names.is_empty());
                assert_eq!(controller.scheduler_job_intents.len(), 2);
                assert_eq!(controller.state, GcpWorkerState::UpdateCreatingSchedulerJobs);
            }
            3 => {
                assert_eq!(controller.scheduler_job_names.len(), 1);
                assert_eq!(
                    controller.state,
                    GcpWorkerState::UpdateCreatingSchedulerJobs
                );
            }
            4 => {
                assert_eq!(controller.scheduler_job_names.len(), 2);
                assert_eq!(controller.state, GcpWorkerState::UpdateCreatingCommandsInfrastructure);
            }
            _ => {
                assert_eq!(controller.scheduler_job_names.len(), 2);
                assert_eq!(controller.state, GcpWorkerState::UpdateSettingIamPolicy);
            }
        }
    }
    let calls = created.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls[0].ends_with("-cron-0"));
    assert!(calls[1].ends_with("-cron-1"));
    assert_eq!(calls[1], calls[2]);
}

#[rstest]
#[case(GcpWorkerState::UpdateCreatingPushSubscriptions)]
#[case(GcpWorkerState::UpdateCreatingSchedulerJobs)]
fn trigger_update_checkpoint_envelope_preserves_phase_and_receipts(
    #[case] state: GcpWorkerState,
) {
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = state;
    controller.push_subscriptions = vec!["projects/test/subscriptions/confirmed".into()];
    controller.scheduler_job_names = vec!["projects/test/locations/us-central1/jobs/confirmed".into()];
    let saved = crate::serialize_controller(&controller).unwrap();
    assert_eq!(saved["_controllerStateVersion"], 6);
    let restored = crate::deserialize_controller(saved).unwrap();
    let restored = restored.as_any().downcast_ref::<GcpWorkerController>().unwrap();
    assert_eq!(restored.state, controller.state);
    assert_eq!(restored.push_subscriptions, controller.push_subscriptions);
    assert_eq!(restored.scheduler_job_names, controller.scheduler_job_names);
}

#[tokio::test]
async fn uncertain_scheduler_create_is_retired_after_controller_reload() {
    let native = Arc::new(Mutex::new(None::<alien_gcp_clients::cloudscheduler::SchedulerJob>));
    let reads = native.clone();
    let writes = native.clone();
    let deletes = native.clone();
    let mut scheduler = MockCloudSchedulerApi::new();
    scheduler.expect_get_job().returning(move |name| {
        match reads.lock().unwrap().clone() {
            Some(job) => { assert_eq!(job.name.as_deref(), Some(name.as_str())); Ok(job) }
            None => Err(AlienError::new(ClientError::RemoteResourceNotFound {
                resource_type: "scheduler job".into(), resource_name: name,
            })),
        }
    });
    scheduler.expect_create_job().times(1).returning(move |region, name, mut job| {
        job.name = Some(format!("projects/{}/locations/{region}/jobs/{name}", GcpClientConfig::mock().project_id));
        *writes.lock().unwrap() = Some(job);
        Err(AlienError::new(ClientError::RemoteServiceUnavailable {
            message: "accepted create with lost reply".into(),
        }))
    });
    scheduler.expect_delete_job().times(1).returning(move |name| {
        let job = deletes.lock().unwrap().take().unwrap();
        assert_eq!(job.name.as_deref(), Some(name.as_str()));
        Ok(())
    });
    let scheduler = Arc::new(scheduler);
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_cloud_scheduler_client().returning(move |_| Ok(scheduler.clone()));
    let services = Arc::new(services);
    let mut desired = function_public_ingress();
    desired.triggers = vec![alien_core::WorkerTrigger::Schedule { cron: "* * * * *".into() }];
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::CreatingSchedulerJobs;
    for step in 0..3 {
        if step == 2 { controller.state = GcpWorkerState::DeletingSchedulerJobs; }
        let mut executor = SingleControllerExecutor::builder()
            .resource(desired.clone()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
        let result = executor.step().await;
        if step == 1 { assert!(result.is_err()); } else { result.unwrap(); }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        assert!(controller.scheduler_job_names.is_empty());
        match step {
            0 => { assert!(native.lock().unwrap().is_none()); assert_eq!(controller.scheduler_job_intents.len(), 1); }
            1 => { assert!(native.lock().unwrap().is_some()); assert_eq!(controller.scheduler_job_intents.len(), 1); }
            _ => { assert!(native.lock().unwrap().is_none(), "uncertain accepted create must be retired"); assert!(controller.scheduler_job_intents.is_empty()); }
        }
    }
}

#[tokio::test]
async fn uncertain_storage_topic_create_is_retired_after_controller_reload() {
    let native = Arc::new(Mutex::new(None::<alien_gcp_clients::pubsub::Topic>));
    let reads = native.clone();
    let writes = native.clone();
    let deletes = native.clone();
    let mut pubsub = MockPubSubApi::new();
    pubsub.expect_get_topic().returning(move |name| {
        reads.lock().unwrap().clone().ok_or_else(|| AlienError::new(ClientError::RemoteResourceNotFound {
            resource_type: "topic".into(), resource_name: name,
        }))
    });
    pubsub.expect_create_topic().times(1).returning(move |name, mut topic| {
        topic.name = Some(format!("projects/{}/topics/{name}", GcpClientConfig::mock().project_id));
        *writes.lock().unwrap() = Some(topic);
        Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted topic create with lost reply".into() }))
    });
    pubsub.expect_delete_topic().times(1).returning(move |name| {
        let topic = deletes.lock().unwrap().take().unwrap();
        assert!(topic.name.unwrap().ends_with(&format!("/topics/{name}")));
        Ok(())
    });
    pubsub.expect_set_topic_iam_policy().never();
    let pubsub = Arc::new(pubsub);
    let gcs = Arc::new(MockGcsApi::new());
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    services.expect_get_gcp_gcs_client().returning(move |_| Ok(gcs.clone()));
    let services = Arc::new(services);
    let mut desired = function_public_ingress();
    desired.triggers = vec![alien_core::WorkerTrigger::Storage {
        storage: (&crate::core::controller_test::test_storage_1()).into(), events: vec!["created".into()],
    }];
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::CreatingPushSubscriptions;
    for step in 0..3 {
        if step == 2 { controller.state = GcpWorkerState::DeletingPushSubscriptions; }
        let mut executor = SingleControllerExecutor::builder()
            .resource(desired.clone()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
        let result = executor.step().await;
        if step == 1 { assert!(result.is_err()); } else { result.unwrap(); }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        assert!(controller.storage_notification_topics.is_empty());
        match step {
            0 => { assert!(native.lock().unwrap().is_none()); assert_eq!(controller.storage_topic_intents.len(), 1); }
            1 => { assert!(native.lock().unwrap().is_some()); assert_eq!(controller.storage_topic_intents.len(), 1); }
            _ => { assert!(native.lock().unwrap().is_none()); assert!(controller.storage_topic_intents.is_empty()); }
        }
    }
}

#[rstest]
#[case(Child::Subscription)]
#[case(Child::Notification)]
#[case(Child::Topic)]
#[case(Child::Scheduler)]
#[tokio::test]
async fn absent_service_does_not_discard_retained_trigger_cleanup(#[case] child: Child) {
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::DeleteStart;
    controller.service_name = None;
    match child {
        Child::Subscription => controller.push_subscriptions.push("recorded-subscription".into()),
        Child::Notification => controller.gcs_notification_ids.push(GcsNotificationTracker { bucket_name: "recorded-bucket".into(), notification_id: "recorded-id".into() }),
        Child::Topic => controller.storage_notification_topics.push("recorded-topic".into()),
        Child::Scheduler => controller.scheduler_job_names.push("recorded-job".into()),
    }
    let original = serde_json::to_value(&controller).unwrap();
    let mut executor = SingleControllerExecutor::builder()
        .resource(function_public_ingress()).controller(controller).platform(Platform::Gcp)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let restored: GcpWorkerController = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
    assert_eq!(restored.state, GcpWorkerState::DeletingPushSubscriptions);
    let restored = serde_json::to_value(restored).unwrap();
    for field in ["pushSubscriptions", "gcsNotificationIds", "storageNotificationTopics", "schedulerJobNames"] {
        assert!(original[field].is_array(), "Expected recorded child collection {field}");
        assert_eq!(restored[field], original[field]);
    }
}

#[tokio::test]
async fn uncertain_queue_subscription_is_retired_after_controller_reload() {
    let native = Arc::new(Mutex::new(None::<alien_gcp_clients::pubsub::Subscription>));
    let reads = native.clone(); let writes = native.clone(); let deletes = native.clone();
    let mut pubsub = MockPubSubApi::new();
    pubsub.expect_get_subscription().returning(move |name| reads.lock().unwrap().clone().ok_or_else(|| AlienError::new(ClientError::RemoteResourceNotFound { resource_type: "subscription".into(), resource_name: name })));
    pubsub.expect_create_subscription().times(1).returning(move |name, mut subscription| {
        subscription.name = Some(format!("projects/{}/subscriptions/{name}", GcpClientConfig::mock().project_id)); *writes.lock().unwrap() = Some(subscription);
        Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted subscription with lost reply".into() }))
    });
    pubsub.expect_delete_subscription().times(1).returning(move |name| { assert!(deletes.lock().unwrap().take().unwrap().name.unwrap().ends_with(&format!("/subscriptions/{name}"))); Ok(()) });
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone())); let services = Arc::new(services);
    let desired = crate::worker::fixtures::function_with_queue_trigger();
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::CreatingPushSubscriptions;
    for step in 0..3 {
        if step == 2 { controller.state = GcpWorkerState::DeletingPushSubscriptions; }
        let queue = crate::queue::gcp::GcpQueueController { state: crate::queue::gcp::GcpQueueState::Ready, topic_name: Some("test-test-queue".into()), subscription_name: Some("test-test-queue-sub".into()), _internal_stay_count: None };
        let mut executor = SingleControllerExecutor::builder().resource(desired.clone()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().with_dependency(crate::worker::fixtures::test_queue(), queue).build().await.unwrap();
        let result = executor.step().await; if step == 1 { assert!(result.is_err()); } else { result.unwrap(); }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        assert!(controller.push_subscriptions.is_empty());
        match step {
            0 => { assert!(native.lock().unwrap().is_none()); assert_eq!(controller.push_subscription_intents.len(), 1); }
            1 => { assert!(native.lock().unwrap().is_some()); assert_eq!(controller.push_subscription_intents.len(), 1); }
            _ => { assert!(native.lock().unwrap().is_none()); assert!(controller.push_subscription_intents.is_empty()); }
        }
    }
}

#[tokio::test]
async fn pending_notification_without_confirmed_id_is_cleaned_after_controller_reload() {
    let project = GcpClientConfig::mock().project_id;
    let topic = "test-public-func-storage-notif";
    let intent = GcpNotificationIntent::new("owned-bucket".into(), GcsNotification {
        topic: Some(format!("//pubsub.googleapis.com/projects/{project}/topics/{topic}")),
        event_types: vec!["OBJECT_FINALIZE".into()], payload_format: Some("JSON_API_V1".into()),
        ..Default::default()
    });
    let native = Arc::new(Mutex::new(Vec::<GcsNotification>::new()));
    let reads = native.clone(); let writes = native.clone(); let deletes = native.clone();
    let mut client = MockGcsApi::new();
    client.expect_list_notifications().returning(move |bucket| {
        assert_eq!(bucket, "owned-bucket");
        Ok(ListNotificationsResponse { items: reads.lock().unwrap().clone(), ..Default::default() })
    });
    client.expect_insert_notification().times(1).returning(move |_, mut notification| {
        notification.id = Some("123".into()); writes.lock().unwrap().push(notification);
        Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted notification create, lost reply".into() }))
    });
    client.expect_delete_notification().times(1).returning(move |bucket, id| {
        assert_eq!(bucket, "owned-bucket"); assert_eq!(id, "123"); deletes.lock().unwrap().clear(); Ok(())
    });
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.storage_notification_intents.insert(topic.into(), intent);
    let saved = serde_json::to_value(&controller).unwrap();
    assert!(controller.storage_notification_intents[topic].create_or_recover(&client, "test-worker").await.is_err());
    let mut controller: GcpWorkerController = serde_json::from_value(saved).unwrap();
    controller.state = GcpWorkerState::DeletingPushSubscriptions;
    assert!(controller.gcs_notification_ids.is_empty());
    let client = Arc::new(client);
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_gcs_client().returning(move |_| Ok(client.clone()));
    let mut executor = SingleControllerExecutor::builder()
        .resource(function_public_ingress()).controller(controller).platform(Platform::Gcp)
        .service_provider(Arc::new(services)).with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert!(controller.storage_notification_intents.is_empty());
    assert!(native.lock().unwrap().is_empty());
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn changed_schedule_after_intent_checkpoint_refuses_cloud_mutation(#[case] removed: bool) {
    let mut scheduler = MockCloudSchedulerApi::new();
    scheduler.expect_get_job().never();
    scheduler.expect_create_job().never();
    let scheduler = Arc::new(scheduler);
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_cloud_scheduler_client().returning(move |_| Ok(scheduler.clone()));
    let services = Arc::new(services);
    let mut desired = function_public_ingress();
    desired.triggers = vec![alien_core::WorkerTrigger::Schedule { cron: "* * * * *".into() }];
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::CreatingSchedulerJobs;
    let mut executor = SingleControllerExecutor::builder()
        .resource(desired.clone()).controller(controller).platform(Platform::Gcp)
        .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let controller: GcpWorkerController = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
    assert_eq!(controller.scheduler_job_intents.len(), 1);
    desired.triggers = if removed { vec![] } else { vec![alien_core::WorkerTrigger::Schedule { cron: "0 * * * *".into() }] };
    let mut executor = SingleControllerExecutor::builder()
        .resource(desired).controller(controller).platform(Platform::Gcp)
        .service_provider(services).with_test_dependencies().build().await.unwrap();
    let error = executor.step().await.expect_err("stale pending schedule must be refused before cloud mutation");
    assert!(format!("{error:?}").contains("RESOURCE_CONFIG_INVALID"));
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert_eq!(controller.scheduler_job_intents.len(), 1);
    assert!(controller.scheduler_job_names.is_empty());
}

#[tokio::test]
async fn notification_creation_checkpoints_before_insert_and_cleans_lost_reply() {
    let storage = crate::core::controller_test::test_storage_1();
    let mut desired = function_public_ingress();
    let topic = format!("test-{}-{}-notif", desired.id, storage.id);
    desired.triggers = vec![alien_core::WorkerTrigger::Storage {
        storage: (&storage).into(), events: vec!["created".into()],
    }];
    let native = Arc::new(Mutex::new(Vec::<GcsNotification>::new()));
    let reads = native.clone(); let writes = native.clone(); let deletes = native.clone();
    let mut gcs = MockGcsApi::new();
    gcs.expect_get_project_service_account().times(2).returning(|| Ok(alien_gcp_clients::gcs::ProjectServiceAccount {
        email_address: "storage@test.iam.gserviceaccount.com".into(), kind: None,
    }));
    gcs.expect_list_notifications().returning(move |_| Ok(ListNotificationsResponse {
        items: reads.lock().unwrap().clone(), ..Default::default()
    }));
    gcs.expect_insert_notification().times(1).returning(move |_, mut notification| {
        assert_eq!(notification.custom_attributes.len(), 1);
        assert!(notification.topic.as_deref().unwrap().starts_with("//pubsub.googleapis.com/projects/"));
        notification.id = Some("123".into()); writes.lock().unwrap().push(notification);
        Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted create, reply lost".into() }))
    });
    gcs.expect_delete_notification().times(1).returning(move |_, id| {
        assert_eq!(id, "123"); deletes.lock().unwrap().clear(); Ok(())
    });
    let mut pubsub = MockPubSubApi::new();
    pubsub.expect_get_topic_iam_policy().times(2).returning(|_| Ok(alien_gcp_clients::iam::IamPolicy {
        bindings: vec![alien_gcp_clients::iam::Binding {
            role: "roles/pubsub.publisher".into(), members: vec!["serviceAccount:storage@test.iam.gserviceaccount.com".into()], condition: None,
        }], ..Default::default()
    }));
    pubsub.expect_create_topic().never(); pubsub.expect_set_topic_iam_policy().never();
    let deleted_topic = topic.clone();
    pubsub.expect_delete_topic().times(1).returning(move |name| { assert_eq!(name, deleted_topic); Ok(()) });
    let gcs = Arc::new(gcs); let pubsub = Arc::new(pubsub);
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_gcs_client().returning(move |_| Ok(gcs.clone()));
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    let services = Arc::new(services);
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::CreatingPushSubscriptions;
    controller.storage_notification_topics = vec![topic];
    for step in 0..3 {
        if step == 2 { controller.state = GcpWorkerState::DeletingPushSubscriptions; }
        let mut executor = SingleControllerExecutor::builder()
            .resource(desired.clone()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
        let result = executor.step().await;
        if step == 1 { assert!(result.is_err()); } else { result.unwrap(); }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        assert!(controller.gcs_notification_ids.is_empty());
        match step {
            0 => { assert!(native.lock().unwrap().is_empty()); assert_eq!(controller.storage_notification_intents.len(), 1); }
            1 => { assert_eq!(native.lock().unwrap().len(), 1); assert_eq!(controller.storage_notification_intents.len(), 1); }
            _ => { assert!(native.lock().unwrap().is_empty()); assert!(controller.storage_notification_intents.is_empty()); assert!(controller.storage_notification_topics.is_empty()); }
        }
    }
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn removed_push_trigger_after_checkpoint_retains_intent_and_refuses_advancement(#[case] storage: bool) {
    let pubsub = Arc::new(MockPubSubApi::new());
    let gcs = Arc::new(MockGcsApi::new());
    let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    services.expect_get_gcp_gcs_client().returning(move |_| Ok(gcs.clone()));
    let services = Arc::new(services);
    let mut desired = function_public_ingress();
    desired.triggers = if storage {
        vec![alien_core::WorkerTrigger::Storage { storage: (&crate::core::controller_test::test_storage_1()).into(), events: vec!["created".into()] }]
    } else {
        vec![alien_core::WorkerTrigger::Queue { queue: (&crate::worker::fixtures::test_queue()).into() }]
    };
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = GcpWorkerState::CreatingPushSubscriptions;
    let queue = crate::queue::gcp::GcpQueueController {
        state: crate::queue::gcp::GcpQueueState::Ready,
        topic_name: Some("test-test-queue".into()),
        subscription_name: Some("test-test-queue-sub".into()),
        _internal_stay_count: None,
    };
    let mut executor = SingleControllerExecutor::builder()
        .resource(desired.clone()).controller(controller).platform(Platform::Gcp)
        .service_provider(services.clone()).with_test_dependencies()
        .with_dependency(crate::worker::fixtures::test_queue(), queue).build().await.unwrap();
    executor.step().await.unwrap();
    let controller: GcpWorkerController = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
    assert_eq!(controller.storage_topic_intents.len() + controller.push_subscription_intents.len(), 1);
    desired.triggers.clear();
    let mut executor = SingleControllerExecutor::builder()
        .resource(desired).controller(controller).platform(Platform::Gcp)
        .service_provider(services).with_test_dependencies().build().await.unwrap();
    let error = executor.step().await.expect_err("removed trigger must retain its unresolved ownership intent");
    assert!(format!("{error:?}").contains("RESOURCE_CONFIG_INVALID"));
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert_eq!(controller.storage_topic_intents.len() + controller.push_subscription_intents.len(), 1);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn denied_command_child_deletion_retains_receipt_for_retry(#[case] subscription: bool) {
    let calls = Arc::new(Mutex::new(0)); let writes = calls.clone();
    let remove = move |name: String| {
        let mut count = writes.lock().unwrap(); *count += 1;
        if *count == 1 {
            Err(AlienError::new(ClientError::RemoteAccessDenied { resource_type: "command child".into(), resource_name: name }))
        } else {
            Err(AlienError::new(ClientError::RemoteResourceNotFound { resource_type: "command child".into(), resource_name: name }))
        }
    };
    let mut pubsub = MockPubSubApi::new();
    if subscription { pubsub.expect_delete_subscription().times(2).returning(remove); }
    else { pubsub.expect_delete_topic().times(2).returning(remove); }
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone())); let services = Arc::new(services);
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    if subscription { controller.commands_subscription_name = Some("test-worker-rq-sub".into()); }
    else { controller.commands_topic_name = Some("test-worker-rq".into()); }
    for retry in [false, true] {
        controller.state = GcpWorkerState::DeletingCommandsInfrastructure;
        let mut executor = SingleControllerExecutor::builder()
            .resource(function_public_ingress()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
        let result = executor.step().await;
        if retry { result.unwrap(); } else {
            let error = result.expect_err("denied command cleanup must not advance or discard its receipt");
            assert!(format!("{error:?}").contains("RESOURCE_CLEANUP_INCOMPLETE"));
        }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        let retained = if subscription { controller.commands_subscription_name.is_some() } else { controller.commands_topic_name.is_some() };
        assert_eq!(retained, !retry);
    }
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn command_creation_checkpoints_and_cleans_uncertain_child_after_reload(#[case] subscription: bool) {
    let native = Arc::new(Mutex::new(None::<serde_json::Value>));
    let reads = native.clone(); let writes = native.clone(); let deletes = native.clone();
    let project = GcpClientConfig::mock().project_id;
    let mut pubsub = MockPubSubApi::new();
    if subscription {
        pubsub.expect_get_subscription().returning(move |name| {
            reads.lock().unwrap().clone().map(|value| serde_json::from_value(value).unwrap()).ok_or_else(|| AlienError::new(ClientError::RemoteResourceNotFound { resource_type: "subscription".into(), resource_name: name }))
        });
        pubsub.expect_create_subscription().times(1).returning(move |name, mut value| {
            assert!(value.push_config.as_ref().unwrap().oidc_token.is_none());
            value.name = Some(format!("projects/{project}/subscriptions/{name}"));
            *writes.lock().unwrap() = Some(serde_json::to_value(value).unwrap());
            Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted command subscription, lost reply".into() }))
        });
        pubsub.expect_delete_subscription().times(1).returning(move |name| {
            assert_eq!(name, "test-worker-rq-sub"); assert!(deletes.lock().unwrap().take().is_some()); Ok(())
        });
        pubsub.expect_delete_topic().times(1).returning(|name| { assert_eq!(name, "test-worker-rq"); Ok(()) });
    } else {
        pubsub.expect_get_topic().returning(move |name| {
            reads.lock().unwrap().clone().map(|value| serde_json::from_value(value).unwrap()).ok_or_else(|| AlienError::new(ClientError::RemoteResourceNotFound { resource_type: "topic".into(), resource_name: name }))
        });
        pubsub.expect_create_topic().times(1).returning(move |name, mut value| {
            value.name = Some(format!("projects/{project}/topics/{name}"));
            *writes.lock().unwrap() = Some(serde_json::to_value(value).unwrap());
            Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted command topic, lost reply".into() }))
        });
        pubsub.expect_delete_topic().times(1).returning(move |name| {
            assert_eq!(name, "test-worker-rq"); assert!(deletes.lock().unwrap().take().is_some()); Ok(())
        });
    }
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone())); let services = Arc::new(services);
    let mut desired = function_public_ingress(); desired.commands_enabled = true;
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::CreatingCommandsInfrastructure;
    if subscription { controller.commands_topic_name = Some("test-worker-rq".into()); }
    for step in 0..3 {
        if step == 2 { controller.state = GcpWorkerState::DeletingCommandsInfrastructure; }
        let mut executor = SingleControllerExecutor::builder().resource(desired.clone()).controller(controller).platform(Platform::Gcp)
            .service_provider(services.clone()).with_test_dependencies().build().await.unwrap();
        let result = executor.step().await;
        if step == 1 { assert!(result.is_err()); } else { result.unwrap(); }
        controller = serde_json::from_value(serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap()).unwrap();
        let intent = if subscription { controller.commands_subscription_intent.is_some() } else { controller.commands_topic_intent.is_some() };
        assert_eq!(intent, step != 2);
        assert_eq!(native.lock().unwrap().is_some(), step == 1);
        if step == 2 { assert!(controller.commands_topic_name.is_none()); assert!(controller.commands_subscription_name.is_none()); }
    }
}

#[tokio::test]
async fn command_cleanup_does_not_infer_ownership_from_service_name() {
    let mut pubsub = MockPubSubApi::new();
    pubsub.expect_delete_topic().never(); pubsub.expect_delete_subscription().never();
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    let mut desired = function_public_ingress(); desired.commands_enabled = true;
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::DeletingCommandsInfrastructure;
    let mut executor = SingleControllerExecutor::builder().resource(desired).controller(controller).platform(Platform::Gcp)
        .service_provider(Arc::new(services)).with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    assert_eq!(executor.internal_state::<GcpWorkerController>().unwrap().state, GcpWorkerState::DeletingService);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn command_enablement_update_reconciles_command_children(#[case] enabled: bool) {
    let mut previous = function_public_ingress(); previous.commands_enabled = !enabled;
    let mut desired = previous.clone(); desired.commands_enabled = enabled;
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::UpdatePushSubscriptions;
    let mut pubsub = MockPubSubApi::new();
    if !enabled {
        controller.commands_topic_name = Some("test-worker-rq".into());
        controller.commands_subscription_name = Some("test-worker-rq-sub".into());
        pubsub.expect_delete_subscription().times(1).returning(|name| { assert_eq!(name, "test-worker-rq-sub"); Ok(()) });
        pubsub.expect_delete_topic().times(1).returning(|name| { assert_eq!(name, "test-worker-rq"); Ok(()) });
    }
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    let mut executor = SingleControllerExecutor::builder().resource(desired).previous_resource(previous)
        .controller(controller).platform(Platform::Gcp).service_provider(Arc::new(services))
        .with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert!(controller.commands_topic_name.is_none()); assert!(controller.commands_subscription_name.is_none());
    assert_eq!(controller.state, GcpWorkerState::UpdateCreatingPushSubscriptions);
}

#[tokio::test]
async fn delivery_timeout_update_retires_old_subscription_before_recreation() {
    let previous = crate::worker::fixtures::function_with_queue_trigger();
    let mut desired = previous.clone(); desired.timeout_seconds = previous.timeout_seconds + 1;
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::UpdatePushSubscriptions;
    controller.push_subscriptions = vec!["recorded-subscription".into()];
    let mut pubsub = MockPubSubApi::new();
    pubsub.expect_delete_subscription().times(1).returning(|name| { assert_eq!(name, "recorded-subscription"); Ok(()) });
    let pubsub = Arc::new(pubsub); let mut services = MockPlatformServiceProvider::new();
    services.expect_get_gcp_pubsub_client().returning(move |_| Ok(pubsub.clone()));
    let mut executor = SingleControllerExecutor::builder().resource(desired).previous_resource(previous)
        .controller(controller).platform(Platform::Gcp).service_provider(Arc::new(services))
        .with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert!(controller.push_subscriptions.is_empty());
    assert_eq!(controller.state, GcpWorkerState::UpdateCreatingPushSubscriptions);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn unchanged_delivery_preserves_trigger_and_command_identities(#[case] capped_timeout: bool) {
    let mut previous = crate::worker::fixtures::function_with_queue_trigger();
    previous.commands_enabled = true;
    if capped_timeout { previous.timeout_seconds = 600; }
    let mut desired = previous.clone();
    if capped_timeout { desired.timeout_seconds = 3600; }
    else {
        desired.code = alien_core::WorkerCode::Image { image: "registry.example.test/worker:updated".into() };
        desired.environment.insert("RELEASE".into(), "updated".into());
    }
    let mut controller = GcpWorkerController::mock_ready("test-worker"); controller.state = GcpWorkerState::UpdatePushSubscriptions;
    controller.push_subscriptions = vec!["retained-queue-sub".into()];
    controller.commands_topic_name = Some("retained-command-topic".into());
    controller.commands_subscription_name = Some("retained-command-sub".into());
    let mut executor = SingleControllerExecutor::builder().resource(desired).previous_resource(previous)
        .controller(controller).platform(Platform::Gcp)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .with_test_dependencies().build().await.unwrap();
    executor.step().await.unwrap();
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    assert_eq!(controller.state, GcpWorkerState::UpdateSettingIamPolicy);
    assert_eq!(controller.push_subscriptions, ["retained-queue-sub"]);
    assert_eq!(controller.commands_topic_name.as_deref(), Some("retained-command-topic"));
    assert_eq!(controller.commands_subscription_name.as_deref(), Some("retained-command-sub"));
}

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
use alien_gcp_clients::gcs::MockGcsApi;
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
    for step in 0..5 {
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
        assert_eq!(restored[field], original[field]);
    }
}

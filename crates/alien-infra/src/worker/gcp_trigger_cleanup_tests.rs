use super::{GcpWorkerController, GcpWorkerState, GcsNotificationTracker};
use crate::core::MockPlatformServiceProvider;
use crate::core::controller_test::SingleControllerExecutor;
use crate::worker::fixtures::function_public_ingress;
use alien_client_core::ErrorData as ClientError;
use alien_core::Platform;
use alien_error::AlienError;
use alien_gcp_clients::cloudscheduler::MockCloudSchedulerApi;
use alien_gcp_clients::gcs::MockGcsApi;
use alien_gcp_clients::pubsub::MockPubSubApi;
use rstest::rstest;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug)]
enum Child {
    Notification,
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
) {
    let mut controller = GcpWorkerController::mock_ready("test-worker");
    controller.state = match child {
        Child::Scheduler => GcpWorkerState::DeletingSchedulerJobs,
        _ => GcpWorkerState::DeletingPushSubscriptions,
    };
    for name in ["first", "second"] {
        match child {
            Child::Notification => controller
                .gcs_notification_ids
                .push(GcsNotificationTracker {
                    bucket_name: "owned-bucket".into(),
                    notification_id: name.into(),
                }),
            Child::Topic => controller.storage_notification_topics.push(name.into()),
            Child::Scheduler => controller.scheduler_job_names.push(name.into()),
        }
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut executor = SingleControllerExecutor::builder()
        .resource(function_public_ingress())
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
        Child::Topic => controller.storage_notification_topics.clone(),
        Child::Scheduler => controller.scheduler_job_names.clone(),
    };
    assert_eq!(remaining, ["second"]);
    let retry_calls = Arc::new(Mutex::new(Vec::new()));
    let mut resumed = SingleControllerExecutor::builder()
        .resource(function_public_ingress())
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(provider(child, denied, true, retry_calls.clone()))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    resumed.step().await.unwrap();
    assert_eq!(*retry_calls.lock().unwrap(), ["second"]);
    let controller = resumed.internal_state::<GcpWorkerController>().unwrap();
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

//! Opt-in qualification with a generated restricted management identity.
//! Set STORAGE_NOTIFICATIONS_FIXTURE to a JSON fixture containing projectId,
//! projectNumber, region, prefix, serviceAccount, setupTokenFile, runtimeTokenFile,
//! and evidenceDir. Run export_permissions, install that plan on a fresh identity,
//! then run round_trip. The fixture owner must clean up all prefixed resources.

use crate::{
    controller_test::SingleControllerExecutor, queue::gcp::GcpQueueController,
    DefaultPlatformServiceProvider, GcpStorageController,
};
use alien_bindings::{providers::queue::GcpPubSubQueue, traits::Queue as QueueBinding};
use alien_core::{
    ClientConfig, GcpClientConfig, GcpCredentials, Platform, Queue, ResourceStatus, Storage,
    StorageNotification,
};
use alien_gcp_clients::{
    gcs::{GcsApi, GcsClient, GcsNotification, Object},
    iam::{Binding, IamPolicy},
    pubsub::{PubSubApi, PubSubClient, Topic},
};
use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::PathBuf, sync::Arc, time::Duration};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    project_id: String,
    project_number: String,
    region: String,
    prefix: String,
    service_account: String,
    setup_token_file: PathBuf,
    runtime_token_file: PathBuf,
    evidence_dir: PathBuf,
}

fn fixture() -> Fixture {
    let path =
        std::env::var("STORAGE_NOTIFICATIONS_FIXTURE").expect("explicit test fixture required");
    let fixture: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(!fixture.prefix.is_empty());
    assert!(fixture
        .service_account
        .ends_with(&format!("@{}.iam.gserviceaccount.com", fixture.project_id)));
    fixture
}

#[test]
#[ignore = "exports the exact permissions to install on an isolated test identity"]
fn export_permissions() {
    let f = fixture();
    let context = PermissionContext::new()
        .with_stack_prefix(&f.prefix)
        .with_stack_name("notifications")
        .with_project_name(&f.project_id)
        .with_project_number(&f.project_number)
        .with_region(&f.region)
        .with_service_account_name(f.service_account.split('@').next().unwrap());
    let mut plans = Vec::new();
    for name in [
        "storage/provision",
        "queue/provision",
        "queue/storage-notifications",
    ] {
        let plan = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(
                get_permission_set(name).unwrap(),
                BindingTarget::Stack,
                &context,
            )
            .unwrap();
        plans.push(serde_json::to_value(plan).unwrap());
    }
    fs::write(
        f.evidence_dir.join("grants.json"),
        serde_json::to_vec_pretty(&plans).unwrap(),
    )
    .unwrap();
}

fn config(f: &Fixture, token_file: &PathBuf) -> GcpClientConfig {
    GcpClientConfig {
        project_id: f.project_id.clone(),
        project_number: Some(f.project_number.clone()),
        region: f.region.clone(),
        credentials: GcpCredentials::AccessToken {
            token: fs::read_to_string(token_file).unwrap().trim().into(),
        },
        service_overrides: None,
    }
}

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

async fn cohort(
    gcs: &GcsClient,
    binding: &GcpPubSubQueue,
    bucket: &str,
    keys: &[&str],
    expected: &[&str],
) -> Vec<Value> {
    for key in keys {
        gcs.insert_object(
            bucket.into(),
            Object {
                name: Some((*key).into()),
                ..Default::default()
            },
            b"payload".to_vec(),
        )
        .await
        .unwrap();
    }
    let mut events = Vec::new();
    for _ in 0..24 {
        let messages = binding.receive("work", 10).await.unwrap();
        for message in messages {
            let value = serde_json::to_value(&message).unwrap();
            let payload = &value["payload"];
            assert_eq!(payload["eventType"], "created");
            assert_eq!(payload["bucketName"], bucket);
            assert_eq!(payload["size"], 7);
            events.push(payload.clone());
            binding.ack("work", &message.receipt_handle).await.unwrap();
        }
        if events.len() >= expected.len() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    let actual: BTreeSet<_> = events
        .iter()
        .map(|event| event["objectKey"].as_str().unwrap())
        .collect();
    assert_eq!(actual, expected.iter().copied().collect());
    assert_eq!(events.len(), expected.len());
    // Pub/Sub may return fewer than maxMessages, even with several uploads.
    // Worker batching is qualified separately by the deployed Worker harness.
    events
}

#[tokio::test]
#[ignore = "creates isolated bucket, topics and subscription with restricted controller credentials"]
async fn round_trip() {
    let f = fixture();
    let management = ClientConfig::Gcp(Box::new(config(&f, &f.runtime_token_file)));
    let setup_config = config(&f, &f.setup_token_file);
    let gcs = GcsClient::new(reqwest::Client::new(), setup_config.clone());
    let pubsub = PubSubClient::new(reqwest::Client::new(), setup_config.clone());
    let provider = Arc::new(DefaultPlatformServiceProvider::default());
    let queue = Queue::new("work".into()).build();
    let mut queue_run = SingleControllerExecutor::builder()
        .resource(queue.clone())
        .controller(GcpQueueController::default())
        .platform(Platform::Gcp)
        .resource_prefix(&f.prefix)
        .client_config(management.clone())
        .service_provider(provider.clone())
        .real_delays()
        .with_stack_resource(storage("in/"), alien_core::ResourceLifecycle::Live)
        .build()
        .await
        .unwrap();
    queue_run.run_until_terminal().await.unwrap();
    assert_eq!(queue_run.status(), ResourceStatus::Running);
    assert!(!queue_run.needs_update().unwrap());
    let mut run = SingleControllerExecutor::builder()
        .resource(storage("in/"))
        .controller(GcpStorageController::default())
        .platform(Platform::Gcp)
        .resource_prefix(&f.prefix)
        .client_config(management.clone())
        .service_provider(provider.clone())
        .real_delays()
        .with_dependency(
            queue.clone(),
            queue_run
                .internal_state::<GcpQueueController>()
                .unwrap()
                .clone(),
        )
        .build()
        .await
        .unwrap();
    run.run_until_terminal().await.unwrap();
    assert_eq!(run.status(), ResourceStatus::Running);
    let bucket = format!("{}-uploads", f.prefix);
    let topic = format!("{}-work", f.prefix);
    let legacy = format!("{}-legacy", f.prefix);
    pubsub
        .create_topic(legacy.clone(), Topic::default())
        .await
        .unwrap();
    let agent = gcs.get_project_service_account().await.unwrap();
    pubsub
        .set_topic_iam_policy(
            legacy.clone(),
            IamPolicy {
                bindings: vec![Binding {
                    role: "roles/pubsub.publisher".into(),
                    members: vec![format!("serviceAccount:{}", agent.email_address)],
                    condition: None,
                }],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let manual = gcs
        .insert_notification(
            bucket.clone(),
            GcsNotification {
                topic: Some(format!("projects/{}/topics/{legacy}", f.project_id)),
                event_types: vec!["OBJECT_FINALIZE".into()],
                payload_format: Some("JSON_API_V1".into()),
                object_name_prefix: Some("legacy/".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let binding = GcpPubSubQueue::new(topic.clone(), format!("{topic}-sub"), setup_config)
        .await
        .unwrap();
    // Provider documentation allows up to 30 seconds for new notification activation.
    tokio::time::sleep(Duration::from_secs(35)).await;
    let first = cohort(
        &gcs,
        &binding,
        &bucket,
        &[
            "in/a.json",
            "in/quote\"é*.json",
            "in/no.txt",
            "other/b.data",
            "outside/no.json",
        ],
        &["in/a.json", "in/quote\"é*.json", "other/b.data"],
    )
    .await;
    run.update(storage("changed/")).unwrap();
    run.run_until_terminal().await.unwrap();
    assert_eq!(run.status(), ResourceStatus::Running);
    let rules = gcs.list_notifications(bucket.clone()).await.unwrap().items;
    assert_eq!(rules.len(), 3);
    assert!(rules.iter().any(|rule| rule.id == manual.id));
    assert_eq!(
        rules
            .iter()
            .map(|rule| rule.object_name_prefix.as_deref().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["changed/", "other/", "legacy/"])
    );
    tokio::time::sleep(Duration::from_secs(35)).await;
    let second = cohort(
        &gcs,
        &binding,
        &bucket,
        &[
            "in/old.json",
            "changed/new.json",
            "changed/no.txt",
            "other/new.data",
        ],
        &["changed/new.json", "other/new.data"],
    )
    .await;
    crate::protect_retained_storage(
        run.stack_state(),
        &["uploads".to_string()],
        &management,
        provider.as_ref(),
        &alien_core::ExternalBindings::default(),
        &[alien_core::ResourceLifecycle::Live],
    )
    .await
    .unwrap();
    let rules = gcs.list_notifications(bucket.clone()).await.unwrap().items;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, manual.id);
    *queue_run.desired_resource_config_mut("uploads").unwrap() =
        alien_core::Resource::new(Storage::new("uploads".into()).build());
    assert!(queue_run.needs_update().unwrap());
    queue_run.update(queue).unwrap();
    queue_run.run_until_terminal().await.unwrap();
    assert_eq!(queue_run.status(), ResourceStatus::Running);
    assert!(!queue_run.needs_update().unwrap());
    assert!(pubsub
        .get_topic_iam_policy(topic.clone())
        .await
        .unwrap()
        .bindings
        .is_empty());
    queue_run.delete().unwrap();
    queue_run.run_until_terminal().await.unwrap();
    assert_eq!(queue_run.status(), ResourceStatus::Deleted);
    fs::write(
        f.evidence_dir.join("messages.json"),
        serde_json::to_vec_pretty(
            &json!({"first": first, "second": second, "manualNotification": manual.id}),
        )
        .unwrap(),
    )
    .unwrap();
    // The fixture owner retains evidence and independently lists then deletes the
    // bucket, objects and manual topic after verifying the preserved notification.
}

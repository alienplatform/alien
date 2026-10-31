//! A compiled SDK application must consume batches and retain its cursor across a cold restart.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
    time::Duration,
};

use alien_core::{
    bindings::serialize_binding_as_env_var, Queue, QueueBatchConfig, ResourceRef, Storage,
    StorageNotification, WorkerTrigger,
};
use alien_local::LocalBindingsProvider;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

fn sources() -> Vec<Storage> {
    [
        (
            "objects",
            vec![("in/", ".json"), ("other/", ".data"), ("other/", ".csv")],
        ),
        ("second", vec![("shared/", ".json")]),
    ]
    .into_iter()
    .map(|(id, rules)| {
        Storage::new(id.into())
            .notifications(
                rules
                    .into_iter()
                    .map(|(prefix, suffix)| {
                        StorageNotification::builder()
                            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
                            .prefix(prefix.into())
                            .suffix(suffix.into())
                            .build()
                    })
                    .collect(),
            )
            .build()
    })
    .collect()
}

async fn start(provider: &Arc<LocalBindingsProvider>) -> String {
    let mut env = HashMap::from([
        ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
        ("ALIEN_WORKER_TIMEOUT_SECONDS".into(), "120".into()),
        (
            "ALIEN_QUEUE_BATCH_CONFIG".into(),
            json!({"jobs": {"maxMessages": 10, "maxWindowSeconds": 5}}).to_string(),
        ),
    ]);
    for id in ["objects", "second", "results"] {
        env.extend(
            serialize_binding_as_env_var(id, &provider.storage_manager().get_binding(id).unwrap())
                .unwrap(),
        );
    }
    let url = provider
        .worker_manager()
        .start_worker("consumer", env, vec![], vec![])
        .await
        .unwrap();
    provider
        .worker_manager()
        .ensure_triggers(
            "consumer",
            vec![WorkerTrigger::Queue {
                queue: ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"),
                batch: Some(QueueBatchConfig {
                    max_messages: 10,
                    max_window_seconds: 5,
                }),
            }],
            Arc::clone(provider),
        )
        .await
        .unwrap();
    for source in sources() {
        provider
            .storage_manager()
            .ensure_notifications(&source, provider.queue_manager())
            .await
            .unwrap();
    }
    url
}

fn batches(root: &Path) -> Vec<Value> {
    let directory = root.join("storage/results/batches");
    if !directory.exists() {
        return vec![];
    }
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn assert_records(root: &Path, expected: &BTreeMap<(String, String, u64), usize>) -> usize {
    let records = batches(root);
    let mut actual = BTreeMap::new();
    for batch in &records {
        assert!(!batch["source"].as_str().unwrap().is_empty());
        let events = batch["events"].as_array().unwrap();
        assert!(!events.is_empty() && events.len() <= 10);
        for delivery in events {
            for field in ["messageId", "deliveryId"] {
                assert!(!delivery[field].as_str().unwrap().is_empty());
            }
            let event = &delivery["event"];
            assert_eq!(event["eventType"], "created");
            chrono::DateTime::parse_from_rfc3339(event["timestamp"].as_str().unwrap()).unwrap();
            let key = (
                event["bucketName"].as_str().unwrap().to_string(),
                event["objectKey"].as_str().unwrap().to_string(),
                event["size"].as_u64().unwrap(),
            );
            let count = actual.entry(key.clone()).or_insert(0);
            *count += 1;
            assert!(
                *count <= *expected.get(&key).unwrap_or(&0),
                "unexpected or duplicate event {key:?}: {records:?}"
            );
        }
    }
    actual.values().sum()
}

async fn delivered(root: &Path, expected: &BTreeMap<(String, String, u64), usize>) {
    timeout(Duration::from_secs(45), async {
        while assert_records(root, expected) != expected.values().sum::<usize>() {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("compiled Worker must record every matching event");
    assert!(
        batches(root)
            .iter()
            .any(|batch| batch["events"].as_array().unwrap().len() > 1),
        "the Worker must receive a real multi-message batch"
    );
}

fn put_offline(root: &Path, key: &str, value: &str) {
    let path = root.join("storage/objects").join(key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let staged = path.with_extension("staged#1");
    std::fs::write(&staged, value).unwrap();
    std::fs::rename(staged, path).unwrap();
}

#[tokio::test]
async fn compiled_worker_consumes_notifications_after_cold_restart() {
    tracing_subscriber::fmt::try_init().expect("initialize test tracing");
    super::require_bun();
    let directory = TempDir::new().unwrap();
    let image = super::build_app(
        directory.path(),
        "tests/e2e/test-apps/storage-notifications",
    )
    .await;
    let root = directory.path().join("state");
    let provider = LocalBindingsProvider::new(&root).unwrap();
    provider.queue_manager().create_queue("jobs").await.unwrap();
    for id in ["objects", "second", "results"] {
        provider.storage_manager().create_storage(id).await.unwrap();
    }
    provider
        .worker_manager()
        .extract_image("consumer", image.to_str().unwrap(), None)
        .await
        .unwrap();
    let url = start(&provider).await;
    let mut writes = vec![];
    let mut expected = BTreeMap::new();
    for (source, key) in [
        ("objects", "in/quote\"é*%.json"),
        ("objects", "other/b.data"),
        ("objects", "other/d.csv"),
        ("second", "shared/c.json"),
    ] {
        expected.insert((source.into(), key.into(), 7), 1);
        writes.push(json!({"storage":source,"key":key,"content":"payload"}));
    }
    for index in 0..12 {
        let key = format!("in/bulk-{index}.json");
        expected.insert(("objects".into(), key.clone(), 7), 1);
        writes.push(json!({"storage":"objects","key":key,"content":"payload"}));
    }
    for key in ["in/excluded.txt", "other/excluded.json", "IN/excluded.json"] {
        writes.push(json!({"storage":"objects","key":key,"content":"excluded"}));
    }
    let response = reqwest::Client::new()
        .post(format!("{url}/objects"))
        .json(&writes)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    delivered(&root, &expected).await;
    // A completed result write precedes settlement. Observe the real queue owner
    // before shutdown, so successful messages cannot remain hidden in flight.
    timeout(Duration::from_secs(10), async {
        while provider
            .queue_manager()
            .load_queue("jobs")
            .await
            .unwrap()
            .stats()
            .await
            .unwrap()
            .total
            > 0
        {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    provider.shutdown().await;

    put_offline(&root, "in/offline.json", "offline");
    put_offline(&root, "other/offline.data", "offline");
    put_offline(&root, "in/bulk-0.json", "overwritten");
    put_offline(&root, "in/offline.txt", "excluded");
    for (key, size) in [
        ("in/offline.json", 7),
        ("other/offline.data", 7),
        ("in/bulk-0.json", 11),
    ] {
        expected.insert(("objects".into(), key.into(), size), 1);
    }
    let provider = LocalBindingsProvider::new(&root).unwrap();
    start(&provider).await;
    delivered(&root, &expected).await;
    // Cover a complete 30-second safety scan as well as filesystem hints.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(32);
    while tokio::time::Instant::now() < deadline {
        assert_eq!(
            assert_records(&root, &expected),
            expected.values().sum::<usize>()
        );
        sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        provider
            .queue_manager()
            .load_queue("jobs")
            .await
            .unwrap()
            .stats()
            .await
            .unwrap()
            .total,
        0
    );
    provider
        .worker_manager()
        .delete_worker("consumer")
        .await
        .unwrap();
    for id in ["objects", "second", "results"] {
        provider.storage_manager().delete_storage(id).await.unwrap();
        assert!(!root.join("storage").join(id).exists());
        assert!(!root.join("storage-notifications").join(id).exists());
    }
    provider.queue_manager().delete_queue("jobs").await.unwrap();
    assert!(!provider.queue_manager().queue_exists("jobs"));
    provider.shutdown().await;
}

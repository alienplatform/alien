use alien_bindings::{
    providers::queue::local::LocalQueue,
    traits::{MessagePayload, Queue},
};
use alien_local::LocalQueueManager;

/// A manager upgrade must not adopt a file still used by an older SDK.
#[tokio::test]
async fn legacy_queue_cannot_be_adopted_while_client_is_alive() {
    let directory = tempfile::tempdir().unwrap();
    let old = LocalQueue::new(directory.path().join("queue/jobs"))
        .await
        .unwrap();
    old.send("jobs", MessagePayload::Text("retained".into()))
        .await
        .unwrap();
    let manager = LocalQueueManager::new(directory.path().to_owned());
    let error = manager
        .create_queue("jobs")
        .await
        .expect_err("legacy ownership must require an offline handoff");
    assert!(error.to_string().contains("offline migration"));
    let messages = old.receive("jobs", 1).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert!(matches!(&messages[0].payload, MessagePayload::Text(text) if text == "retained"));
    old.ack("jobs", &messages[0].receipt_handle).await.unwrap();
}

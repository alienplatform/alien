//! Real process boundary: a queue owner, an SDK sender and independent HTTP observers.
use alien_bindings::{
    providers::queue::{local::LocalQueue, local_http::LocalHttpQueue},
    traits::{MessagePayload, Queue, QueueAckResult, QueueSendResult},
    Bindings,
};
use alien_core::bindings::QueueBinding;
use alien_local::{LocalQueueManager, LocalQueueService};
use std::{
    collections::HashSet,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if self.0.try_wait().expect("child status").is_none() {
            self.0.kill().expect("kill child");
        }
        self.0.wait().expect("reap child");
    }
}
fn child(mode: &str, dir: &Path) -> Process {
    Process(
        Command::new(std::env::current_exe().expect("test binary"))
            .args(["--ignored", "--exact", mode, "--nocapture"])
            .env("ALIEN_QUEUE_PROCESS_TEST_DIR", dir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn process"),
    )
}
async fn wait_file(path: &Path, process: &mut Process) {
    let until = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            process.0.try_wait().expect("child status").is_none(),
            "child exited before ready"
        );
        assert!(Instant::now() < until, "child readiness timeout");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn binding(dir: &Path) -> QueueBinding {
    serde_json::from_slice(&std::fs::read(dir.join("binding.json")).expect("binding file"))
        .expect("binding JSON")
}
fn client(dir: &Path) -> LocalHttpQueue {
    let QueueBinding::LocalHttp(binding) = binding(dir) else {
        panic!("HTTP binding")
    };
    LocalHttpQueue::new(binding).expect("HTTP client")
}

#[tokio::test]
#[ignore = "spawned by owner_http_survives_process_boundaries_and_crash"]
async fn owner_process() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("ALIEN_QUEUE_PROCESS_TEST_DIR").expect("fixture dir"),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let manager = Arc::new(LocalQueueManager::new(dir.clone()));
    let service = Arc::new(LocalQueueService::default());
    service.register(
        "dep",
        manager.clone(),
        &format!("http://{}", listener.local_addr().expect("address")),
    );
    manager.create_queue("jobs").await.expect("create queue");
    let bytes =
        serde_json::to_vec(&manager.runtime_binding("jobs").expect("binding")).expect("encode");
    std::fs::write(dir.join("binding.tmp"), bytes).expect("write binding");
    std::fs::rename(dir.join("binding.tmp"), dir.join("binding.json")).expect("publish binding");
    axum::serve(listener, service.router())
        .await
        .expect("serve");
}

#[tokio::test]
#[ignore = "spawned by owner_http_survives_process_boundaries_and_crash"]
async fn sender_process() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("ALIEN_QUEUE_PROCESS_TEST_DIR").expect("fixture dir"),
    );
    let mut env = std::collections::HashMap::from([(
        "ALIEN_DEPLOYMENT_TYPE".to_string(),
        "local".to_string(),
    )]);
    env.insert(
        "ALIEN_JOBS_BINDING".into(),
        serde_json::to_string(&binding(&dir)).unwrap(),
    );
    let bindings = Bindings::from_env_map(env).expect("SDK configuration");
    let queue = bindings.queue("jobs").await.expect("SDK queue");
    for i in 0..100 {
        queue
            .send(MessagePayload::Text(format!("message-{i}")))
            .await
            .expect("durable send");
    }
}

#[tokio::test]
async fn owner_http_survives_process_boundaries_and_crash() {
    let dir = TempDir::new().expect("fixture");
    let mut owner = child("owner_process", dir.path());
    wait_file(&dir.path().join("binding.json"), &mut owner).await;
    let queue = client(dir.path());
    let mut competitor = child("competing_owner_process", dir.path());
    assert!(competitor.0.wait().expect("competing owner exit").success());
    let mut sender = child("sender_process", dir.path());
    let mut received = HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while received.len() < 100 {
        if let Some(status) = sender.0.try_wait().expect("sender status") {
            assert!(status.success(), "sender failed");
        }
        assert!(
            Instant::now() < deadline,
            "all acknowledged sends must arrive"
        );
        let observer = client(dir.path());
        let (batch, stats) = tokio::join!(queue.receive("jobs", 10), observer.stats());
        let stats = stats.expect("concurrent observer");
        assert_eq!(stats.total, stats.visible + stats.in_flight + stats.delayed);
        let batch = batch.expect("receive");
        let mut receipts = Vec::new();
        for message in batch {
            let MessagePayload::Text(text) = message.payload else {
                panic!("text preserved")
            };
            assert!(
                received.insert(text),
                "duplicate delivery before lease expiry"
            );
            assert_eq!(message.attempt, 1);
            receipts.push(message.receipt_handle);
        }
        assert!(queue
            .ack_batch("jobs", &receipts)
            .await
            .unwrap()
            .iter()
            .all(|r| *r == QueueAckResult::Acknowledged));
        tokio::task::yield_now().await;
    }
    assert!(sender.0.wait().expect("sender exit").success());
    assert_eq!(queue.stats().await.unwrap().total, 0);

    let outcomes = queue
        .send_batch(
            "jobs",
            vec![
                MessagePayload::Json(serde_json::json!([1, true, null])),
                MessagePayload::Text(String::new()),
                MessagePayload::Text("receipt".into()),
            ],
        )
        .await
        .unwrap();
    assert!(matches!(
        outcomes.as_slice(),
        [
            QueueSendResult::Sent,
            QueueSendResult::Rejected { .. },
            QueueSendResult::Sent
        ]
    ));
    let first = queue.receive("jobs", 2).await.unwrap();
    assert_eq!(first.len(), 2);
    assert!(
        matches!(&first[0].payload, MessagePayload::Json(v) if v == &serde_json::json!([1, true, null]))
    );
    queue.nack("jobs", &first[1].receipt_handle).await.unwrap();
    let next = queue.receive("jobs", 1).await.unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].attempt, 2);
    assert!(queue.ack("jobs", &first[1].receipt_handle).await.is_err());
    assert!(queue
        .renew_lease("jobs", &first[1].receipt_handle, 30)
        .await
        .is_err());
    queue
        .renew_lease("jobs", &next[0].receipt_handle, 1)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(queue
        .renew_lease("jobs", &next[0].receipt_handle, 30)
        .await
        .is_err());
    queue.purge("jobs").await.unwrap();
    assert_eq!(queue.stats().await.unwrap().total, 0);

    // A confirmed send must survive SIGKILL, without a graceful database close.
    queue
        .send("jobs", MessagePayload::Text("survives-crash".into()))
        .await
        .unwrap();
    drop(owner);
    assert!(queue
        .send("jobs", MessagePayload::Text("offline".into()))
        .await
        .is_err());
    let unknown = queue
        .send_batch("jobs", vec![MessagePayload::Text("uncertain".into())])
        .await
        .unwrap();
    assert!(matches!(
        unknown.as_slice(),
        [QueueSendResult::Unknown { .. }]
    ));
    std::fs::remove_file(dir.path().join("binding.json")).unwrap();
    let mut restarted = child("owner_process", dir.path());
    wait_file(&dir.path().join("binding.json"), &mut restarted).await;
    let queue = client(dir.path());
    let recovered = queue.receive("jobs", 10).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(matches!(&recovered[0].payload, MessagePayload::Text(v) if v == "survives-crash"));
    queue
        .ack("jobs", &recovered[0].receipt_handle)
        .await
        .unwrap();
    assert_eq!(queue.stats().await.unwrap().total, 0);
}

#[tokio::test]
async fn legacy_store_is_refused_and_deleted_handles_are_revoked() {
    let dir = TempDir::new().unwrap();
    let legacy = dir.path().join("queue/jobs");
    let old = LocalQueue::new(legacy.clone()).await.unwrap();
    old.send("jobs", MessagePayload::Text("legacy".into()))
        .await
        .unwrap();
    let manager = LocalQueueManager::new(dir.path().to_owned());
    assert!(manager
        .create_queue("jobs")
        .await
        .unwrap_err()
        .to_string()
        .contains("offline migration"));
    assert_eq!(old.receive("jobs", 1).await.unwrap().len(), 1);
    drop(old);
    // Explicit offline handoff preserves the entire store and WAL together.
    std::fs::create_dir(dir.path().join("queue-owned")).unwrap();
    std::fs::rename(legacy, dir.path().join("queue-owned/jobs")).unwrap();
    let owned = manager.load_queue("jobs").await.unwrap();
    assert_eq!(owned.stats().await.unwrap().total, 1);
    manager.delete_queue("jobs").await.unwrap();
    assert!(owned
        .send("jobs", MessagePayload::Text("late".into()))
        .await
        .is_err());
    assert!(!dir.path().join("queue-owned/jobs").exists());
}

#[tokio::test]
async fn delayed_rows_survive_restart_and_observers_use_http() {
    let dir = TempDir::new().unwrap();
    let store_dir = dir.path().join("queue-owned/jobs");
    {
        let store = LocalQueue::single_process(store_dir.clone()).await.unwrap();
        store
            .send("jobs", MessagePayload::Text("delayed".into()))
            .await
            .unwrap();
        let db = turso::Builder::new_local(&store_dir.join("localqueue.sqlite").to_string_lossy())
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute(
            "UPDATE messages SET visible_at = ?1",
            [chrono::Utc::now().timestamp_millis() + 10_000],
        )
        .await
        .unwrap();
    }
    let mut owner = child("owner_process", dir.path());
    wait_file(&dir.path().join("binding.json"), &mut owner).await;
    let queue = client(dir.path());
    let stats = queue.stats().await.unwrap();
    assert_eq!(
        (stats.total, stats.visible, stats.delayed, stats.in_flight),
        (1, 0, 1, 0)
    );
    assert!(queue.receive("jobs", 1).await.unwrap().is_empty());
    let QueueBinding::LocalHttp(mut invalid) = binding(dir.path()) else {
        panic!("HTTP binding")
    };
    invalid.token = Some("invalid".into());
    let error = LocalHttpQueue::new(invalid)
        .unwrap()
        .purge("jobs")
        .await
        .unwrap_err();
    assert_eq!(error.code, "LOCAL_QUEUE_UNAUTHORIZED");
    assert_eq!(error.http_status_code, Some(401));
    assert!(!error.retryable);
    assert_eq!(
        queue.stats().await.unwrap().total,
        1,
        "unauthorized purge cannot mutate queue"
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let batch = queue.receive("jobs", 1).await.unwrap();
        if let Some(message) = batch.first() {
            assert_eq!(message.attempt, 1);
            assert!(matches!(&message.payload, MessagePayload::Text(v) if v == "delayed"));
            queue.ack("jobs", &message.receipt_handle).await.unwrap();
            break;
        }
        assert!(
            Instant::now() < deadline,
            "delayed message must become visible"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(queue.stats().await.unwrap().total, 0);
}

#[tokio::test]
#[ignore = "spawned by owner_http_survives_process_boundaries_and_crash"]
async fn competing_owner_process() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("ALIEN_QUEUE_PROCESS_TEST_DIR").expect("fixture dir"),
    );
    let manager = LocalQueueManager::new(dir);
    assert!(
        manager.load_queue("jobs").await.is_err(),
        "a second process must not open an owned queue"
    );
}

#[tokio::test]
async fn large_batches_preserve_order_and_rejections_across_http_chunks() {
    let dir = TempDir::new().unwrap();
    let mut owner = child("owner_process", dir.path());
    wait_file(&dir.path().join("binding.json"), &mut owner).await;
    let queue = client(dir.path());
    // Exceeds 8 MiB even before escaping; control characters expand sixfold on the wire.
    let payload = "\0".repeat(65_536);
    let mut messages = vec![MessagePayload::Text(payload.clone()); 129];
    messages.insert(12, MessagePayload::Text("x".repeat(65_537)));
    let outcomes = queue.send_batch("jobs", messages).await.unwrap();
    assert_eq!(outcomes.len(), 130);
    for (index, outcome) in outcomes.iter().enumerate() {
        if index == 12 {
            assert!(matches!(outcome, QueueSendResult::Rejected { .. }));
        } else {
            assert!(matches!(outcome, QueueSendResult::Sent));
        }
    }
    assert_eq!(queue.stats().await.unwrap().total, 129);
    let mut count = 0;
    loop {
        let messages = queue.receive("jobs", 10).await.unwrap();
        if messages.is_empty() {
            break;
        }
        count += messages.len();
        for message in messages {
            assert!(matches!(&message.payload, MessagePayload::Text(v) if v == &payload));
            queue.ack("jobs", &message.receipt_handle).await.unwrap();
        }
    }
    assert_eq!(count, 129);
    assert_eq!(queue.stats().await.unwrap().total, 0);
}

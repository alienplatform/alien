//! CloudRun transport integration tests
//!
//! Tests CloudRun event handling:
//! - HTTP request forwarding
//! - GCS CloudEvents → StorageEvent via gRPC → KV storage
//! - Pub/Sub CloudEvents → QueueMessage via gRPC → KV storage
//! - Cloud Scheduler → CronEvent via gRPC → KV storage
//! - Commands via Pub/Sub CloudEvents → command dispatch via gRPC

use alien_core::bindings;
use alien_worker_protocol::{run_grpc_server, ControlGrpcServer, WaitUntilGrpcServer};
use alien_worker_runtime::{run, RuntimeConfig, RuntimeDependencies, TransportType};
use anyhow::Context;
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use port_check::free_local_port;
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    sync::{Arc, Once},
    time::Duration,
};
use tempfile::TempDir;
use test_context::{test_context, AsyncTestContext};
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::{debug, info, instrument};
use uuid::Uuid;

mod test_utils;

static TRACING_INIT: Once = Once::new();

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    "info,alien_worker_runtime=debug,alien_test_server=debug".into()
                }),
            )
            .with_test_writer()
            .try_init()
            .ok();
    });
}

/// Handle for managing a running alien-worker-runtime instance
struct RuntimeHandle {
    task: JoinHandle<alien_worker_runtime::Result<()>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl RuntimeHandle {
    fn new(
        task: JoinHandle<alien_worker_runtime::Result<()>>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        Self { task, shutdown_tx }
    }

    /// Signal shutdown
    fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Holds gRPC server resources for tests
struct GrpcTestResources {
    #[allow(dead_code)]
    server_task:
        JoinHandle<Result<(), alien_error::AlienError<alien_worker_protocol::error::ErrorData>>>,
    grpc_address: String,
    #[allow(dead_code)]
    wait_until_server: Arc<WaitUntilGrpcServer>,
    #[allow(dead_code)]
    control_server: Arc<ControlGrpcServer>,
    _temp_dir: TempDir,
}

impl std::fmt::Debug for GrpcTestResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcTestResources")
            .field("grpc_address", &self.grpc_address)
            .finish()
    }
}

async fn setup_grpc_server() -> anyhow::Result<GrpcTestResources> {
    // The worker-protocol gRPC server (Control + WaitUntil) needs no bindings
    // provider — bindings are resolved in-process by the child app. This temp dir
    // is just kept alive on the returned handle.
    let temp_data_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let port = free_local_port().context("Failed to find free port for gRPC server")?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let server_addr_str = addr.to_string();
    let grpc_address = server_addr_str.clone();

    info!(grpc_address = %grpc_address, "Starting gRPC server");

    let grpc_handles = run_grpc_server(&server_addr_str)
        .await
        .context("Failed to start gRPC server")?;

    // Wait for server to be ready
    grpc_handles
        .readiness_receiver
        .await
        .map_err(|_| anyhow::anyhow!("gRPC readiness channel closed"))?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(GrpcTestResources {
        server_task: grpc_handles.server_task,
        grpc_address,
        wait_until_server: grpc_handles.wait_until_server,
        control_server: grpc_handles.control_server,
        _temp_dir: temp_data_dir,
    })
}

/// Test context for CloudRun transport integration tests
struct CloudRunTestContext {
    runtime_handle: Option<RuntimeHandle>,
    transport_port: u16,
    grpc_resources: Option<GrpcTestResources>,
    // Backing dir for the child app's in-process `test-kv` binding; kept alive
    // for the lifetime of the test so the event round-trip can read it back.
    _app_data_dir: TempDir,
}

impl std::fmt::Debug for CloudRunTestContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudRunTestContext")
            .field("transport_port", &self.transport_port)
            .finish()
    }
}

impl AsyncTestContext for CloudRunTestContext {
    async fn setup() -> Self {
        init_tracing();

        let grpc_resources = setup_grpc_server()
            .await
            .expect("Failed to setup gRPC server");

        let test_app_path = test_utils::get_test_app_path().expect("Failed to get test app path");
        info!(?test_app_path, "Using alien-test-app binary");

        let (application_port, transport_port) = test_utils::distinct_free_local_ports()
            .expect("Failed to find distinct ports for app and CloudRun transport");
        info!(%application_port, %transport_port, "Resolved application and CloudRun transport ports");

        // Build RuntimeConfig directly (no CLI parsing needed)
        let mut env_vars = HashMap::new();
        env_vars.insert(
            "ALIEN_SKIP_WAIT_UNTIL_EXTENSION".to_string(),
            "1".to_string(),
        );
        // Bindings are now resolved in-process by the direct provider, so the child
        // app runs under the `local` platform (no cloud feature required to build the
        // provider). The CloudRun transport under test is selected explicitly via
        // RuntimeConfig::transport, independent of this deployment type. The runtime
        // injects ALIEN_WORKER_GRPC_ADDRESS for the child from RuntimeConfig's
        // worker_grpc_address; its presence is what selects the Worker protocol (Control
        // + wait_until) gRPC channel that this test's server provides.
        env_vars.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());
        env_vars.insert("PORT".to_string(), application_port.to_string());

        // The event handlers in alien-test-app persist events into the `test-kv`
        // binding and the read-back endpoints load it. With bindings resolved
        // in-process (never through the Worker protocol), give the child app its own local test-kv
        // binding so the storage/queue event round-trips still work end-to-end.
        let app_data_dir =
            tempfile::tempdir().expect("Failed to create app data dir for test-kv binding");
        let kv_binding =
            bindings::KvBinding::local(app_data_dir.path().to_str().unwrap().to_string());
        env_vars.insert(
            bindings::binding_env_var_name("test-kv"),
            serde_json::to_string(&kv_binding).expect("Failed to serialize test-kv binding"),
        );

        env_vars.insert(
            "RUST_LOG".to_string(),
            env::var("RUST_LOG").unwrap_or_else(|_| {
                "info,alien_worker_runtime=debug,alien_test_server=debug".to_string()
            }),
        );

        let config = RuntimeConfig::builder()
            .transport(TransportType::CloudRun)
            .transport_port(transport_port)
            .command(vec![test_app_path.to_str().unwrap().to_string()])
            .worker_grpc_address(grpc_resources.grpc_address.clone())
            .env_vars(env_vars)
            .build();

        info!("Starting alien-worker-runtime programmatically with CloudRun transport...");

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Clone handles for the runtime
        let wait_until_server = grpc_resources.wait_until_server.clone();
        let control_server = grpc_resources.control_server.clone();

        // Start alien-worker-runtime in a background task with external gRPC handles
        let runtime_task = tokio::spawn(async move {
            run(
                config,
                shutdown_rx,
                RuntimeDependencies::ExternalWorkerProtocol {
                    wait_until_server,
                    control_server,
                },
            )
            .await
        });

        let runtime_handle = RuntimeHandle::new(runtime_task, shutdown_tx);

        // Wait for CloudRun transport readiness
        info!("Waiting for CloudRun transport to become ready...");

        let ready_result = tokio::time::timeout(Duration::from_secs(60), async {
            let client = reqwest::Client::new();
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                // Try a simple HTTP request to check if the transport is ready
                if let Ok(_resp) = client
                    .get(&format!("http://127.0.0.1:{}/health", transport_port))
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await
                {
                    // Any response means the transport is listening
                    break;
                }
                debug!("CloudRun transport not ready yet...");
            }
        })
        .await;

        ready_result.expect("CloudRun transport failed to become ready after 60s");
        info!("CloudRun transport is ready");

        Self {
            runtime_handle: Some(runtime_handle),
            transport_port,
            grpc_resources: Some(grpc_resources),
            _app_data_dir: app_data_dir,
        }
    }

    async fn teardown(mut self) {
        info!("Tearing down CloudRun test context");

        // Take ownership of resources to drop them in order
        if let Some(runtime_handle) = self.runtime_handle.take() {
            // Signal shutdown
            runtime_handle.shutdown();

            // Wait for runtime task to complete gracefully (with timeout)
            // This gives the runtime time to kill the child process properly
            match tokio::time::timeout(Duration::from_secs(5), runtime_handle.task).await {
                Ok(Ok(_)) => {
                    info!("Runtime task completed gracefully");
                }
                Ok(Err(e)) => {
                    panic!("Runtime task completed with error: {:?}", e);
                }
                Err(_) => {
                    panic!("Runtime task did not complete within 5 seconds - child process may be orphaned!");
                }
            }
        }

        if let Some(grpc_resources) = self.grpc_resources.take() {
            // Abort gRPC server task
            grpc_resources.server_task.abort();
            drop(grpc_resources);
        }

        // Wait a moment for everything to clean up
        tokio::time::sleep(Duration::from_millis(200)).await;

        info!("CloudRun test context teardown complete");
    }
}

async fn check_event_stored(
    transport_port: u16,
    event_type: &str,
    event_key: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let client = reqwest::Client::new();
    let url = format!(
        "http://127.0.0.1:{}/events/{}/{}",
        transport_port, event_type, event_key
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .context("Failed to check event storage")?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    anyhow::ensure!(
        response.status().is_success(),
        "Event read-back endpoint returned {}",
        response.status()
    );

    // The test app returns the stored record directly (no `{found, event}` wrapper).
    let body: serde_json::Value = response.json().await?;
    Ok(Some(body))
}

// --- Test Cases ---

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_http_request(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing HTTP request forwarding...");
    let test_id = Uuid::new_v4().to_string();

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/inspect", ctx.transport_port);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(json!({ "test_id": test_id }).to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send HTTP request")?;

    assert!(
        response.status().is_success(),
        "HTTP request should succeed"
    );

    let res_body: serde_json::Value = response.json().await?;
    assert_eq!(res_body["success"], true);
    assert_eq!(res_body["requestBody"]["test_id"], test_id);

    info!("HTTP request forwarding PASSED");
    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_gcs_storage_event(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing GCS storage CloudEvent...");
    let test_key = format!("test/data-{}.zip", Uuid::new_v4());
    let event_time = Utc::now();

    // Build GCS CloudEvent in binary format
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let event_data = json!({
        "bucket": "test-alien-bucket",
        "name": test_key,
        "size": "1024",
        "contentType": "application/zip",
        "etag": "test-etag",
        "storageClass": "STANDARD"
    });

    let response = client
        .post(&url)
        .header("ce-id", Uuid::new_v4().to_string())
        .header("ce-type", "google.cloud.storage.object.v1.finalized")
        .header(
            "ce-source",
            "//storage.googleapis.com/projects/_/buckets/test-alien-bucket",
        )
        .header("ce-specversion", "1.0")
        .header("ce-time", event_time.to_rfc3339())
        .header("Content-Type", "application/json")
        .body(event_data.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send GCS CloudEvent")?;

    assert!(
        response.status().is_success(),
        "GCS CloudEvent request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify event was delivered to the handler and stored in KV.
    let stored_event = check_event_stored(ctx.transport_port, "storage", &test_key)
        .await?
        .context("Storage event should have been delivered and stored in KV")?;
    assert_eq!(stored_event["bucket"], "test-alien-bucket");
    assert_eq!(stored_event["key"], test_key);
    info!("GCS storage CloudEvent PASSED");

    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_gcs_pubsub_notification(
    ctx: &mut CloudRunTestContext,
) -> anyhow::Result<()> {
    info!("Testing wrapped GCS Pub/Sub notification...");
    let test_key = format!("test/pubsub-{}.txt", Uuid::new_v4());
    let event_time = Utc::now();
    let object_data = json!({
        "bucket": "test-alien-bucket",
        "name": test_key,
        "size": "42",
        "contentType": "text/plain",
        "etag": "test-pubsub-etag",
        "generation": "1752619211123000",
        "storageClass": "STANDARD"
    });
    let push_body = json!({
        "message": {
            "data": general_purpose::STANDARD.encode(object_data.to_string()),
            "messageId": Uuid::new_v4().to_string(),
            "publishTime": event_time.to_rfc3339(),
            "attributes": {
                "notificationConfig": "projects/_/buckets/test-alien-bucket/notificationConfigs/6",
                "eventType": "OBJECT_FINALIZE",
                "payloadFormat": "JSON_API_V1",
                "bucketId": "test-alien-bucket",
                "objectId": test_key,
                "objectGeneration": "1752619211123000",
                "eventTime": event_time.to_rfc3339()
            }
        },
        "subscription": "projects/my-project/subscriptions/storage-notification-sub"
    });

    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/", ctx.transport_port))
        .header("Content-Type", "application/json")
        .body(push_body.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send wrapped GCS Pub/Sub notification")?;

    assert!(
        response.status().is_success(),
        "GCS Pub/Sub notification should succeed: {}",
        response.status()
    );

    let stored_event = check_event_stored(ctx.transport_port, "storage", &test_key)
        .await?
        .context("Wrapped GCS notification should reach the storage handler")?;
    assert_eq!(stored_event["bucket"], "test-alien-bucket");
    assert_eq!(stored_event["key"], test_key);
    assert_eq!(stored_event["eventType"], "Created");
    assert_eq!(stored_event["size"], 42);

    info!("Wrapped GCS Pub/Sub notification PASSED");
    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_cloud_scheduler(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing Cloud Scheduler event...");
    let schedule_name = format!("test-cron-{}", Uuid::new_v4());
    let schedule_time = Utc::now();

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let response = client
        .post(&url)
        .header("X-CloudScheduler", "true")
        .header("X-CloudScheduler-JobName", &schedule_name)
        .header("X-CloudScheduler-ScheduleTime", schedule_time.to_rfc3339())
        .body("")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send Cloud Scheduler event")?;

    assert!(
        response.status().is_success(),
        "Cloud Scheduler request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify the cron event was delivered to the handler and stored in KV.
    let stored_event = check_event_stored(ctx.transport_port, "cron", &schedule_name)
        .await?
        .context("Cron event should have been delivered and stored in KV")?;
    assert_eq!(stored_event["scheduleName"], schedule_name);
    assert!(stored_event["scheduledTime"].is_string());
    info!("Cloud Scheduler event PASSED");
    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_pubsub_queue_message(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing Pub/Sub CloudEvent (queue message)...");
    let message_id = format!("msg-{}", Uuid::new_v4());
    let event_time = Utc::now();

    // Base64 encode message data
    let message_content = json!({"orderId": "order-123", "amount": 50.0});
    let encoded_data = general_purpose::STANDARD.encode(message_content.to_string());

    let event_data = json!({
        "message": {
            "data": encoded_data,
            "messageId": message_id,
            "publishTime": event_time.to_rfc3339(),
            "attributes": {}
        },
        "subscription": "projects/my-project/subscriptions/test-queue-sub"
    });

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let response = client
        .post(&url)
        .header("ce-id", Uuid::new_v4().to_string())
        .header("ce-type", "google.cloud.pubsub.topic.v1.messagePublished")
        .header(
            "ce-source",
            "//pubsub.googleapis.com/projects/my-project/topics/test-queue",
        )
        .header("ce-specversion", "1.0")
        .header("ce-time", event_time.to_rfc3339())
        .header("Content-Type", "application/json")
        .body(event_data.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send Pub/Sub CloudEvent")?;

    assert!(
        response.status().is_success(),
        "Pub/Sub CloudEvent request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify message was delivered to the handler and stored in KV.
    let stored_message = check_event_stored(ctx.transport_port, "queue", &message_id)
        .await?
        .context("Queue message should have been delivered and stored in KV")?;
    assert_eq!(stored_message["messageId"], message_id);
    info!("Pub/Sub queue message PASSED");

    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_sse_streaming(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing SSE streaming response...");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/sse", ctx.transport_port);

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("Failed to send SSE request")?;

    assert!(response.status().is_success(), "SSE request should succeed");
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream"),
        "Content-Type should be text/event-stream"
    );

    // Read the SSE stream
    let body_text = response.text().await.context("Failed to read SSE body")?;

    // Count SSE data events
    let event_count = body_text
        .lines()
        .filter(|line| line.starts_with("data:"))
        .count();
    assert_eq!(event_count, 10, "Should receive 10 SSE data events");

    // Verify event content (check first and last)
    assert!(
        body_text.contains("data: sse_message_0"),
        "Should contain first event"
    );
    assert!(
        body_text.contains("data: sse_message_9"),
        "Should contain last event"
    );

    info!("SSE streaming PASSED");
    Ok(())
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
#[instrument]
async fn test_cloudrun_env_var_propagation(ctx: &mut CloudRunTestContext) -> anyhow::Result<()> {
    info!("Testing environment variable propagation...");

    let client = reqwest::Client::new();

    // Test retrieving ALIEN_WORKER_GRPC_ADDRESS which is always set by the runtime
    let url = format!(
        "http://127.0.0.1:{}/env-var/ALIEN_WORKER_GRPC_ADDRESS",
        ctx.transport_port
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send env var request")?;

    assert!(
        response.status().is_success(),
        "Env var request should succeed"
    );

    let body: serde_json::Value = response.json().await?;
    assert_eq!(body["success"], true);
    assert_eq!(body["variable"], "ALIEN_WORKER_GRPC_ADDRESS");
    assert!(body["value"].as_str().is_some(), "Should have a value");
    assert!(
        body["value"].as_str().unwrap().contains(":"),
        "Value should be a host:port address"
    );

    info!("Environment variable propagation PASSED");
    Ok(())
}

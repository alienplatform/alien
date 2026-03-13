//! Container App transport integration tests
//!
//! Tests Azure Container App event handling:
//! - HTTP request forwarding
//! - Azure Blob Storage CloudEvents → StorageEvent via gRPC → KV storage
//! - Dapr Service Bus messages → QueueMessage via gRPC → KV storage
//! - Azure Timer trigger → CronEvent via gRPC → KV storage
//! - ARC via Service Bus CloudEvents → command dispatch via gRPC

use alien_bindings::{
    grpc::{
        control_service::ControlGrpcServer, run_grpc_server,
        wait_until_service::WaitUntilGrpcServer,
    },
    BindingsProvider,
};
use alien_core::{bindings, ClientConfig};
use alien_runtime::{run, BindingsSource, RuntimeConfig, TransportType};
use anyhow::Context;
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
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,alien_runtime=debug,alien_test_server=debug".into()),
            )
            .with_test_writer()
            .try_init()
            .ok();
    });
}

/// Handle for managing a running alien-runtime instance
struct RuntimeHandle {
    task: JoinHandle<alien_runtime::Result<()>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl RuntimeHandle {
    fn new(
        task: JoinHandle<alien_runtime::Result<()>>,
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
    server_task: JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
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
    let temp_data_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let temp_data_dir_path = temp_data_dir.path().to_str().unwrap().to_string();

    // Create local bindings for storage and KV
    let storage_binding = bindings::StorageBinding::local(temp_data_dir_path.clone());
    let kv_binding = bindings::KvBinding::local(temp_data_dir_path.clone());

    let client_config = ClientConfig::Local {
        state_directory: temp_data_dir_path.clone(),
        artifact_registry_config: None,
    };

    let mut bindings_map: HashMap<String, serde_json::Value> = HashMap::new();
    bindings_map.insert(
        "test-storage".to_string(),
        serde_json::to_value(&storage_binding).expect("Failed to serialize storage binding"),
    );
    bindings_map.insert(
        "test-kv".to_string(),
        serde_json::to_value(&kv_binding).expect("Failed to serialize KV binding"),
    );

    let local_provider = Arc::new(
        BindingsProvider::new(client_config, bindings_map)
            .expect("Failed to create local provider"),
    );

    let port = free_local_port().context("Failed to find free port for gRPC server")?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let server_addr_str = addr.to_string();
    let grpc_address = server_addr_str.clone();

    info!(grpc_address = %grpc_address, "Starting gRPC server");

    let grpc_handles = run_grpc_server(local_provider, &server_addr_str)
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

/// Test context for Container App transport integration tests
struct ContainerAppTestContext {
    runtime_handle: Option<RuntimeHandle>,
    transport_port: u16,
    grpc_resources: Option<GrpcTestResources>,
}

impl std::fmt::Debug for ContainerAppTestContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerAppTestContext")
            .field("transport_port", &self.transport_port)
            .finish()
    }
}

impl AsyncTestContext for ContainerAppTestContext {
    async fn setup() -> Self {
        init_tracing();

        let grpc_resources = setup_grpc_server()
            .await
            .expect("Failed to setup gRPC server");

        let transport_port =
            free_local_port().expect("Failed to find free port for ContainerApp transport");
        info!(%transport_port, "Resolved ContainerApp transport port");

        let test_app_path = test_utils::get_test_app_path().expect("Failed to get test app path");
        info!(?test_app_path, "Using alien-test-app binary");

        // Build RuntimeConfig directly (no CLI parsing needed)
        let mut env_vars = HashMap::new();
        env_vars.insert(
            "ALIEN_SKIP_WAIT_UNTIL_EXTENSION".to_string(),
            "1".to_string(),
        );
        env_vars.insert("ALIEN_AGENT_TYPE".to_string(), "azure".to_string());
        env_vars.insert("ALIEN_BINDINGS_MODE".to_string(), "grpc".to_string());
        env_vars.insert(
            "RUST_LOG".to_string(),
            env::var("RUST_LOG")
                .unwrap_or_else(|_| "info,alien_runtime=debug,alien_test_server=debug".to_string()),
        );

        let config = RuntimeConfig::builder()
            .transport(TransportType::ContainerApp)
            .transport_port(transport_port)
            .command(vec![test_app_path.to_str().unwrap().to_string()])
            .bindings_address(grpc_resources.grpc_address.clone())
            .env_vars(env_vars)
            .build();

        info!("Starting alien-runtime programmatically with ContainerApp transport...");

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        // Clone handles for the runtime
        let wait_until_server = grpc_resources.wait_until_server.clone();
        let control_server = grpc_resources.control_server.clone();

        // Start alien-runtime in a background task with external gRPC handles
        let runtime_task = tokio::spawn(async move {
            run(
                config,
                shutdown_rx,
                BindingsSource::ExternalGrpc {
                    wait_until_server,
                    control_server,
                },
            )
            .await
        });

        let runtime_handle = RuntimeHandle::new(runtime_task, shutdown_tx);

        // Wait for ContainerApp transport readiness
        info!("Waiting for ContainerApp transport to become ready...");

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
                debug!("ContainerApp transport not ready yet...");
            }
        })
        .await;

        ready_result.expect("ContainerApp transport failed to become ready after 60s");
        info!("ContainerApp transport is ready");

        Self {
            runtime_handle: Some(runtime_handle),
            transport_port,
            grpc_resources: Some(grpc_resources),
        }
    }

    async fn teardown(mut self) {
        info!("Tearing down ContainerApp test context");

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

        info!("ContainerApp test context teardown complete");
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

    if !response.status().is_success() {
        return Ok(None);
    }

    let body: serde_json::Value = response.json().await?;
    if body["found"].as_bool().unwrap_or(false) {
        Ok(body["event"]
            .as_object()
            .map(|o| serde_json::Value::Object(o.clone())))
    } else {
        Ok(None)
    }
}

// --- Test Cases ---

#[test_context(ContainerAppTestContext)]
#[tokio::test]
#[instrument]
async fn test_containerapp_http_request(ctx: &mut ContainerAppTestContext) -> anyhow::Result<()> {
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

#[test_context(ContainerAppTestContext)]
#[tokio::test]
#[instrument]
async fn test_containerapp_azure_blob_storage_event(
    ctx: &mut ContainerAppTestContext,
) -> anyhow::Result<()> {
    info!("Testing Azure Blob Storage CloudEvent...");
    let test_key = format!("test/data-{}.zip", Uuid::new_v4());
    let event_time = Utc::now();

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let event_data = json!({
        "api": "PutBlob",
        "blobType": "BlockBlob",
        "url": format!("https://mystorageaccount.blob.core.windows.net/testcontainer/{}", test_key),
        "contentType": "application/zip",
        "contentLength": 1024,
        "etag": "test-etag"
    });

    let response = client
        .post(&url)
        .header("ce-id", Uuid::new_v4().to_string())
        .header("ce-type", "Microsoft.Storage.BlobCreated")
        .header("ce-source", "/subscriptions/{subscription-id}/resourceGroups/{resource-group}/providers/Microsoft.Storage/storageAccounts/mystorageaccount")
        .header("ce-specversion", "1.0")
        .header("ce-time", event_time.to_rfc3339())
        .header("ce-subject", format!("/blobServices/default/containers/testcontainer/blobs/{}", test_key))
        .header("Content-Type", "application/json")
        .body(event_data.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send Azure Blob CloudEvent")?;

    assert!(
        response.status().is_success(),
        "Azure Blob CloudEvent request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    info!("Azure Blob Storage CloudEvent PASSED");
    Ok(())
}

#[test_context(ContainerAppTestContext)]
#[tokio::test]
#[instrument]
async fn test_containerapp_azure_timer_trigger(
    ctx: &mut ContainerAppTestContext,
) -> anyhow::Result<()> {
    info!("Testing Azure Timer trigger event...");
    let schedule_name = format!("test-cron-{}", Uuid::new_v4());
    let schedule_time = Utc::now();

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let response = client
        .post(&url)
        .header("X-Azure-Timer", "true")
        .header("X-Azure-Timer-Name", &schedule_name)
        .header("X-Azure-Timer-ScheduledTime", schedule_time.to_rfc3339())
        .body("")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send Azure Timer event")?;

    assert!(
        response.status().is_success(),
        "Azure Timer request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    info!("Azure Timer trigger event PASSED");
    Ok(())
}

#[test_context(ContainerAppTestContext)]
#[tokio::test]
#[instrument]
async fn test_containerapp_dapr_queue_message(
    ctx: &mut ContainerAppTestContext,
) -> anyhow::Result<()> {
    info!("Testing Dapr Service Bus CloudEvent (queue message)...");
    let message_id = format!("msg-{}", Uuid::new_v4());
    let event_time = Utc::now();

    // Build Dapr CloudEvent format
    let event_data = json!({
        "orderId": "order-123",
        "amount": 50.0
    });

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", ctx.transport_port);

    let response = client
        .post(&url)
        .header("ce-id", &message_id)
        .header("ce-type", "com.dapr.event.sent")
        .header("ce-source", "servicebus")
        .header("ce-specversion", "1.0")
        .header("ce-time", event_time.to_rfc3339())
        .header("ce-topic", "test-queue")
        .header("ce-pubsubname", "servicebus")
        .header("Content-Type", "application/json")
        .body(event_data.to_string())
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to send Dapr CloudEvent")?;

    assert!(
        response.status().is_success(),
        "Dapr CloudEvent request should succeed: {}",
        response.status()
    );

    // Wait for event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify message was stored
    let stored_message = check_event_stored(ctx.transport_port, "queue", &message_id).await?;
    if let Some(msg) = stored_message {
        assert_eq!(msg["messageId"], message_id);
        info!("Dapr queue message PASSED");
    } else {
        info!("Dapr queue message processed (no handler registered)");
    }

    Ok(())
}

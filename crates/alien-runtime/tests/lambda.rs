//! Lambda transport integration tests
//!
//! Tests Lambda event handling through cargo-lambda:
//! - HTTP request forwarding (API Gateway)
//! - S3 events → StorageEvent via gRPC → KV storage
//! - SQS messages → QueueMessage via gRPC → KV storage  
//! - CloudWatch scheduled events → CronEvent via gRPC → KV storage
//! - commands envelope detection → command dispatch via gRPC → response submission

use anyhow::Context;
use aws_lambda_events::{
    cloudwatch_events::CloudWatchEvent,
    s3::{
        S3Bucket, S3Entity, S3Event, S3EventRecord, S3Object, S3RequestParameters, S3UserIdentity,
    },
    sqs::{SqsEvent, SqsMessage},
};
use backon::{ConstantBuilder, Retryable};
use chrono::{TimeZone, Utc};
use lambda_http::aws_lambda_events::apigw::ApiGatewayV2httpResponse;
use port_check::free_local_port;
use serde_json::json;
use snapbox::cmd::Command as SnapboxCommand;
use std::process::Command as StdCommand;
use std::{
    collections::HashMap,
    env,
    io::{self},
    process::{Child, Stdio},
    sync::Once,
    time::Duration,
};
use tempfile::{self, TempDir};
use test_context::AsyncTestContext;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

mod test_utils;

// Commands testing imports
use alien_commands::{
    server::CommandDispatcher,
    test_utils::TestCommandServer,
    types::{BodySpec, CommandState, CreateCommandRequest, Envelope, UploadCompleteRequest},
    Result as ArcResult,
};

static TRACING_INIT: Once = Once::new();

/// Initializes tracing subscriber for tests.
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

fn cargo_lambda_available() -> bool {
    StdCommand::new("cargo")
        .arg("lambda")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

// ===========================================
// CARGO LAMBDA INVOKE DISPATCHER
// ===========================================

/// A test dispatcher that uses `cargo lambda invoke` to send commands envelopes to a Lambda function.
/// This simulates the real AWS Lambda InvokeFunction API call.
#[derive(Debug)]
pub struct CargoLambdaInvokeDispatcher {
    /// The port where cargo lambda invoke is listening
    invoke_port: u16,
}

impl CargoLambdaInvokeDispatcher {
    pub fn new(invoke_port: u16) -> Self {
        Self { invoke_port }
    }
}

#[async_trait::async_trait]
impl CommandDispatcher for CargoLambdaInvokeDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> ArcResult<()> {
        use alien_commands::error::ErrorData as ArcErrorData;

        info!(
            command_id = %envelope.command_id,
            command = %envelope.command,
            invoke_port = %self.invoke_port,
            "Dispatching commands envelope via cargo lambda invoke"
        );

        // Serialize the commands envelope as JSON payload for Lambda
        let envelope_json = serde_json::to_string(envelope).map_err(|e| {
            alien_error::AlienError::new(ArcErrorData::TransportDispatchFailed {
                message: format!(
                    "Failed to serialize commands envelope for Lambda invoke: {}",
                    e
                ),
                transport_type: Some("cargo-lambda-invoke".to_string()),
                target: Some(envelope.command.clone()),
            })
        })?;

        // Fire and forget: spawn the lambda invoke asynchronously
        let invoke_port = self.invoke_port;
        let command_id = envelope.command_id.clone();
        let command = envelope.command.clone();

        tokio::spawn(async move {
            match invoke_lambda_with_envelope(invoke_port, &envelope_json).await {
                Ok(result) => {
                    if result.status_code == 200 {
                        debug!(
                            command_id = %command_id,
                            command = %command,
                            status_code = %result.status_code,
                            "Successfully invoked Lambda via cargo lambda invoke"
                        );
                    } else {
                        error!(
                            command_id = %command_id,
                            command = %command,
                            status_code = %result.status_code,
                            "Lambda invoke returned non-200 status"
                        );
                    }
                }
                Err(e) => {
                    error!(
                        command_id = %command_id,
                        command = %command,
                        error = %e,
                        "Failed to invoke Lambda via cargo lambda invoke"
                    );
                }
            }
        });

        debug!(
            command_id = %envelope.command_id,
            command = %envelope.command,
            "commands envelope dispatched asynchronously via cargo lambda invoke"
        );

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Invokes the lambda function via `cargo lambda invoke` with an commands envelope JSON payload.
#[instrument(skip(envelope_json))]
async fn invoke_lambda_with_envelope(
    invoke_port: u16,
    envelope_json: &str,
) -> anyhow::Result<ApiGatewayV2httpResponse> {
    debug!(%envelope_json, "Sending commands envelope via cargo lambda invoke");

    let task = || async {
        let invoke_assert = SnapboxCommand::new("cargo")
            .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
            .arg("lambda")
            .arg("invoke")
            .arg("--data-ascii")
            .arg(envelope_json)
            .arg("--invoke-port")
            .arg(invoke_port.to_string())
            .arg("--output-format")
            .arg("json")
            .env_remove("RUST_LOG")
            .assert()
            .success();

        let output = invoke_assert.get_output();
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        if !stderr_str.is_empty() {
            warn!(target: "lambda_invoke_stderr", "Stderr from commands envelope invoke: {}", stderr_str);
        }
        debug!(%stdout_str, "Received commands envelope response from cargo lambda invoke");

        let response: ApiGatewayV2httpResponse =
            serde_json::from_str(&stdout_str).with_context(|| {
                format!(
                    "Failed to parse commands envelope invoke stdout: '{}', stderr: '{}'",
                    stdout_str, stderr_str
                )
            })?;

        Ok::<ApiGatewayV2httpResponse, anyhow::Error>(response)
    };

    let retry_policy = ConstantBuilder::new()
        .with_delay(Duration::from_secs(2))
        .with_max_times(5);

    task.retry(&retry_policy)
        .await
        .context("commands envelope Lambda invocation failed after retries")
}

// --- Child Process Management ---

/// Kills a process group on Unix, or just the process on other platforms
fn kill_process_tree(child: &mut Child) {
    let pid = child.id();

    #[cfg(unix)]
    {
        // Kill the entire process group (negative PID)
        // This ensures we kill cargo lambda watch AND alien-runtime AND alien-test-server
        let pgid = pid as i32;
        info!(pid, pgid, "Killing process group");
        unsafe {
            // First try SIGTERM to allow graceful shutdown
            libc::killpg(pgid, libc::SIGTERM);
        }
        // Give processes a moment to terminate gracefully
        std::thread::sleep(Duration::from_millis(500));
        unsafe {
            // Then SIGKILL to force termination
            libc::killpg(pgid, libc::SIGKILL);
        }
    }

    #[cfg(not(unix))]
    {
        info!(pid, "Killing child process");
        let _ = child.kill();
    }
}

/// Waits for a child process to exit with a timeout
fn wait_for_process_exit(child: &mut Child, timeout: Duration) -> bool {
    let pid = child.id();
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(100);

    while start.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(status)) => {
                info!(pid, ?status, "Child process exited");
                return true;
            }
            Ok(None) => {
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::InvalidInput {
                    // Process already reaped
                    debug!(pid, "Child process already reaped");
                    return true;
                }
                error!(pid, "Error waiting for child process: {}", e);
                return false;
            }
        }
    }

    warn!(pid, "Child process did not exit within timeout");
    false
}

// --- Test Context ---

/// Test context for Lambda tests that manages the cargo lambda watch process
pub struct LambdaTestContext {
    child: Option<Child>,
    pub invoke_port: u16,
    pub temp_dir: TempDir,
    skip: bool,
}

impl LambdaTestContext {
    /// Creates a new Lambda test context with Command server for command testing
    pub async fn with_command_server(&self) -> anyhow::Result<TestCommandServer> {
        // Create the cargo lambda invoke dispatcher for testing
        let dispatcher = std::sync::Arc::new(CargoLambdaInvokeDispatcher::new(self.invoke_port));

        // Create Command server
        let command_server = TestCommandServer::builder()
            .with_dispatcher(dispatcher)
            .build()
            .await;

        info!(
            command_server_url = %command_server.base_url(),
            "Command server started for lambda tests with cargo lambda invoke dispatcher"
        );

        Ok(command_server)
    }
}

impl AsyncTestContext for LambdaTestContext {
    async fn setup() -> Self {
        init_tracing();

        if !cargo_lambda_available() {
            eprintln!("Skipping lambda tests: cargo-lambda is not available");
            return Self {
                child: None,
                invoke_port: 0,
                temp_dir: tempfile::tempdir().expect("Failed to create temp dir"),
                skip: true,
            };
        }

        // Create temp directory for local bindings state
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let invoke_port =
            free_local_port().expect("Failed to find free port for cargo-lambda invoke");
        info!(%invoke_port, "Resolved invoke port");

        let test_app_path = test_utils::get_test_app_path().expect("Failed to get test app path");
        info!(?test_app_path, "Using alien-test-app binary");

        // Create local storage binding JSON (uses serde tag "service" and camelCase fields)
        let storage_binding = serde_json::json!({
            "service": "local",
            "storageUrl": null,
            "dataDir": temp_dir_path
        });

        // Create local KV binding JSON (uses serde tag "service" and camelCase fields)
        let kv_binding = serde_json::json!({
            "service": "local",
            "dataDir": temp_dir_path
        });

        // Build cargo lambda watch command
        // Note: cargo lambda watch runs alien-runtime which spawns the test server
        let mut lambda_cmd = StdCommand::new("cargo");
        lambda_cmd
            .arg("lambda")
            .arg("watch")
            .arg("--ignore-changes")
            .arg("-p")
            .arg("alien-runtime")
            .arg("--bin")
            .arg("alien-runtime")
            .arg(format!("--invoke-port={}", invoke_port))
            .arg("--")
            // CLI arguments for alien-runtime
            .arg("--transport")
            .arg("lambda")
            .arg("--lambda-mode")
            .arg("buffered")
            .arg("--")
            // The application to run
            .arg(test_app_path.to_str().unwrap())
            // Environment variables for local bindings
            .env("ALIEN_DEPLOYMENT_TYPE", "local")
            .env("ALIEN_LOCAL_STATE_DIRECTORY", &temp_dir_path)
            .env("ALIEN_TEST_STORAGE_BINDING", storage_binding.to_string())
            .env("ALIEN_TEST_ALIEN_KV_BINDING", kv_binding.to_string())
            .env("ALIEN_SKIP_WAIT_UNTIL_EXTENSION", "1")
            .env(
                "RUST_LOG",
                env::var("RUST_LOG").unwrap_or_else(|_| {
                    "info,alien_runtime=debug,alien_test_server=debug".to_string()
                }),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        // On Unix, create a new process group so we can kill the entire tree
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            lambda_cmd.process_group(0);
        }

        info!("Spawning command: {:?}", lambda_cmd);
        let child = lambda_cmd
            .spawn()
            .expect("Failed to spawn cargo lambda watch");
        info!(pid = child.id(), "Spawned child process");

        // Wait for Lambda readiness
        info!("Waiting for cargo lambda to become ready...");

        let ready_check = || async {
            let client = reqwest::Client::new();
            let health_url = format!("http://127.0.0.1:{}/health", invoke_port);
            match client
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            }
        };

        let ready_result = tokio::time::timeout(Duration::from_secs(120), async {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                if ready_check().await {
                    break;
                }
                debug!("Lambda not ready yet, retrying...");
            }
        })
        .await;

        ready_result.expect("Lambda process failed to become ready after 120s");
        info!("Lambda process is ready");

        Self {
            child: Some(child),
            invoke_port,
            temp_dir,
            skip: false,
        }
    }

    async fn teardown(mut self) {
        if self.skip {
            return;
        }

        let Some(mut child) = self.child.take() else {
            return;
        };

        let pid = child.id();
        info!(pid, "Tearing down Lambda test context");

        // Kill the entire process tree
        kill_process_tree(&mut child);

        // Wait for the process to actually exit
        if !wait_for_process_exit(&mut child, Duration::from_secs(10)) {
            error!(pid, "Process did not exit cleanly after teardown");
        }

        info!(pid, "Lambda test context teardown complete");
    }
}

fn skip_if_unavailable(ctx: &LambdaTestContext) -> bool {
    if ctx.skip {
        eprintln!("Skipping lambda test: cargo-lambda is not available");
        true
    } else {
        false
    }
}

// --- Test Invocation Helpers ---

/// Invokes the lambda function via `cargo lambda invoke` with a JSON payload.
#[instrument(skip(json_payload))]
async fn invoke_lambda_with_json(
    invoke_port: u16,
    json_payload: &str,
) -> anyhow::Result<ApiGatewayV2httpResponse> {
    debug!(%json_payload, "Sending invoke payload JSON");

    let task = || async {
        let invoke_assert = SnapboxCommand::new("cargo")
            .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()))
            .arg("lambda")
            .arg("invoke")
            .arg("--data-ascii")
            .arg(json_payload)
            .arg("--invoke-port")
            .arg(invoke_port.to_string())
            .arg("--output-format")
            .arg("json")
            .env_remove("RUST_LOG")
            .assert()
            .success();

        let output = invoke_assert.get_output();
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        if !stderr_str.is_empty() {
            warn!(target: "lambda_invoke_stderr", "Stderr from invoke: {}", stderr_str);
        }
        debug!(%stdout_str, "Received response JSON from cargo lambda invoke");

        let response: ApiGatewayV2httpResponse =
            serde_json::from_str(&stdout_str).with_context(|| {
                format!(
                    "Failed to parse invoke stdout: '{}', stderr: '{}'",
                    stdout_str, stderr_str
                )
            })?;

        Ok::<ApiGatewayV2httpResponse, anyhow::Error>(response)
    };

    let retry_policy = ConstantBuilder::new()
        .with_delay(Duration::from_secs(5))
        .with_max_times(9);

    task.retry(&retry_policy)
        .await
        .context("Lambda invocation failed after retries")
}

/// Check if an event was stored in KV via the new /events/* endpoints
async fn check_event_stored(
    invoke_port: u16,
    event_type: &str,
    event_key: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let client = reqwest::Client::new();
    let url = format!(
        "http://127.0.0.1:{}/events/{}/{}",
        invoke_port, event_type, event_key
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

#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_http_request(ctx: &mut LambdaTestContext) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let invoke_port = ctx.invoke_port;

    info!("Testing HTTP request invocation...");
    let test_id = Uuid::new_v4().to_string();

    let client = reqwest::Client::new();
    let invoke_url = format!("http://127.0.0.1:{}/inspect", invoke_port);
    info!("Sending direct POST request to {}", invoke_url);

    let task = || async {
        let res = client
            .post(&invoke_url)
            .header("Content-Type", "application/json")
            .body(json!({ "test_id": test_id }).to_string())
            .send()
            .await
            .context("Failed to send request to lambda invoke port")?;

        let status = res.status();
        debug!(%status, "Received HTTP response from invoke port");
        anyhow::ensure!(
            status.is_success(),
            "HTTP invoke request failed with status {}",
            status
        );

        let res_body = res
            .json::<serde_json::Value>()
            .await
            .context("Failed to read response body from lambda invoke port")?;

        Ok(res_body)
    };

    let retry_policy = ConstantBuilder::new()
        .with_delay(Duration::from_secs(5))
        .with_max_times(9);

    let res_body = task
        .retry(&retry_policy)
        .await
        .context("HTTP request invocation failed after retries")?;

    assert_eq!(res_body["success"], true);
    assert_eq!(res_body["requestBody"]["test_id"], test_id);

    info!("HTTP request invocation PASSED");
    Ok(())
}

#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_s3_storage_event(ctx: &mut LambdaTestContext) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let invoke_port = ctx.invoke_port;

    info!("Testing S3 storage event invocation...");
    let s3_event_time = Utc.with_ymd_and_hms(2024, 5, 1, 10, 0, 0).unwrap();
    let test_key = format!("test/data-{}.zip", Uuid::new_v4());

    let s3_event = S3Event {
        records: vec![S3EventRecord {
            event_version: Some("2.1".to_string()),
            event_source: Some("aws:s3".to_string()),
            aws_region: Some("us-east-1".to_string()),
            event_time: s3_event_time,
            event_name: Some("ObjectCreated:Put".to_string()),
            principal_id: S3UserIdentity {
                principal_id: Some("EXAMPLE_PRINCIPAL".to_string()),
            },
            request_parameters: S3RequestParameters {
                source_ip_address: Some("198.51.100.1".to_string()),
            },
            response_elements: HashMap::from([
                ("x-amz-request-id".to_string(), "EXAMPLE_REQ_ID".to_string()),
                ("x-amz-id-2".to_string(), "EXAMPLE_ID2".to_string()),
            ]),
            s3: S3Entity {
                configuration_id: Some("test-config-id".to_string()),
                bucket: S3Bucket {
                    name: Some("test-alien-bucket".to_string()),
                    owner_identity: Some(S3UserIdentity {
                        principal_id: Some("EXAMPLE_OWNER".to_string()),
                    }),
                    arn: Some("arn:aws:s3:::test-alien-bucket".to_string()),
                    ..Default::default()
                },
                object: S3Object {
                    key: Some(test_key.clone()),
                    size: Some(1024),
                    e_tag: Some("d41d8cd98f00b204e9800998ecf8427e".to_string()),
                    version_id: None,
                    sequencer: Some("0055AED6DCD90281E5".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }],
    };

    let s3_request_json =
        serde_json::to_string(&s3_event).context("Failed to serialize S3 event")?;
    let _ = invoke_lambda_with_json(invoke_port, &s3_request_json).await?;

    // Allow time for async event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify event was stored in KV via new endpoint
    let stored_event = check_event_stored(invoke_port, "storage", &test_key)
        .await?
        .context("Storage event should have been stored in KV")?;

    assert_eq!(stored_event["bucket"], "test-alien-bucket");
    assert_eq!(stored_event["key"], test_key);
    assert_eq!(stored_event["eventType"], "ObjectCreated:Put");

    info!("S3 storage event invocation PASSED");
    Ok(())
}

#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_cloudwatch_scheduled_event(ctx: &mut LambdaTestContext) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let invoke_port = ctx.invoke_port;

    info!("Testing CloudWatch scheduled event invocation...");
    let scheduled_event_time = Utc.with_ymd_and_hms(2024, 5, 1, 11, 30, 0).unwrap();
    let schedule_name = format!("test-cron-{}", Uuid::new_v4());

    let scheduled_event = CloudWatchEvent {
        version: Some("0".to_string()),
        id: Some(Uuid::new_v4().to_string()),
        detail_type: Some("Scheduled Event".to_string()),
        source: Some("aws.events".to_string()),
        account_id: Some("123456789012".to_string()),
        time: scheduled_event_time,
        region: Some("eu-west-1".to_string()),
        resources: vec![format!(
            "arn:aws:events:eu-west-1:123456789012:rule/{}",
            schedule_name
        )],
        detail: Some(serde_json::json!({})),
    };

    let scheduled_request_json = serde_json::to_string(&scheduled_event)?;
    let _ = invoke_lambda_with_json(invoke_port, &scheduled_request_json).await?;

    // Allow time for async event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify event was stored in KV
    let stored_event = check_event_stored(
        invoke_port,
        "cron",
        &format!(
            "arn:aws:events:eu-west-1:123456789012:rule/{}",
            schedule_name
        ),
    )
    .await?
    .context("Cron event should have been stored in KV")?;

    assert!(stored_event["scheduledTime"].is_string());

    info!("CloudWatch scheduled event invocation PASSED");
    Ok(())
}

#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_sqs_queue_message(ctx: &mut LambdaTestContext) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let invoke_port = ctx.invoke_port;

    info!("Testing SQS queue message invocation...");

    let message_id = format!("msg-test-{}", Uuid::new_v4());
    let message_body = json!({
        "orderId": "order-12345",
        "customerEmail": "test@example.com",
        "items": ["item1", "item2"],
        "total": 99.99
    });

    let sqs_event = SqsEvent {
        records: vec![SqsMessage {
            message_id: Some(message_id.clone()),
            receipt_handle: Some("receipt-handle-12345".to_string()),
            body: Some(message_body.to_string()),
            attributes: {
                let mut attrs = HashMap::new();
                attrs.insert("SentTimestamp".to_string(), "1640995200000".to_string());
                attrs.insert("ApproximateReceiveCount".to_string(), "1".to_string());
                attrs
            },
            message_attributes: HashMap::new(),
            md5_of_body: Some("test-md5".to_string()),
            md5_of_message_attributes: None,
            event_source: Some("aws:sqs".to_string()),
            event_source_arn: Some("arn:aws:sqs:us-east-1:123456789012:test-queue".to_string()),
            aws_region: Some("us-east-1".to_string()),
        }],
    };

    let sqs_request_json = serde_json::to_string(&sqs_event)?;
    let _ = invoke_lambda_with_json(invoke_port, &sqs_request_json).await?;

    // Allow time for async event processing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify message was stored in KV
    let stored_message = check_event_stored(invoke_port, "queue", &message_id)
        .await?
        .context("Queue message should have been stored in KV")?;

    assert_eq!(stored_message["messageId"], message_id);

    info!("SQS queue message invocation PASSED");
    Ok(())
}

// ===========================================
// COMMANDS PROTOCOL LAMBDA TESTS
// ===========================================

/// Test commands: Small params + Small response (both inline)
#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_cmd_small_params_small_response(
    ctx: &mut LambdaTestContext,
) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let command_server = ctx.with_command_server().await?;

    info!("Testing commands: Small params + Small response (both inline)");

    let params = json!({
        "message": "Small test params",
        "data": {"key": "value"},
        "size": "small"
    });
    let params_bytes = serde_json::to_vec(&params)?;

    let request = CreateCommandRequest {
        deployment_id: "lambda-test-deployment".to_string(),
        command: "cmd-test-small".to_string(),
        params: BodySpec::inline(&params_bytes),
        deadline: None,
        idempotency_key: None,
    };

    // Create command - should auto-dispatch via CargoLambdaInvokeDispatcher
    let response = command_server.create_command(request).await?;
    assert_eq!(response.state, CommandState::Dispatched);
    assert!(response.storage_upload.is_none()); // Should be inline

    // Wait for completion
    let final_status = command_server
        .wait_for_completion(&response.command_id, Duration::from_secs(30))
        .await
        .context("Command did not complete within timeout")?;

    assert_eq!(final_status.state, CommandState::Succeeded);

    let final_response = final_status.response.unwrap();
    assert!(final_response.is_success());

    if let alien_commands::CommandResponse::Success { response: body } = final_response {
        let response_data = body.decode_inline().expect("Response should be inline");
        let response_json: serde_json::Value = serde_json::from_slice(&response_data)?;

        assert_eq!(response_json["success"], true);
        assert_eq!(response_json["testType"], "cmd-small-payload");

        // Validate params hash - handler re-serializes params to JSON string before hashing
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_string(&params)?.as_bytes());
        let expected_hash = format!("{:x}", hasher.finalize());
        assert_eq!(response_json["paramsHash"], expected_hash);
    }

    info!("Commands: Small params + Small response PASSED");
    Ok(())
}

/// Test commands: Small params + Large response (inline params, storage response)
#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_cmd_small_params_large_response(
    ctx: &mut LambdaTestContext,
) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let command_server = ctx.with_command_server().await?;

    info!("Testing commands: Small params + Large response (inline params, storage response)");

    let params = json!({
        "message": "Small params, generate large response",
        "generate_large_response": true
    });
    let params_bytes = serde_json::to_vec(&params)?;

    let request = CreateCommandRequest {
        deployment_id: "lambda-test-deployment".to_string(),
        command: "cmd-test-large".to_string(), // Uses large response handler
        params: BodySpec::inline(&params_bytes),
        deadline: None,
        idempotency_key: None,
    };

    let response = command_server.create_command(request).await?;
    assert_eq!(response.state, CommandState::Dispatched);
    assert!(response.storage_upload.is_none()); // Params should be inline

    let final_status = command_server
        .wait_for_completion(&response.command_id, Duration::from_secs(30))
        .await
        .context("Command did not complete within timeout")?;

    assert_eq!(final_status.state, CommandState::Succeeded);

    let final_response = final_status.response.unwrap();
    assert!(final_response.is_success());

    if let alien_commands::CommandResponse::Success { response: body } = final_response {
        // Large response should use storage
        assert!(matches!(body, BodySpec::Storage { .. }));

        if let BodySpec::Storage {
            size,
            storage_get_request,
            ..
        } = body
        {
            assert!(size.unwrap_or(0) > 48000, "Response should be > 48KB");
            assert!(
                storage_get_request.is_some(),
                "Should have storage get request"
            );

            // Download and verify large response content
            let storage_response = storage_get_request.unwrap().execute(None).await?;
            assert_eq!(storage_response.status_code, 200);
            let response_data = storage_response.body.unwrap();
            let response_json: serde_json::Value = serde_json::from_slice(&response_data)?;

            assert_eq!(response_json["success"], true);
            assert_eq!(response_json["testType"], "cmd-large-payload");
            assert!(response_json["largeResponseData"].is_string());
            assert!(response_json["bulkData"].is_array());
        }
    }

    info!("Commands: Small params + Large response PASSED");
    Ok(())
}

/// Test commands: Large params + Small response (storage params, inline response)
#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_cmd_large_params_small_response(
    ctx: &mut LambdaTestContext,
) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let command_server = ctx.with_command_server().await?;

    info!("Testing commands: Large params + Small response (storage params, inline response)");

    // Create large JSON params that exceed inline limit (>150KB)
    // Command params are JSON - we need valid JSON data
    // 8000 items × ~25 chars = ~200KB
    let large_data: Vec<String> = (0..8000)
        .map(|i| format!("large-data-item-{:06}", i))
        .collect();
    let large_params = json!({
        "message": "Large params test for storage-based transfer",
        "testType": "large-params-small-response",
        "bulkData": large_data,
    });
    let large_params_bytes = serde_json::to_vec(&large_params)?;
    assert!(
        large_params_bytes.len() > 150_000,
        "Params should exceed 150KB inline limit (actual: {} bytes)",
        large_params_bytes.len()
    );

    let request = CreateCommandRequest {
        deployment_id: "lambda-test-deployment".to_string(),
        command: "cmd-test-small".to_string(), // Small response handler
        params: BodySpec::storage(large_params_bytes.len() as u64),
        deadline: None,
        idempotency_key: None,
    };

    // Create command - should require upload
    let response = command_server.create_command(request).await?;
    assert_eq!(response.state, CommandState::PendingUpload);
    assert!(response.storage_upload.is_some());

    // Upload large params
    let storage_upload = response.storage_upload.unwrap();
    storage_upload
        .put_request
        .execute(Some(large_params_bytes.clone().into()))
        .await?;

    // Complete upload - should auto-dispatch
    let upload_complete = UploadCompleteRequest {
        size: large_params_bytes.len() as u64,
    };
    command_server
        .upload_complete(&response.command_id, upload_complete)
        .await?;

    // Wait for completion
    let final_status = command_server
        .wait_for_completion(&response.command_id, Duration::from_secs(30))
        .await
        .context("Command did not complete within timeout")?;

    assert_eq!(final_status.state, CommandState::Succeeded);

    let final_response = final_status.response.unwrap();
    assert!(final_response.is_success());

    if let alien_commands::CommandResponse::Success { response: body } = final_response {
        // Small response should be inline
        let response_data = body.decode_inline().expect("Response should be inline");
        let response_json: serde_json::Value = serde_json::from_slice(&response_data)?;

        assert_eq!(response_json["success"], true);
        assert_eq!(response_json["testType"], "cmd-small-payload");

        // Validate params hash - handler re-serializes params to JSON string before hashing
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        // The handler deserializes then re-serializes, so we hash the re-serialized form
        hasher.update(serde_json::to_string(&large_params)?.as_bytes());
        let expected_hash = format!("{:x}", hasher.finalize());
        assert_eq!(response_json["paramsHash"], expected_hash);
    }

    info!("Commands: Large params + Small response PASSED");
    Ok(())
}

/// Test commands: Large params + Large response (both storage)
#[test_context::test_context(LambdaTestContext)]
#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_lambda_cmd_large_params_large_response(
    ctx: &mut LambdaTestContext,
) -> anyhow::Result<()> {
    if skip_if_unavailable(ctx) {
        return Ok(());
    }

    let command_server = ctx.with_command_server().await?;

    info!("Testing commands: Large params + Large response (both storage)");

    // Create large JSON params that exceed inline limit (>150KB)
    // 8000 items × ~25 chars = ~200KB
    let large_data: Vec<String> = (0..8000)
        .map(|i| format!("bulk-params-item-{:06}", i))
        .collect();
    let large_params = json!({
        "message": "Large params test for both storage-based transfer",
        "testType": "large-params-large-response",
        "bulkData": large_data,
    });
    let large_params_bytes = serde_json::to_vec(&large_params)?;
    assert!(
        large_params_bytes.len() > 150_000,
        "Params should exceed 150KB inline limit (actual: {} bytes)",
        large_params_bytes.len()
    );

    let request = CreateCommandRequest {
        deployment_id: "lambda-test-deployment".to_string(),
        command: "cmd-test-large".to_string(), // Large response handler
        params: BodySpec::storage(large_params_bytes.len() as u64),
        deadline: None,
        idempotency_key: None,
    };

    let response = command_server.create_command(request).await?;
    assert_eq!(response.state, CommandState::PendingUpload);
    assert!(response.storage_upload.is_some());

    // Upload large params
    let storage_upload = response.storage_upload.unwrap();
    storage_upload
        .put_request
        .execute(Some(large_params_bytes.clone().into()))
        .await?;

    // Complete upload
    let upload_complete = UploadCompleteRequest {
        size: large_params_bytes.len() as u64,
    };
    command_server
        .upload_complete(&response.command_id, upload_complete)
        .await?;

    // Wait for completion
    let final_status = command_server
        .wait_for_completion(&response.command_id, Duration::from_secs(35))
        .await
        .context("Command did not complete within timeout")?;

    assert_eq!(final_status.state, CommandState::Succeeded);

    let final_response = final_status.response.unwrap();
    assert!(final_response.is_success());

    if let alien_commands::CommandResponse::Success { response: body } = final_response {
        // Large response should use storage
        assert!(matches!(body, BodySpec::Storage { .. }));

        if let BodySpec::Storage {
            size,
            storage_get_request,
            ..
        } = body
        {
            assert!(size.unwrap_or(0) > 48000, "Response should be > 48KB");
            assert!(storage_get_request.is_some());

            // Download and verify
            let storage_response = storage_get_request.unwrap().execute(None).await?;
            assert_eq!(storage_response.status_code, 200);
            let response_data = storage_response.body.unwrap();
            let response_json: serde_json::Value = serde_json::from_slice(&response_data)?;

            assert_eq!(response_json["success"], true);
            assert_eq!(response_json["testType"], "cmd-large-payload");

            // Validate params hash - handler re-serializes params to JSON string before hashing
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(serde_json::to_string(&large_params)?.as_bytes());
            let expected_hash = format!("{:x}", hasher.finalize());
            assert_eq!(response_json["paramsHash"], expected_hash);
        }
    }

    info!("Commands: Large params + Large response PASSED");
    Ok(())
}

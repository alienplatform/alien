//! Integration tests for the pull command `Receiver` against the real
//! in-crate command server (the same `TestCommandServer` harness the
//! ALIEN-219 integration tests use): lease over HTTP, dispatch to in-process
//! handlers, submit responses through the envelope's real inline/presigned
//! flow.

#![cfg(all(feature = "receiver", feature = "test-utils"))]

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_commands::receiver::{
    box_handler, process_lease, Context, HandlerError, Receiver, ERROR_CODE_HANDLER_ERROR,
    ERROR_CODE_HANDLER_TIMEOUT, ERROR_CODE_UNKNOWN_COMMAND,
};
use alien_commands::test_utils::{test_inline_create_command, TestCommandServer};
use alien_commands::types::{
    BodySpec, CommandResponse, CommandState, CommandTarget, CommandTargetType, Envelope, LeaseInfo,
    ResponseHandling,
};
use alien_commands::{INLINE_MAX_BYTES, PROTOCOL_VERSION};
use alien_core::presigned::{PresignedOperation, PresignedRequest};
use alien_core::{
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL, ENV_ALIEN_DEPLOYMENT_ID,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tracing_subscriber::fmt::MakeWriter;

const DEPLOYMENT_ID: &str = "pull-agent";
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Pull-mode server + the env map a receiver for its default daemon target
/// would be injected with.
async fn pull_server_and_env() -> (TestCommandServer, HashMap<String, String>) {
    let server = TestCommandServer::builder().with_pull_mode().build().await;
    let env = HashMap::from([
        (
            ENV_ALIEN_COMMANDS_URL.to_string(),
            server.command_base_url(),
        ),
        (
            ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            "test-token".to_string(),
        ),
        (
            ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            DEPLOYMENT_ID.to_string(),
        ),
        (
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            server.default_target.resource_id.clone(),
        ),
        (
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
            "daemon".to_string(),
        ),
    ]);
    (server, env)
}

fn expect_error_response(response: &CommandResponse, expected_code: &str) -> String {
    match response {
        CommandResponse::Error { code, message, .. } => {
            assert_eq!(code, expected_code, "unexpected error code: {message}");
            message.clone()
        }
        CommandResponse::Success { .. } => panic!("expected {expected_code} error response"),
    }
}

/// Happy path: registered handler receives decoded JSON input plus the full
/// context (command_id, attempt, budget deadline), and its serialized return
/// round-trips to the caller as a SUCCEEDED inline response.
#[tokio::test]
async fn receiver_round_trips_success_response() {
    let (server, env) = pull_server_and_env().await;

    let (ctx_tx, mut ctx_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL);
    receiver.handle("sync-data", move |ctx| {
        let ctx_tx = ctx_tx.clone();
        async move {
            let input: serde_json::Value = ctx.input_json()?;
            ctx_tx
                .send((ctx.command_id.clone(), ctx.attempt, ctx.deadline))
                .expect("test channel open");
            Ok(serde_json::json!({
                "status": "synced",
                "echoedTestData": input["testData"],
            }))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let request = test_inline_create_command(DEPLOYMENT_ID, "sync-data");
    let created = server.create_command(request).await.expect("create");

    let final_status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(5))
        .await
        .expect("command should complete");
    assert_eq!(final_status.state, CommandState::Succeeded);

    let response = final_status.response.expect("response present");
    let CommandResponse::Success { response: body } = response else {
        panic!("expected success response");
    };
    let bytes = body.decode_inline().expect("inline response body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON response");
    assert_eq!(json["status"], "synced");
    assert_eq!(
        json["echoedTestData"],
        "This is test command params for validation"
    );

    // Handler context carried the real command identity and a budget.
    let (command_id, attempt, deadline) = ctx_rx.recv().await.expect("handler ran");
    assert_eq!(command_id, created.command_id);
    assert_eq!(attempt, 1);
    let budget = deadline.expect("budget deadline always set under leases");
    assert!(budget > chrono::Utc::now(), "budget must be in the future");

    shutdown.shutdown();
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");
}

/// A leased command with no registered handler is answered with an
/// UNKNOWN_COMMAND error response (recorded contract for the TS twin).
#[tokio::test]
async fn receiver_submits_unknown_command_error() {
    let (server, env) = pull_server_and_env().await;

    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL);
    receiver.handle(
        "something-else",
        |_ctx| async move { Ok(serde_json::json!({})) },
    );
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let request = test_inline_create_command(DEPLOYMENT_ID, "not-registered");
    let created = server.create_command(request).await.expect("create");

    let final_status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(5))
        .await
        .expect("command should complete");
    assert_eq!(final_status.state, CommandState::Failed);
    let message = expect_error_response(
        &final_status.response.expect("response present"),
        ERROR_CODE_UNKNOWN_COMMAND,
    );
    assert!(message.contains("not-registered"), "got: {message}");

    shutdown.shutdown();
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");
}

/// Budget = min(deadline, lease expiry): with a 1s lease and no envelope
/// deadline, a slow handler is aborted at lease expiry and HANDLER_TIMEOUT
/// is submitted. (The deadline side of min() is unit-tested in-module.)
#[tokio::test]
async fn receiver_aborts_handler_at_lease_expiry_budget() {
    let (server, env) = pull_server_and_env().await;

    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL)
        .with_lease_seconds(1);
    receiver.handle("slow-command", |_ctx| async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        Ok(serde_json::json!({ "should": "never happen" }))
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let request = test_inline_create_command(DEPLOYMENT_ID, "slow-command");
    let created = server.create_command(request).await.expect("create");

    let final_status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(10))
        .await
        .expect("command should complete");
    assert_eq!(final_status.state, CommandState::Failed);
    let message = expect_error_response(
        &final_status.response.expect("response present"),
        ERROR_CODE_HANDLER_TIMEOUT,
    );
    assert!(message.contains("slow-command"), "got: {message}");

    shutdown.shutdown();
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");
}

/// Redelivery passes the server's attempt count through to ctx.attempt:
/// lease #1 is released back (attempt incremented), the receiver's lease
/// then observes attempt 2.
#[tokio::test]
async fn receiver_passes_attempt_through_on_redelivery() {
    let (server, env) = pull_server_and_env().await;

    // First delivery: lease directly (as if a previous receiver instance
    // died) and release it back, which increments the attempt counter.
    let request = test_inline_create_command(DEPLOYMENT_ID, "retry-me");
    let created = server.create_command(request).await.expect("create");
    let first_lease = server
        .acquire_single_lease(DEPLOYMENT_ID)
        .await
        .expect("lease acquisition")
        .expect("one pending command");
    assert_eq!(first_lease.command_id, created.command_id);
    let first_attempt = first_lease.attempt;
    server
        .release_lease(&first_lease.command_id, &first_lease.lease_id)
        .await
        .expect("release lease");

    // Second delivery: through the receiver.
    let (attempt_tx, mut attempt_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL);
    receiver.handle("retry-me", move |ctx| {
        let attempt_tx = attempt_tx.clone();
        async move {
            attempt_tx.send(ctx.attempt).expect("test channel open");
            Ok(serde_json::json!({ "attempt": ctx.attempt }))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let final_status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(5))
        .await
        .expect("command should complete");
    assert_eq!(final_status.state, CommandState::Succeeded);

    let handler_attempt = attempt_rx.recv().await.expect("handler ran");
    assert_eq!(
        handler_attempt,
        first_attempt + 1,
        "redelivered ctx.attempt must be the incremented server attempt"
    );

    shutdown.shutdown();
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");
}

/// Shutdown drains: the in-flight handler finishes and its response is
/// submitted, run() returns, and a command created after shutdown is never
/// leased.
#[tokio::test]
async fn receiver_shutdown_drains_in_flight_and_stops_leasing() {
    let (server, env) = pull_server_and_env().await;

    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL);
    receiver.handle("drain-me", move |_ctx| {
        let started_tx = started_tx.clone();
        async move {
            started_tx.send(()).expect("test channel open");
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok(serde_json::json!({ "drained": true }))
        }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let request = test_inline_create_command(DEPLOYMENT_ID, "drain-me");
    let in_flight = server.create_command(request).await.expect("create");

    // Wait until the handler is actually running, then shut down mid-flight.
    started_rx.recv().await.expect("handler started");
    shutdown.shutdown();

    // A command created after shutdown must never be leased.
    let late_request = test_inline_create_command(DEPLOYMENT_ID, "drain-me");
    let late = server.create_command(late_request).await.expect("create");

    // run() must wait for the in-flight command before returning.
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");

    let drained_status = server
        .get_command_status(&in_flight.command_id)
        .await
        .expect("status");
    assert_eq!(
        drained_status.state,
        CommandState::Succeeded,
        "in-flight command must finish (and submit) during drain"
    );

    // Give a stopped receiver several poll intervals worth of chances to
    // (incorrectly) lease the late command.
    tokio::time::sleep(POLL_INTERVAL * 4).await;
    let late_status = server
        .get_command_status(&late.command_id)
        .await
        .expect("status");
    assert_eq!(
        late_status.state,
        CommandState::Pending,
        "no new leases once draining/stopped"
    );
}

/// A response larger than max_inline_bytes goes through the envelope's real
/// presigned storage upload flow and round-trips.
#[tokio::test]
async fn receiver_round_trips_large_response_via_storage() {
    let (server, env) = pull_server_and_env().await;

    // Comfortably above the 150KB inline limit once JSON-encoded.
    let large_payload = "x".repeat(200_000);
    let expected_len = large_payload.len();
    let mut receiver = Receiver::from_env_vars(&env)
        .expect("receiver should build from env")
        .with_poll_interval(POLL_INTERVAL);
    receiver.handle("big-report", move |_ctx| {
        let large_payload = large_payload.clone();
        async move { Ok(serde_json::json!({ "report": large_payload })) }
    });
    let shutdown = receiver.shutdown_handle();
    let run = tokio::spawn(async move { receiver.run().await });

    let request = test_inline_create_command(DEPLOYMENT_ID, "big-report");
    let created = server.create_command(request).await.expect("create");

    let final_status = server
        .wait_for_completion(&created.command_id, Duration::from_secs(10))
        .await
        .expect("command should complete");
    assert_eq!(final_status.state, CommandState::Succeeded);

    let response = final_status.response.expect("response present");
    let CommandResponse::Success { response: body } = response else {
        panic!("expected success response");
    };
    let BodySpec::Storage {
        storage_get_request,
        ..
    } = &body
    else {
        panic!("large response must be storage mode, got inline");
    };
    let get_request = storage_get_request
        .as_ref()
        .expect("storage response must carry a get request");
    let downloaded = get_request
        .execute(None)
        .await
        .expect("download stored response");
    assert_eq!(downloaded.status_code, 200);
    let bytes = downloaded.body.expect("stored response body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON response");
    assert_eq!(
        json["report"].as_str().expect("report string").len(),
        expected_len
    );

    shutdown.shutdown();
    run.await
        .expect("run task join")
        .expect("run returns Ok on shutdown");
}

// ---------------------------------------------------------------------------
// Observability: the `process_lease` "Command processed" event field contract.
//
// These drive `process_lease` directly (bypassing the server) so the pinned
// event fields — command id, lease id, target resource id/type wire value,
// attempt, deadline rendering, handler/submit status — can be asserted exactly
// against captured tracing output. The TypeScript twin (`processLease`) logs
// the same field set.
// ---------------------------------------------------------------------------

/// In-memory `MakeWriter` capturing formatted tracing output. One definition
/// shared by both observability tests (was duplicated per test in-module).
#[derive(Clone)]
struct BufWriter(Arc<Mutex<Vec<u8>>>);

impl Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BufWriter {
    type Writer = BufWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn observability_envelope(command: &str, deadline: Option<DateTime<Utc>>) -> Envelope {
    Envelope {
        protocol: PROTOCOL_VERSION.to_string(),
        deployment_id: "dep-123".to_string(),
        target: CommandTarget::new("agent", CommandTargetType::Daemon),
        command_id: "cmd_1".to_string(),
        attempt: 1,
        deadline,
        command: command.to_string(),
        params: BodySpec::inline(br#"{"key":"value"}"#),
        response_handling: ResponseHandling {
            max_inline_bytes: INLINE_MAX_BYTES as u64,
            submit_response_url: "https://commands.example.com/v1/commands/cmd_1/response"
                .to_string(),
            storage_upload_request: PresignedRequest::new_http(
                "https://storage.example.com/upload".to_string(),
                "PUT".to_string(),
                HashMap::new(),
                PresignedOperation::Put,
                "test-path".to_string(),
                Utc::now() + ChronoDuration::hours(1),
            ),
        },
    }
}

#[tokio::test]
async fn process_lease_emits_command_processed_with_pinned_fields() {
    // Submit target is an unroutable local port: the submit fails fast
    // (connection refused, no DNS, offline) so `submit_status` is "failed"
    // while the completion event still fires with every pinned field.
    let deadline = Utc::now() + ChronoDuration::seconds(30);
    let mut envelope = observability_envelope("echo", Some(deadline));
    envelope.response_handling.submit_response_url =
        "http://127.0.0.1:1/v1/commands/cmd_1/response".to_string();
    let lease = LeaseInfo {
        lease_id: "lease_obs".to_string(),
        lease_expires_at: Utc::now() + ChronoDuration::seconds(60),
        command_id: envelope.command_id.clone(),
        attempt: 2,
        envelope,
    };
    let handler = box_handler(|_ctx: Context| async move { Ok(serde_json::json!({ "ok": true })) });
    let target = CommandTarget::new("agent", CommandTargetType::Daemon);

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufWriter(buf.clone()))
        .with_ansi(false)
        .finish();
    {
        let _guard = tracing::subscriber::set_default(subscriber);
        process_lease(Some(handler), lease, target).await;
    }

    let logs = String::from_utf8(buf.lock().expect("lock").clone()).expect("utf8");
    assert!(logs.contains("Command processed"), "logs: {logs}");
    assert!(logs.contains("command_id"), "command id field: {logs}");
    assert!(logs.contains("lease_obs"), "lease id value: {logs}");
    assert!(
        logs.contains("target_resource_id"),
        "target id field: {logs}"
    );
    // Wire value (lowercase, TS twin's `"daemon"`), not Rust Debug's `Daemon`.
    assert!(
        logs.contains("target_resource_type=daemon"),
        "target type must render the wire value, not Debug: {logs}"
    );
    assert!(
        !logs.contains("target_resource_type=Daemon"),
        "target type must not render Debug's capitalized variant: {logs}"
    );
    assert!(logs.contains("attempt=2"), "attempt value: {logs}");
    // ISO-8601 string (TS twin's bare `deadline.toISOString()`-equivalent),
    // not Debug's `Some(2026-...)`.
    let expected_deadline = format!("deadline=\"{}\"", deadline.to_rfc3339());
    assert!(
        logs.contains(&expected_deadline),
        "deadline must render as a bare ISO-8601 string, got: {logs}"
    );
    assert!(
        !logs.contains("deadline=Some"),
        "deadline must not render Debug's Some(..) wrapper: {logs}"
    );
    assert!(
        logs.contains("handler_status=success"),
        "handler status: {logs}"
    );
    assert!(
        logs.contains("submit_status=failed"),
        "submit status: {logs}"
    );
}

#[tokio::test]
async fn process_lease_emits_command_processed_for_handler_error() {
    // Submit target is an unroutable local port (same trick as the
    // success-path test above): the submit fails fast so this test stays
    // deterministic and network-free.
    //
    // No deadline on the envelope: the completion event's `deadline` field
    // must be entirely absent (tracing's `Option<T>: Value` skips `None`),
    // matching the TS twin's bare-string-or-`null` semantics for the
    // no-deadline case.
    let mut envelope = observability_envelope("burn", None);
    envelope.target = CommandTarget::new("worker-1", CommandTargetType::Container);
    envelope.response_handling.submit_response_url =
        "http://127.0.0.1:1/v1/commands/cmd_1/response".to_string();
    let lease = LeaseInfo {
        lease_id: "lease_err".to_string(),
        lease_expires_at: Utc::now() + ChronoDuration::seconds(60),
        command_id: envelope.command_id.clone(),
        attempt: 1,
        envelope,
    };
    let handler = box_handler(|_ctx: Context| async move {
        Err::<serde_json::Value, HandlerError>("database on fire".into())
    });
    let target = CommandTarget::new("worker-1", CommandTargetType::Container);

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufWriter(buf.clone()))
        .with_ansi(false)
        .finish();
    {
        let _guard = tracing::subscriber::set_default(subscriber);
        process_lease(Some(handler), lease, target).await;
    }

    let logs = String::from_utf8(buf.lock().expect("lock").clone()).expect("utf8");
    assert!(logs.contains("Command processed"), "logs: {logs}");
    assert!(
        logs.contains("target_resource_type=container"),
        "target type must render the wire value: {logs}"
    );
    assert!(
        logs.contains(&format!("handler_status={ERROR_CODE_HANDLER_ERROR}")),
        "handler status must be the non-success error code: {logs}"
    );
    assert!(
        !logs.contains("handler_status=success"),
        "handler status must not be success: {logs}"
    );
    assert!(
        logs.contains("submit_status=failed"),
        "submit status: {logs}"
    );
    assert!(
        !logs.contains("deadline="),
        "absent deadline must not appear as a field at all: {logs}"
    );
}

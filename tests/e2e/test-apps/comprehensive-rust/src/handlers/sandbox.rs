use std::collections::BTreeMap;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use futures_util::StreamExt;
use tracing::info;

use crate::{
    models::{AppState, KvTestResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context};
use alien_sdk::traits::{
    CommandOutput, CreateSessionRequest, RunCommandRequest, Sandbox, SandboxSessionState,
};

/// Test a sandbox binding by running a command and moving a file through a session.
#[utoipa::path(
    post,
    path = "/sandbox-test/{binding_name}",
    tag = "sandbox",
    params(
        ("binding_name" = String, Path, description = "Name of the sandbox binding to test")
    ),
    responses(
        (status = 200, description = "Sandbox test completed", body = KvTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Sandbox operation failed", body = AlienError),
    ),
    operation_id = "test_sandbox",
    summary = "Test sandbox session operations",
    description = "Creates a session, runs a command, writes and reads a file, then terminates"
)]
pub async fn test_sandbox(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<KvTestResponse>> {
    info!(%binding_name, "Received sandbox test request");

    let sandbox = app_state
        .ctx
        .bindings()
        .sandbox(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // Awaited rather than raced: only the id create returns can end a session — AWS and Azure
    // allocate their own rather than taking the one asked for — so a bounded wait here would
    // terminate an id no session answers to while the real one is still being created.
    let session_id = format!("e2e-{}", Utc::now().timestamp_millis());
    let created = sandbox
        .create(CreateSessionRequest {
            session_id: Some(session_id.clone()),
            tenant_key: None,
            env: BTreeMap::new(),
        })
        .await;
    let session = match created {
        Ok(session) => session,
        Err(error) => {
            // The create has settled, so ending the id asked for is not a race: local, Kubernetes
            // and GCP take that id, so a session provisioned under a lost response answers to it.
            // AWS and Azure allocate their own; there the 600s maxLifetimeSeconds reclaims it.
            terminate_and_confirm(sandbox.as_ref(), &session_id).await?;
            return Err(error).context(ErrorData::SandboxOperationFailed {
                operation: "create".to_string(),
            });
        }
    };

    // Everything after create runs in a helper so a failure still reaches terminate below. A
    // session left running is a session still billing. The exercise is bounded as a whole so a
    // command or file operation that never answers still lets terminate run.
    let outcome = match tokio::time::timeout(
        EXERCISE_TIMEOUT,
        exercise(sandbox.as_ref(), &session.session_id),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!(
                "the sandbox exercise gave no answer in {}s",
                EXERCISE_TIMEOUT.as_secs()
            ),
        })),
    };
    let terminated = terminate_and_confirm(sandbox.as_ref(), &session.session_id).await;

    // A leaked session is reported first even when the exercise also failed: an exercise failure
    // is a broken test, a surviving session is a billable sandbox nobody will look for.
    terminated?;
    outcome?;

    info!(%binding_name, "Sandbox test completed successfully");

    Ok(Json(KvTestResponse {
        binding_name,
        success: true,
    }))
}

/// How long the whole exercise — commands and files — may take before terminate runs anyway.
const EXERCISE_TIMEOUT: Duration = Duration::from_secs(180);
/// How many times a refused terminate is retried before the session is called leaked.
const TERMINATE_ATTEMPTS: u32 = 5;
/// How long to wait for an accepted terminate to converge before calling the session leaked.
const TERMINATE_POLL_ATTEMPTS: u32 = 15;
const TERMINATE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How long one status read may take before it counts as a failed read: a stalled manager must
/// cost one attempt, not the whole test.
const STATUS_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How long one terminate call may take before it counts as refused. Longer than a read: a
/// backend may confirm deletion inside the call, and that is bounded on its side too.
const TERMINATE_CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Terminates the session and reads it back to confirm it is gone.
///
/// A successful terminate is not the same claim: the backends return once deletion is accepted,
/// so a test that stops at the return value passes while the session keeps running.
async fn terminate_and_confirm(sandbox: &dyn Sandbox, session_id: &str) -> Result<()> {
    // Polled rather than read once: every backend returns from terminate as soon as the deletion
    // is accepted, so a single read races normal convergence and would fail a teardown that was
    // simply still finishing.
    // The terminate itself is retried, because it is idempotent and a transient refusal is the
    // one failure that leaves a session running: giving up on it would hand back an error and a
    // billable sandbox nobody will look for.
    // A failed read inside the window is retried like a non-terminal state: the session may well
    // be gone, and giving up on the first blip would fail a teardown that had already converged.
    // A read that never succeeds still fails, carrying the last error rather than a bare timeout.
    // The two budgets are separate so a terminate accepted on its last try still gets the whole
    // convergence window rather than one immediate read.
    // Carried as the reason text: an answer that never came and an answer that refused are the
    // same thing to the test — a session it could not end.
    let mut refused: Option<String> = None;
    for attempt in 0..TERMINATE_ATTEMPTS {
        match tokio::time::timeout(TERMINATE_CALL_TIMEOUT, sandbox.terminate(session_id)).await {
            Ok(Ok(())) => {
                refused = None;
                break;
            }
            Ok(Err(error)) => refused = Some(error.to_string()),
            Err(_) => {
                refused = Some(format!(
                    "no answer in {}s",
                    TERMINATE_CALL_TIMEOUT.as_secs()
                ))
            }
        }
        if attempt + 1 < TERMINATE_ATTEMPTS {
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }
    }
    // A refused terminate is not yet a leak: the request may have succeeded with its response
    // lost, so the confirmation poll below decides, and the refusal is what is reported if the
    // session turns out to still be there.
    let mut last = match &refused {
        Some(reason) => format!("running (terminate refused: {reason})"),
        None => String::from("unread"),
    };
    for attempt in 0..TERMINATE_POLL_ATTEMPTS {
        match tokio::time::timeout(STATUS_READ_TIMEOUT, sandbox.get(session_id)).await {
            Err(_) => {
                last = format!(
                    "unreadable (no answer in {}s)",
                    STATUS_READ_TIMEOUT.as_secs()
                )
            }
            Ok(Ok(None)) => return Ok(()),
            Ok(Ok(Some(session))) if session.state == SandboxSessionState::Terminated => {
                return Ok(())
            }
            Ok(Ok(Some(session))) => last = format!("{:?}", session.state),
            Ok(Err(error)) => last = format!("unreadable ({error})"),
        }

        if attempt + 1 < TERMINATE_POLL_ATTEMPTS {
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }
    }

    if let Some(reason) = refused {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!(
                "session '{session_id}' could not be terminated ({reason}); it may still be billing"
            ),
        }));
    }
    Err(AlienError::new(ErrorData::TestValidationFailed {
        reason: format!(
            "session '{session_id}' is still {last} {}s after terminate; it may still be billing",
            TERMINATE_POLL_ATTEMPTS * TERMINATE_POLL_INTERVAL.as_secs() as u32
        ),
    }))
}

/// The part of the test that can fail without leaking a session.
async fn exercise(sandbox: &dyn Sandbox, session_id: &str) -> Result<()> {
    let marker = format!("alien-sandbox-e2e-{}", Utc::now().timestamp_millis());

    let mut frames = sandbox
        .run_command(
            session_id,
            RunCommandRequest {
                command: vec!["/bin/echo".to_string(), marker.clone()],
                working_directory: None,
                env: BTreeMap::new(),
                deadline: Duration::from_secs(30),
            },
        )
        .await
        .context(ErrorData::SandboxOperationFailed {
            operation: "run_command".to_string(),
        })?;

    let mut stdout = Vec::new();
    let mut exit_code = None;
    while let Some(frame) = frames.next().await {
        match frame.context(ErrorData::SandboxOperationFailed {
            operation: "run_command stream".to_string(),
        })? {
            CommandOutput::Stdout { data, .. } => stdout.extend_from_slice(&data),
            CommandOutput::Exit { code, .. } => exit_code = Some(code),
            CommandOutput::Stderr { .. } => {}
        }
    }

    if exit_code != Some(0) {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!("run_command exited with {exit_code:?}, expected 0"),
        }));
    }

    let printed = String::from_utf8_lossy(&stdout);
    if !printed.contains(&marker) {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!("stdout did not carry the marker, got '{printed}'"),
        }));
    }

    // Files both directions through the same session, which is what makes it a session rather
    // than a sequence of unrelated commands.
    sandbox
        .write_files(
            session_id,
            BTreeMap::from([("e2e/input.txt".to_string(), marker.as_bytes().to_vec())]),
        )
        .await
        .context(ErrorData::SandboxOperationFailed {
            operation: "write_files".to_string(),
        })?;

    let read_back = sandbox
        .read_file(session_id, "e2e/input.txt")
        .await
        .context(ErrorData::SandboxOperationFailed {
            operation: "read_file".to_string(),
        })?;

    if read_back != marker.as_bytes() {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: "read_file returned different bytes than write_files sent".to_string(),
        }));
    }

    Ok(())
}

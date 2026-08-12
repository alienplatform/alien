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

    let session = sandbox
        .create(CreateSessionRequest {
            session_id: Some(format!("e2e-{}", Utc::now().timestamp_millis())),
            tenant_key: None,
            env: BTreeMap::new(),
        })
        .await
        .context(ErrorData::SandboxOperationFailed {
            operation: "create".to_string(),
        })?;

    // Everything after create runs in a helper so a failure still reaches terminate below. A
    // session left running is a session still billing.
    let outcome = exercise(sandbox.as_ref(), &session.session_id).await;
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

/// How long to wait for a terminate to converge before calling the session leaked.
const TERMINATE_POLL_ATTEMPTS: u32 = 15;
const TERMINATE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Terminates the session and reads it back to confirm it is gone.
///
/// A successful terminate is not the same claim: the backends return once deletion is accepted,
/// so a test that stops at the return value passes while the session keeps running.
async fn terminate_and_confirm(sandbox: &dyn Sandbox, session_id: &str) -> Result<()> {
    sandbox
        .terminate(session_id)
        .await
        .context(ErrorData::SandboxOperationFailed {
            operation: "terminate".to_string(),
        })?;

    // Polled rather than read once: every backend returns from terminate as soon as the deletion
    // is accepted, so a single read races normal convergence and would fail a teardown that was
    // simply still finishing.
    // A failed read inside the window is retried like a non-terminal state: the session may well
    // be gone, and giving up on the first blip would fail a teardown that had already converged.
    // A read that never succeeds still fails, carrying the last error rather than a bare timeout.
    let mut last = String::from("unread");
    for attempt in 0..TERMINATE_POLL_ATTEMPTS {
        match sandbox.get(session_id).await {
            Ok(None) => return Ok(()),
            Ok(Some(session)) if session.state == SandboxSessionState::Terminated => return Ok(()),
            Ok(Some(session)) => last = format!("{:?}", session.state),
            Err(error) => last = format!("unreadable ({error})"),
        }

        if attempt + 1 < TERMINATE_POLL_ATTEMPTS {
            tokio::time::sleep(TERMINATE_POLL_INTERVAL).await;
        }
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

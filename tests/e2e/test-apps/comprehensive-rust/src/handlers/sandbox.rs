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
    models::{AppState, KvTestResponse, SandboxSessionsResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context};
use alien_sdk::traits::{CommandOutput, CreateSessionRequest, RunCommandRequest, Sandbox};

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

    // No timeouts here: the runner bounds the whole request, and a step that hangs is a failed
    // check either way. A bound inside the handler would only add a path where the handler
    // abandons a session it then has to hunt for — and the runtime drops this future when the
    // runner gives up, so cleanup cannot depend on it surviving. The sandbox's own delete flow
    // reaps every session, and the runner asserts none survived; this handler only has to leave
    // nothing behind when it is the one still running.
    let session_id = format!("e2e-rs-{}", Utc::now().timestamp_millis());
    let session = match sandbox
        .create(CreateSessionRequest {
            session_id: Some(session_id.clone()),
            tenant_key: None,
            env: BTreeMap::new(),
        })
        .await
    {
        Ok(session) => session,
        Err(error) => {
            // A create that provisioned and then lost its answer exists under the id asked for on
            // every backend that honours one, so ending that id here is what stops it; where the
            // backend allocates its own id there is nothing to name, and its teardown reaps it.
            let _ = sandbox.terminate(&session_id).await;
            return Err(error).context(ErrorData::SandboxOperationFailed {
                operation: "create".to_string(),
            });
        }
    };

    let outcome = exercise(sandbox.as_ref(), &session.session_id).await;

    // Unconditional: a failed exercise and a healthy one tear down the same way, and terminate is
    // idempotent on every backend. Its own failure is reported only when it is the sole failure —
    // the fault the exercise found is what a reader of this check needs, and a session that also
    // would not close is second to it.
    let terminated =
        sandbox
            .terminate(&session.session_id)
            .await
            .context(ErrorData::SandboxOperationFailed {
                operation: "terminate".to_string(),
            });
    outcome?;
    terminated?;

    info!(%binding_name, "Sandbox test completed successfully");

    Ok(Json(KvTestResponse {
        binding_name,
        success: true,
    }))
}

/// What the check leaves behind, read from the backend rather than trusted from the handler.
#[utoipa::path(
    get,
    path = "/sandbox-sessions/{binding_name}",
    tag = "sandbox",
    params(
        ("binding_name" = String, Path, description = "Name of the sandbox binding to inspect")
    ),
    responses(
        (status = 200, description = "Sessions still present", body = SandboxSessionsResponse),
        (status = 400, description = "Binding not found", body = AlienError),
    ),
    operation_id = "sandbox_sessions",
    summary = "List the sandbox's surviving sessions",
    description = "Reads the backend's own view of which sessions still exist"
)]
pub async fn sandbox_sessions(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<SandboxSessionsResponse>> {
    let sandbox = app_state
        .ctx
        .bindings()
        .sandbox(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // A session the handler abandoned would not show up in its own answer, so this asks the
    // backend. Where the backend cannot enumerate, that is reported as such rather than as zero.
    match sandbox.list().await {
        Ok(sessions) => Ok(Json(SandboxSessionsResponse {
            enumerable: true,
            session_ids: sessions.into_iter().map(|s| s.session_id).collect(),
        })),
        Err(error) if error.code == "OPERATION_NOT_SUPPORTED" => {
            Ok(Json(SandboxSessionsResponse {
                enumerable: false,
                session_ids: Vec::new(),
            }))
        }
        Err(error) => Err(error).context(ErrorData::SandboxOperationFailed {
            operation: "list".to_string(),
        }),
    }
}

/// Runs a command to completion and returns its stdout, stderr and exit code.
async fn run_to_completion(
    sandbox: &dyn Sandbox,
    session_id: &str,
    command: Vec<String>,
) -> Result<(Vec<u8>, Vec<u8>, Option<i32>)> {
    let mut frames = sandbox
        .run_command(
            session_id,
            RunCommandRequest {
                command,
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
    let mut stderr = Vec::new();
    let mut exit_code = None;
    while let Some(frame) = frames.next().await {
        match frame.context(ErrorData::SandboxOperationFailed {
            operation: "run_command stream".to_string(),
        })? {
            CommandOutput::Stdout { data, .. } => stdout.extend_from_slice(&data),
            CommandOutput::Exit { code, .. } => exit_code = Some(code),
            CommandOutput::Stderr { data, .. } => stderr.extend_from_slice(&data),
        }
    }

    Ok((stdout, stderr, exit_code))
}

/// The part of the test that can fail without leaking a session.
async fn exercise(sandbox: &dyn Sandbox, session_id: &str) -> Result<()> {
    let marker = format!("alien-sandbox-e2e-{}", Utc::now().timestamp_millis());

    let (stdout, stderr, exit_code) = run_to_completion(
        sandbox,
        session_id,
        vec!["/bin/echo".to_string(), marker.clone()],
    )
    .await?;

    if exit_code != Some(0) {
        // stderr is kept, not reduced to a boolean: when this fails it is the only thing that
        // says why, and this harness diagnoses a deployment from the outside.
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!(
                "run_command exited with {exit_code:?}, expected 0: {}",
                String::from_utf8_lossy(&stderr)
            ),
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

    // Read the same file from inside the session, not only back through the agent. `read_file` is
    // the agent reading what the agent wrote, so it holds even when the command the upload exists
    // for cannot open it — which is the difference between the backends that run an agent as a
    // different user than the command and the ones that do not.
    let (inside, inside_stderr, inside_exit) = run_to_completion(
        sandbox,
        session_id,
        vec!["/bin/cat".to_string(), "e2e/input.txt".to_string()],
    )
    .await?;

    if inside_exit != Some(0) {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!(
                "the session could not read the file written into it, exit {inside_exit:?}: {}",
                String::from_utf8_lossy(&inside_stderr)
            ),
        }));
    }

    if !String::from_utf8_lossy(&inside).contains(&marker) {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: format!(
                "the session read different bytes than write_files sent: '{}'",
                String::from_utf8_lossy(&inside)
            ),
        }));
    }

    Ok(())
}

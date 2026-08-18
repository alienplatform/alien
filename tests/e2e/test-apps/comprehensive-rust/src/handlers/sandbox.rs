use std::collections::BTreeMap;
use std::time::{Duration, Instant};

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

    // Chosen here so teardown can name a session even when create never reports one. Local honours
    // a requested id, and Local is the only platform this check runs on.
    let session_id = format!("e2e-rs-{}", Utc::now().timestamp_millis());
    let exercise_by = Instant::now() + REQUEST_BUDGET - TEARDOWN_RESERVE;
    let left = || exercise_by.saturating_duration_since(Instant::now());

    let created = tokio::time::timeout(
        left(),
        sandbox.create(CreateSessionRequest {
            session_id: Some(session_id.clone()),
            tenant_key: None,
            env: BTreeMap::new(),
        }),
    )
    .await;
    let (session, mut outcome, create_settled) = match created {
        Ok(Ok(session)) => (Some(session), Ok(()), true),
        Ok(Err(error)) => (
            None,
            Err(error).context(ErrorData::SandboxOperationFailed {
                operation: "create".to_string(),
            }),
            true,
        ),
        Err(_) => (
            None,
            Err(AlienError::new(ErrorData::TestValidationFailed {
                reason: "create gave no answer within the request budget; a session it lands \
                         later is reaped when the sandbox is deleted"
                    .to_string(),
            })),
            false,
        ),
    };

    if let Some(session) = &session {
        outcome =
            match tokio::time::timeout(left(), exercise(sandbox.as_ref(), &session.session_id))
                .await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(AlienError::new(ErrorData::TestValidationFailed {
                    reason: "the sandbox exercise gave no answer within the request budget"
                        .to_string(),
                })),
            };
    }

    let target = session
        .as_ref()
        .map_or(session_id.as_str(), |session| session.session_id.as_str());
    if let Err(leaked) = converge(sandbox.as_ref(), target, create_settled).await {
        // Both are reported: a session left running is a sandbox nobody will look for, and why
        // the exercise failed is often why it is still there.
        return Err(match outcome {
            Err(failure) => AlienError::new(ErrorData::TestValidationFailed {
                reason: format!("{leaked}; {failure}"),
            }),
            Ok(()) => leaked,
        });
    }
    outcome?;

    info!(%binding_name, "Sandbox test completed successfully");

    Ok(Json(KvTestResponse {
        binding_name,
        success: true,
    }))
}

/// How long the whole check may take, teardown included. Held under the 300s read timeout the
/// worker runtime applies to a proxied response (`alien-worker-runtime/src/transports/shared.rs`):
/// this handler answers only at the end, so a longer run reaches the caller as a bare proxy error.
const REQUEST_BUDGET: Duration = Duration::from_secs(150);
/// Held back from the budget so teardown still runs after a slow create or exercise. Local frees
/// a session only when the sandbox itself is deleted, so skipping teardown strands it.
const TEARDOWN_RESERVE: Duration = Duration::from_secs(45);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Terminates the session and reads it back until it is gone.
///
/// Terminate is reissued on every pass: it is idempotent, a transient refusal is the one failure
/// that leaves a session running, and a session visible only on a later pass is ended rather than
/// reported as a leak.
///
/// `create_settled` is false when create never answered. The session can still be created after
/// an empty read there, so absence decides nothing and the wait runs its full reserve, ending
/// whatever appears. A read that fails counts as present: an unreadable session is not a gone one.
async fn converge(sandbox: &dyn Sandbox, session_id: &str, create_settled: bool) -> Result<()> {
    let deadline = Instant::now() + TEARDOWN_RESERVE;
    let left = || deadline.saturating_duration_since(Instant::now());

    loop {
        let refused = match tokio::time::timeout(left(), sandbox.terminate(session_id)).await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.to_string()),
            Err(_) => Some("no answer".to_string()),
        };

        let present = match tokio::time::timeout(left(), sandbox.get(session_id)).await {
            Ok(Ok(None)) => None,
            Ok(Ok(Some(session))) if session.state == SandboxSessionState::Terminated => None,
            Ok(Ok(Some(session))) => Some(format!("{:?}", session.state)),
            Ok(Err(error)) => Some(format!("unreadable ({error})")),
            Err(_) => Some("unreadable (no answer)".to_string()),
        };

        if present.is_none() && create_settled {
            return Ok(());
        }

        if Instant::now() >= deadline {
            // Nothing left to end. A create still in flight can land one after this, which is why
            // the caller reports its own failure and the sandbox's deletion reaps what remains.
            let Some(seen) = present else { return Ok(()) };
            let why = match refused {
                Some(refusal) => format!("{seen}, terminate refused: {refusal}"),
                None => seen,
            };
            return Err(AlienError::new(ErrorData::TestValidationFailed {
                reason: format!(
                    "session '{session_id}' is still {why} {}s after terminate",
                    TEARDOWN_RESERVE.as_secs()
                ),
            }));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
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

    Ok(())
}

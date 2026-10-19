//! The checks behind the image build's validate hook.
//!
//! AWS calls the hook on a MicroVM run from the finished snapshot, so this is the one moment a
//! broken image can fail the build instead of the first session. Each check goes through the code
//! path a real request takes, never a shortcut beside it: the spawn that `/v1/exec` uses, and the
//! confined open that `/v1/files` uses. A pass here means those paths work on this image.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use uuid::Uuid;

use crate::error::{ErrorData, Result};
use crate::exec::{self, ExecRequest, Frame};
use crate::files;
use crate::server::AgentState;
use alien_error::{AlienError, Context, IntoAlienError};

/// First argument that makes the agent binary print its effective `uid gid` and exit.
///
/// The agent is the one program every image is known to carry, root-owned and executable by the
/// exec identity, so it is what the identity check runs. A customer's base image may have no
/// shell and no `id`.
pub const IDENTITY_ARGUMENT: &str = "identity";

/// Ceiling on the identity probe. One fork and exec of a binary already in the page cache;
/// well under the hook's own timeout so a hang is reported as this check, not as the hook's.
const PROBE_TIMEOUT_MS: u64 = 10_000;

/// Runs every check, stopping at the first failure.
pub async fn run(state: &AgentState) -> Result<()> {
    identity(state).await?;
    file(state).await
}

/// Runs the agent binary as the exec identity, through the same spawn path as `/v1/exec`, and
/// requires it to report exactly the configured uid and gid.
///
/// Equality is the whole check. Under uid-split the configured identity is already refused at
/// startup when it is the agent's own, so a match here is also proof the drop happened.
async fn identity(state: &AgentState) -> Result<()> {
    let request = ExecRequest {
        command: vec![
            state.agent_binary.to_string_lossy().into_owned(),
            IDENTITY_ARGUMENT.to_string(),
        ],
        timeout_ms: PROBE_TIMEOUT_MS,
        cwd: None,
        env: BTreeMap::new(),
    };
    let frames = exec::run(
        &request,
        Some(&state.session_root),
        state.exec_identity,
        state.output_cap,
    )
    .await;

    let mut stdout = Vec::new();
    for frame in &frames {
        match frame {
            Frame::Stdout { data, .. } => {
                let bytes = BASE64.decode(data).into_alien_error().context(
                    ErrorData::ValidationFailed {
                        check: "identity".to_string(),
                        reason: "the probe's output was not valid base64".to_string(),
                    },
                )?;
                stdout.extend(bytes);
            }
            Frame::Stderr { .. } | Frame::Exit { code: 0, .. } => {}
            Frame::Exit { code, .. } => {
                return Err(failed(
                    "identity",
                    format!("the probe exited with code {code}"),
                ))
            }
            Frame::Error { code, message } => {
                return Err(failed("identity", format!("{code}: {message}")))
            }
        }
    }

    let reported = String::from_utf8_lossy(&stdout);
    let mut ids = reported.split_whitespace().map(str::parse::<u32>);
    let (Some(Ok(uid)), Some(Ok(gid))) = (ids.next(), ids.next()) else {
        return Err(failed(
            "identity",
            format!("the probe reported '{}', not 'uid gid'", reported.trim()),
        ));
    };

    let expected = state.exec_identity;
    if uid != expected.uid || gid != expected.gid {
        return Err(failed(
            "identity",
            format!(
                "the command ran as {uid}:{gid}, not the configured {}:{}",
                expected.uid, expected.gid
            ),
        ));
    }
    Ok(())
}

/// Writes a file into the session root through the confined path, reads it back, compares the
/// bytes, and removes it.
///
/// The removal runs whether or not the read agreed, so a failing check leaves nothing behind
/// either. The first error wins.
async fn file(state: &AgentState) -> Result<()> {
    let root = &state.session_root;
    // A fresh name each time: the hook is unauthenticated and the session may already hold
    // files, so a fixed name could overwrite one of them.
    let path = format!("/.validate-{}", Uuid::new_v4().simple());
    let payload: Vec<u8> = (0u8..=255).collect();

    let written = files::write(root, &path, &payload)
        .await
        .context(step("writing the probe file"));
    if written.is_err() {
        // The open can create the file before the write fails, so it is removed either way. The
        // write's error is the one reported.
        let removed = files::remove(root, &path)
            .await
            .context(step("removing the probe file"));
        return written.and(removed);
    }

    let read_back = match files::read(root, &path).await {
        Ok(contents) if contents == payload => Ok(()),
        Ok(_) => Err(failed(
            "file",
            "the bytes read back differ from the bytes written".to_string(),
        )),
        Err(error) => Err(error).context(step("reading the probe file back")),
    };
    let removed = files::remove(root, &path)
        .await
        .context(step("removing the probe file"));

    read_back.and(removed)
}

fn failed(check: &str, reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationFailed {
        check: check.to_string(),
        reason,
    })
}

fn step(reason: &str) -> ErrorData {
    ErrorData::ValidationFailed {
        check: "file".to_string(),
        reason: reason.to_string(),
    }
}

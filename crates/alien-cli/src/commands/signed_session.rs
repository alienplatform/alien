//! The engineer's side of a signed exec session.
//!
//! A protected machine opens a session only after the engineer's device signs
//! a `session/v1` statement naming the machine's fresh challenge. The CLI
//! verifies the project's signer list from the pinned fingerprint, prints
//! what it is about to sign, signs it, and runs the Noise handshake. From then
//! on every frame is encrypted, so the control plane relaying the session can
//! neither read nor inject anything.

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::kinds::{SessionBody, SessionTarget, MAX_SESSION_SECONDS};
use alien_signing::session::{Challenge, Channel, Handshake, SessionKeys, Side};
use alien_signing::{Kind, Role};
use futures::StreamExt;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::signing::{self, api::SigningApi, SignerArgs, SigningHome};

/// What the CLI needs to sign a session when the machine asks for one.
#[derive(Debug, Clone)]
pub struct SessionSigning {
    /// Where to verify the signer list and the project.
    pub ctx: ExecutionMode,
    /// The deployment.
    pub deployment_id: String,
    /// Machine the session targets.
    pub machine: Option<String>,
    /// The command, or `None` for a shell.
    pub argv: Option<Vec<String>>,
    /// Whether a terminal is allocated.
    pub tty: bool,
    /// Why, for the customer's transcript.
    pub reason: String,
    /// Which key signs.
    pub signer: SignerArgs,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum MachineHello {
    Challenge { challenge: Challenge },
}

/// Whether `text` is a machine's challenge.
pub fn challenge(text: &str) -> Option<Challenge> {
    match serde_json::from_str::<MachineHello>(text) {
        Ok(MachineHello::Challenge { challenge }) => Some(challenge),
        Err(_) => None,
    }
}

/// Sign the session for `challenge` and run the handshake on the socket.
pub async fn handshake<St>(
    signing: &SessionSigning,
    challenge: Challenge,
    send: &tokio::sync::mpsc::Sender<Message>,
    stream: &mut St,
) -> Result<Channel>
where
    St: futures::Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    let machine = signing.machine.clone().ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: "This machine needs a signed session; pass --machine".to_string(),
        })
    })?;
    let (project, _) = signing.ctx.resolve_project(None, false).await?;
    let home = SigningHome::current()?;
    let api = SigningApi::from_context(&signing.ctx).await?;
    let chain =
        signing::verified_chain(&api, &home, &project, signing.signer.fingerprint.as_deref())
            .await?;
    let signer = signing.signer.open(&home).await?;
    signing::check_signer(&chain, &signer, Role::Operate)?;

    let keys = SessionKeys::generate().context(ErrorData::SigningFailed {
        message: "Could not create the session key".to_string(),
    })?;
    let body = SessionBody {
        session_id: challenge.session_id.clone(),
        target: SessionTarget {
            machine: Some(machine.clone()),
            workload: None,
            tool: None,
            scope: None,
        },
        argv: signing.argv.clone(),
        tty: signing.tty,
        max_duration_seconds: MAX_SESSION_SECONDS,
        requester: signer.id().to_string(),
        reason: signing.reason.clone(),
        verifier_nonce: challenge.nonce.clone(),
        verifier_key: challenge.verifier_key.clone(),
        client_key: keys.public_key(),
    };
    eprintln!(
        "Signing session {} on machine {machine} as '{}': {}",
        body.session_id,
        signer.id(),
        body.argv
            .as_ref()
            .map(|argv| argv.join(" "))
            .unwrap_or_else(|| "a shell".to_string())
    );
    let envelope = signing::envelope(
        Kind::Session,
        &project,
        Some(&signing.deployment_id),
        &chain,
        body,
    );
    let statement = signing::sign_envelope(&envelope, &signer).await?;
    let digest = statement
        .open()
        .context(ErrorData::SigningFailed {
            message: "Could not read the signed session".to_string(),
        })?
        .digest;

    let hello = serde_json::json!({ "type": "session", "statement": statement });
    let mut handshake = Handshake::new(Side::Client, &keys, &challenge.verifier_key, &digest)
        .context(ErrorData::SigningFailed {
            message: "Could not start the session handshake".to_string(),
        })?;
    let first = handshake.write().context(ErrorData::SigningFailed {
        message: "Could not start the session handshake".to_string(),
    })?;
    let send_failed = || {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: "The session closed during the handshake".to_string(),
            url: None,
        })
    };
    send.send(Message::Text(hello.to_string().into()))
        .await
        .map_err(|_| send_failed())?;
    send.send(Message::Binary(first.into()))
        .await
        .map_err(|_| send_failed())?;
    loop {
        let message = stream
            .next()
            .await
            .ok_or_else(send_failed)?
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "The session failed during the handshake".to_string(),
                url: None,
            })?;
        match message {
            Message::Binary(reply) => {
                handshake.read(&reply).context(ErrorData::SigningFailed {
                    message: "The machine's handshake does not match the signed session"
                        .to_string(),
                })?;
                break;
            }
            Message::Text(text) => {
                return Err(AlienError::new(ErrorData::SigningFailed {
                    message: format!("The machine refused the session: {text}"),
                }));
            }
            Message::Close(_) => return Err(send_failed()),
            _ => {}
        }
    }
    handshake.finish().context(ErrorData::SigningFailed {
        message: "The session handshake did not finish".to_string(),
    })
}

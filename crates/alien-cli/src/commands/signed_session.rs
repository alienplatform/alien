//! The engineer's side of a signed exec session.
//!
//! A protected machine opens a session only after the engineer's device signs
//! a `session/v1` statement naming the machine's fresh challenge. The CLI
//! verifies the project's signer list from the pinned fingerprint, prints
//! what it is about to sign, signs it, and runs the Noise handshake. From then
//! on every frame is encrypted, so the control plane relaying the session can
//! neither read nor inject anything.

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::session::{
    Challenge, Channel, Handshake, SessionBody, SessionKeys, SessionTarget, Side,
    MAX_SESSION_SECONDS,
};
use alien_signing::{InstallId, Kind, Role};
use futures::StreamExt;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::signing::{self, api::SigningApi};

/// Printed before every signed session. The statement binds the session to
/// the install, not to one machine in it, until machines can prove which
/// one they are.
pub const SESSION_NOTICE: &str =
    "the session is signed; this does not prove which machine answered";

/// What the CLI needs to sign a session when the machine asks for one.
#[derive(Debug, Clone)]
pub struct SessionSigning {
    /// Where to verify the signer list and the project.
    pub ctx: ExecutionMode,
    /// Machine the session targets.
    pub machine: Option<String>,
    /// Workload the session targets, for replica sessions.
    pub workload: Option<String>,
    /// The command, or `None` for a shell.
    pub argv: Option<Vec<String>>,
    /// Whether a terminal is allocated.
    pub tty: bool,
    /// Why, for the customer's transcript.
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum MachineHello {
    Challenge {
        challenge: Challenge,
        install: String,
    },
}

/// The machine's first message: its challenge and the install id it derived.
pub struct MachineChallenge {
    challenge: Challenge,
    install: String,
}

/// Whether `text` is a machine's challenge.
pub fn challenge(text: &str) -> Option<MachineChallenge> {
    match serde_json::from_str::<MachineHello>(text) {
        Ok(MachineHello::Challenge { challenge, install }) => {
            Some(MachineChallenge { challenge, install })
        }
        Err(_) => None,
    }
}

/// Sign the session for `hello` and run the handshake on the socket.
pub async fn handshake<St>(
    signing: &SessionSigning,
    hello: MachineChallenge,
    send: &tokio::sync::mpsc::Sender<Message>,
    stream: &mut St,
) -> Result<Channel>
where
    St: futures::Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    let MachineChallenge { challenge, install } = hello;
    let install = InstallId::parse(&install).context(ErrorData::SigningFailed {
        message: format!("The machine names install '{install}', which is not an install id"),
    })?;
    let target = match (&signing.machine, &signing.workload) {
        (Some(machine), _) => SessionTarget {
            machine: Some(machine.clone()),
            workload: None,
        },
        (None, Some(workload)) => SessionTarget {
            machine: None,
            workload: Some(workload.clone()),
        },
        (None, None) => {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: "A protected machine needs a signed session; pass --machine or --resource"
                    .to_string(),
            }))
        }
    };
    let api = SigningApi::from_context(&signing.ctx).await?;
    let (chain, signer) = signing::project_signer(&api, Role::Operate).await?;

    let keys = SessionKeys::generate().context(ErrorData::SigningFailed {
        message: "Could not create the session key".to_string(),
    })?;
    let body = SessionBody {
        session_id: challenge.session_id.clone(),
        target: target.clone(),
        argv: signing.argv.clone(),
        tty: signing.tty,
        max_duration_seconds: MAX_SESSION_SECONDS,
        requester: signer.id().to_string(),
        reason: signing.reason.clone(),
        verifier_nonce: challenge.nonce.clone(),
        verifier_key: challenge.verifier_key.clone(),
        client_key: keys.public_key(),
    };
    let reaches = match (&target.machine, &target.workload) {
        (Some(machine), _) => format!("machine {machine}"),
        (None, Some(workload)) => format!("a replica of {workload}"),
        (None, None) => "the install".to_string(),
    };
    eprintln!(
        "Signing session {} for {install} ({reaches}) as '{}': {}",
        body.session_id,
        signer.id(),
        body.argv
            .as_ref()
            .map(|argv| argv.join(" "))
            .unwrap_or_else(|| "a shell".to_string())
    );
    eprintln!("  {SESSION_NOTICE}");
    let envelope = signing::envelope(Kind::Session, &chain, Some(&install), body);
    if let Some(gesture) = signer.gesture() {
        eprintln!("  {gesture} to sign the session ▸");
    }
    let statement = signing::sign_envelope(&envelope, &signer)?;
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

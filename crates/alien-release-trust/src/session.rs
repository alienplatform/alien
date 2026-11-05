//! The channel of a signed exec session, without any network code.
//!
//! 1. The enforcer sends a [`Challenge`]: a fresh nonce and an ephemeral
//!    X25519 key for this session.
//! 2. The engineer's device signs a `session/v1` statement naming that nonce,
//!    that key and the client's own ephemeral key.
//! 3. Both sides run `Noise_KK_25519_ChaChaPoly_SHA256` with the two keys,
//!    bound to the statement's digest, so the session is the one that was
//!    signed. Every later frame is encrypted and numbered: whoever relays
//!    the frames reads nothing, and a frame it injects, drops, reorders or
//!    replays fails to decrypt.
//! 4. The enforcer appends every frame to a hash-chained [`Transcript`].

use alien_error::AlienError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::kinds::SessionChallenge;

const NOISE_PATTERN: &str = "Noise_KK_25519_ChaChaPoly_SHA256";
/// Largest Noise message.
const MAX_MESSAGE: usize = 65535;
/// Plaintext room in one Noise message, after the tag and the frame kind.
pub const MAX_FRAME_PAYLOAD: usize = MAX_MESSAGE - 16 - 1;
/// How long a challenge's nonce stays usable.
pub const NONCE_LIFETIME_SECONDS: u32 = 120;

/// An X25519 key pair for one session.
pub struct SessionKeys {
    private: Vec<u8>,
    public: Vec<u8>,
}

impl SessionKeys {
    /// A new key pair.
    pub fn generate() -> Result<Self> {
        let keypair = builder()?
            .generate_keypair()
            .map_err(|error| channel_error(format!("could not generate a session key: {error}")))?;
        Ok(Self {
            private: keypair.private,
            public: keypair.public,
        })
    }

    /// The public key, base64.
    pub fn public_key(&self) -> String {
        STANDARD.encode(&self.public)
    }
}

/// What the enforcer sends first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Challenge {
    /// Session id the enforcer was asked to open.
    pub session_id: String,
    /// Fresh random nonce, base64.
    pub nonce: String,
    /// The enforcer's ephemeral public key for this session, base64.
    pub verifier_key: String,
    /// When the enforcer issued it.
    pub issued_at: DateTime<Utc>,
}

impl Challenge {
    /// A new challenge for `session_id` with `keys`.
    pub fn new(session_id: &str, keys: &SessionKeys) -> Result<Self> {
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce)
            .map_err(|error| channel_error(format!("no randomness: {error}")))?;
        Ok(Self {
            session_id: session_id.to_string(),
            nonce: STANDARD.encode(nonce),
            verifier_key: keys.public_key(),
            issued_at: Utc::now(),
        })
    }

    /// What [`crate::kinds::SessionBody::check`] compares the statement to.
    pub fn expectation(&self) -> SessionChallenge {
        SessionChallenge {
            nonce: self.nonce.clone(),
            verifier_key: self.verifier_key.clone(),
            issued_at: self.issued_at,
            nonce_lifetime_seconds: NONCE_LIFETIME_SECONDS,
        }
    }
}

/// One side of the handshake, before it completes.
pub struct Handshake {
    state: snow::HandshakeState,
}

/// Which side of the channel this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The engineer's CLI, which starts the handshake.
    Client,
    /// The enforcer.
    Enforcer,
}

impl Handshake {
    /// Start a handshake. `own` are this side's session keys, `remote_key` the
    /// other side's public key (base64) and `statement_digest` the signed
    /// session statement's digest, which both sides mix in.
    pub fn new(
        side: Side,
        own: &SessionKeys,
        remote_key: &str,
        statement_digest: &str,
    ) -> Result<Self> {
        let remote = STANDARD
            .decode(remote_key)
            .map_err(|_| channel_error("the remote session key is not base64"))?;
        if remote.len() != 32 {
            return Err(channel_error("the remote session key is not 32 bytes"));
        }
        let prologue = prologue(statement_digest);
        let builder = builder()?
            .local_private_key(&own.private)
            .map_err(|error| channel_error(format!("bad local key: {error}")))?
            .remote_public_key(&remote)
            .map_err(|error| channel_error(format!("bad remote key: {error}")))?
            .prologue(&prologue)
            .map_err(|error| channel_error(format!("bad prologue: {error}")))?;
        let state = match side {
            Side::Client => builder.build_initiator(),
            Side::Enforcer => builder.build_responder(),
        }
        .map_err(|error| channel_error(format!("could not start the handshake: {error}")))?;
        Ok(Self { state })
    }

    /// The next handshake message to send.
    pub fn write(&mut self) -> Result<Vec<u8>> {
        let mut message = vec![0u8; MAX_MESSAGE];
        let length = self
            .state
            .write_message(&[], &mut message)
            .map_err(|error| channel_error(format!("handshake failed: {error}")))?;
        message.truncate(length);
        Ok(message)
    }

    /// Read the other side's handshake message.
    pub fn read(&mut self, message: &[u8]) -> Result<()> {
        let mut payload = vec![0u8; MAX_MESSAGE];
        self.state
            .read_message(message, &mut payload)
            .map_err(|error| channel_error(format!("handshake failed: {error}")))?;
        Ok(())
    }

    /// Finish into an encrypted channel.
    pub fn finish(self) -> Result<Channel> {
        if !self.state.is_handshake_finished() {
            return Err(channel_error("the handshake is not finished"));
        }
        let transport = self
            .state
            .into_transport_mode()
            .map_err(|error| channel_error(format!("handshake failed: {error}")))?;
        Ok(Channel { transport })
    }
}

/// What one frame carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// Terminal or process bytes.
    Data,
    /// A JSON control message (resize, exit, error).
    Control,
}

/// An encrypted, numbered channel.
pub struct Channel {
    transport: snow::TransportState,
}

impl Channel {
    /// Encrypt one frame.
    pub fn seal(&mut self, kind: FrameKind, payload: &[u8]) -> Result<Vec<u8>> {
        if payload.len() > MAX_FRAME_PAYLOAD {
            return Err(channel_error("the frame is too large"));
        }
        let mut plaintext = Vec::with_capacity(payload.len() + 1);
        plaintext.push(match kind {
            FrameKind::Data => 0,
            FrameKind::Control => 1,
        });
        plaintext.extend_from_slice(payload);
        let mut message = vec![0u8; plaintext.len() + 16];
        let length = self
            .transport
            .write_message(&plaintext, &mut message)
            .map_err(|error| channel_error(format!("could not encrypt a frame: {error}")))?;
        message.truncate(length);
        Ok(message)
    }

    /// Decrypt one frame. A frame that was changed, injected, replayed or
    /// reordered fails, and the channel can't be used after that.
    pub fn open(&mut self, message: &[u8]) -> Result<(FrameKind, Vec<u8>)> {
        let mut plaintext = vec![0u8; message.len()];
        let length = self
            .transport
            .read_message(message, &mut plaintext)
            .map_err(|_| {
                channel_error("a frame failed to decrypt: it was changed, injected or replayed")
            })?;
        plaintext.truncate(length);
        let (kind, payload) = plaintext
            .split_first()
            .ok_or_else(|| channel_error("an empty frame"))?;
        let kind = match kind {
            0 => FrameKind::Data,
            1 => FrameKind::Control,
            _ => return Err(channel_error("an unknown frame kind")),
        };
        Ok((kind, payload.to_vec()))
    }
}

/// Direction of a transcript entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// From the engineer to the enforcer.
    In,
    /// From the enforcer to the engineer.
    Out,
}

/// One transcript entry. Each names the hash of the one before, so an entry
/// that is removed or changed breaks the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptEntry {
    /// Session id.
    pub session_id: String,
    /// Position in the session, from 0. Entry 0 holds the statement.
    pub index: u64,
    /// `sha256:<hex>` of the previous entry, or the statement digest for entry 0.
    pub previous: String,
    /// When it was recorded.
    pub at: DateTime<Utc>,
    /// Direction, for frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// Base64 of the frame, or of the statement JSON for entry 0.
    pub data: String,
}

impl TranscriptEntry {
    /// `sha256:<hex>` of this entry's JSON.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("an entry always serializes");
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }
}

/// A hash-chained transcript of one session.
pub struct Transcript {
    session_id: String,
    index: u64,
    previous: String,
}

impl Transcript {
    /// Start with the signed statement as entry 0.
    pub fn start(
        session_id: &str,
        statement_digest: &str,
        statement_json: &str,
    ) -> (Self, TranscriptEntry) {
        let entry = TranscriptEntry {
            session_id: session_id.to_string(),
            index: 0,
            previous: statement_digest.to_string(),
            at: Utc::now(),
            direction: None,
            data: STANDARD.encode(statement_json),
        };
        let transcript = Self {
            session_id: session_id.to_string(),
            index: 1,
            previous: entry.digest(),
        };
        (transcript, entry)
    }

    /// Record one frame.
    pub fn record(&mut self, direction: Direction, bytes: &[u8]) -> TranscriptEntry {
        let entry = TranscriptEntry {
            session_id: self.session_id.clone(),
            index: self.index,
            previous: self.previous.clone(),
            at: Utc::now(),
            direction: Some(direction),
            data: STANDARD.encode(bytes),
        };
        self.index += 1;
        self.previous = entry.digest();
        entry
    }
}

/// Check that `entries` form one unbroken chain from the statement.
pub fn verify_transcript(statement_digest: &str, entries: &[TranscriptEntry]) -> Result<()> {
    let mut previous = statement_digest.to_string();
    for (index, entry) in entries.iter().enumerate() {
        if entry.index != index as u64 || entry.previous != previous {
            return Err(channel_error(format!(
                "the transcript breaks at entry {index}"
            )));
        }
        previous = entry.digest();
    }
    Ok(())
}

fn builder() -> Result<snow::Builder<'static>> {
    let params: snow::params::NoiseParams = NOISE_PATTERN
        .parse()
        .map_err(|error| channel_error(format!("bad noise pattern: {error:?}")))?;
    Ok(snow::Builder::new(params))
}

fn prologue(statement_digest: &str) -> Vec<u8> {
    format!("session/v1 {statement_digest}").into_bytes()
}

fn channel_error(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BodyRefused {
        kind: "session/v1".to_string(),
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(digest_client: &str, digest_enforcer: &str) -> Result<(Channel, Channel)> {
        let client_keys = SessionKeys::generate()?;
        let enforcer_keys = SessionKeys::generate()?;
        let mut client = Handshake::new(
            Side::Client,
            &client_keys,
            &enforcer_keys.public_key(),
            digest_client,
        )?;
        let mut enforcer = Handshake::new(
            Side::Enforcer,
            &enforcer_keys,
            &client_keys.public_key(),
            digest_enforcer,
        )?;
        enforcer.read(&client.write()?)?;
        client.read(&enforcer.write()?)?;
        Ok((client.finish()?, enforcer.finish()?))
    }

    #[test]
    fn frames_cross_the_channel_and_nothing_else_does() {
        let (mut client, mut enforcer) = pair("sha256:s", "sha256:s").unwrap();
        let first = client.seal(FrameKind::Data, b"ls -la\n").unwrap();
        let second = client.seal(FrameKind::Data, b"whoami\n").unwrap();
        assert!(
            !first.windows(6).any(|window| window == b"ls -la"),
            "the relay sees ciphertext"
        );

        // A relay that drops the first frame or injects its own breaks the channel.
        let mut skipped = pair("sha256:s", "sha256:s").unwrap();
        let a = skipped.0.seal(FrameKind::Data, b"one").unwrap();
        let b = skipped.0.seal(FrameKind::Data, b"two").unwrap();
        skipped
            .1
            .open(&b)
            .expect_err("a reordered or dropped frame fails");
        drop(a);

        assert_eq!(
            enforcer.open(&first).unwrap(),
            (FrameKind::Data, b"ls -la\n".to_vec())
        );
        enforcer.open(&first).expect_err("a replayed frame fails");

        let (mut client2, mut enforcer2) = pair("sha256:s", "sha256:s").unwrap();
        let mut forged = client2.seal(FrameKind::Data, b"exit\n").unwrap();
        forged[3] ^= 0x01;
        enforcer2.open(&forged).expect_err("a changed frame fails");
        drop((second, client2));

        let reply = enforcer
            .seal(FrameKind::Control, br#"{"type":"exit","exitCode":0}"#)
            .unwrap();
        assert_eq!(client.open(&reply).unwrap().0, FrameKind::Control);
    }

    #[test]
    fn the_handshake_is_bound_to_the_signed_statement() {
        assert!(
            pair("sha256:signed", "sha256:other").is_err(),
            "a different statement can't complete the handshake"
        );
    }

    #[test]
    fn the_handshake_needs_the_keys_the_statement_names() {
        let client_keys = SessionKeys::generate().unwrap();
        let enforcer_keys = SessionKeys::generate().unwrap();
        let relay_keys = SessionKeys::generate().unwrap();
        // The relay substitutes its own key for the enforcer's.
        let mut client = Handshake::new(
            Side::Client,
            &client_keys,
            &relay_keys.public_key(),
            "sha256:s",
        )
        .unwrap();
        let mut enforcer = Handshake::new(
            Side::Enforcer,
            &enforcer_keys,
            &client_keys.public_key(),
            "sha256:s",
        )
        .unwrap();
        assert!(enforcer.read(&client.write().unwrap()).is_err());
    }

    #[test]
    fn a_transcript_with_a_removed_or_changed_entry_is_caught() {
        let (mut transcript, first) = Transcript::start("ses_1", "sha256:statement", "{}");
        let mut entries = vec![first];
        entries.push(transcript.record(Direction::In, b"rm -rf /tmp/x\n"));
        entries.push(transcript.record(Direction::Out, b"done\n"));
        verify_transcript("sha256:statement", &entries).unwrap();

        let mut removed = entries.clone();
        removed.remove(1);
        verify_transcript("sha256:statement", &removed).expect_err("a removed entry is caught");
        let mut changed = entries.clone();
        changed[1].data = STANDARD.encode(b"ls\n");
        verify_transcript("sha256:statement", &changed).expect_err("a changed entry is caught");
        verify_transcript("sha256:other", &entries).expect_err("another statement is caught");
    }
}

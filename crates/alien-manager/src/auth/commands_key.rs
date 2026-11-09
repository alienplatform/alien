//! The commands-only key of a deployment's command receivers built before
//! capability files.
//!
//! Such a receiver reads its credential from `ALIEN_COMMANDS_TOKEN` and cannot
//! read the capability file. The manager gives it this key instead of the
//! deployment token: the key leases and answers commands for its deployment
//! and does nothing else (see [`Role::CommandsReceiver`](super::Role)).
//!
//! The raw key is derived from the manager's signing key, the deployment id
//! and the deployment's key generation, so every sync hands the same key to the
//! pull target and nothing but its hash is stored. The key stays valid until
//! the manager revokes it, which it does once the deployment's current and
//! desired releases have no such receiver left. Each revocation moves the
//! generation forward, so a revoked key is never issued again: a later release
//! with such a receiver gets a new key.

use alien_error::AlienError;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::dev::LogEntry;
use crate::ids::sha256_hash;
use crate::routes::AppState;
use crate::traits::token_store::{CreateTokenParams, TokenStore, TokenType};

/// What issuing a commands key needs. A request handler builds it from
/// [`AppState`]; the deployment loop, which has no request state, builds it
/// from the same parts.
#[derive(Clone, Copy)]
pub struct CommandsKeyStores<'a> {
    pub signing_key: &'a [u8],
    pub token_store: &'a dyn TokenStore,
    pub log_buffer: &'a crate::dev::LogBuffer,
}

impl<'a> CommandsKeyStores<'a> {
    pub fn from_state(state: &'a AppState) -> Self {
        Self {
            signing_key: &state.config.response_signing_key,
            token_store: state.token_store.as_ref(),
            log_buffer: state.log_buffer.as_ref(),
        }
    }
}

/// Raw key length after the prefix, in hex characters. Matches generated tokens.
const KEY_HEX_LEN: usize = 40;

/// The raw key of `deployment_id` in `generation`, or `None` when the manager
/// has no signing key to derive it from. A guessable key would be worse than
/// none.
fn derive(signing_key: &[u8], deployment_id: &str, generation: u64) -> Option<String> {
    if signing_key.is_empty() {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_key).expect("HMAC accepts any key");
    mac.update(format!("commands-receiver-key.v2:{deployment_id}:{generation}").as_bytes());
    let digest = hex::encode(mac.finalize().into_bytes());
    Some(format!(
        "{}{}",
        TokenType::CommandsReceiver.prefix(),
        &digest[..KEY_HEX_LEN]
    ))
}

/// Make sure the deployment's commands key is accepted and return it.
/// `receivers` are the resources that need it; they are named in the warning
/// shown to the vendor when the key is issued. `None` means the manager cannot
/// supply a key; that is logged, and the receiver that needed it fails to
/// start with a named cause instead of starting without a credential.
pub async fn ensure(state: &AppState, deployment_id: &str, receivers: &[String]) -> Option<String> {
    ensure_with(
        &CommandsKeyStores::from_state(state),
        deployment_id,
        receivers,
    )
    .await
}

/// [`ensure`] without request state.
pub async fn ensure_with(
    stores: &CommandsKeyStores<'_>,
    deployment_id: &str,
    receivers: &[String],
) -> Option<String> {
    if stores.signing_key.is_empty() {
        tracing::warn!(%deployment_id, "The manager has no signing key, so it cannot issue the commands key a command receiver built before capability files needs");
        return None;
    }
    // A revocation that lands between reading the generation and storing the
    // key would leave a key of the old generation accepted, so the generation is
    // read again after the key is stored.
    for _ in 0..3 {
        let generation = match stores
            .token_store
            .commands_key_generation(deployment_id)
            .await
        {
            Ok(generation) => generation,
            Err(error) => {
                tracing::warn!(%deployment_id, %error, "Cannot read the commands key generation");
                return None;
            }
        };
        let raw = derive(stores.signing_key, deployment_id, generation)?;
        let key_hash = sha256_hash(&raw);
        match stores.token_store.validate_token(&key_hash).await {
            Ok(Some(_)) => return Some(raw),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%deployment_id, %error, "Cannot look up the commands key");
                return None;
            }
        }
        let created = stores
            .token_store
            .create_token(CreateTokenParams {
                token_type: TokenType::CommandsReceiver,
                key_prefix: raw[..12].to_string(),
                key_hash: key_hash.clone(),
                deployment_group_id: None,
                deployment_id: Some(deployment_id.to_string()),
            })
            .await;
        match created {
            Ok(_) => {}
            // A concurrent sync may have issued it first.
            Err(error) => match stores.token_store.validate_token(&key_hash).await {
                Ok(Some(_)) => return Some(raw),
                _ => {
                    tracing::warn!(%deployment_id, %error, "Cannot issue the commands key");
                    return None;
                }
            },
        }
        match stores
            .token_store
            .commands_key_generation(deployment_id)
            .await
        {
            Ok(now) if now == generation => {
                warn_vendor(stores, deployment_id, receivers).await;
                return Some(raw);
            }
            Ok(_) => {
                // Revoked while issuing: remove what was just stored and retry.
                if let Ok(Some(stale)) = stores.token_store.validate_token(&key_hash).await {
                    if let Err(error) = stores.token_store.delete_token(&stale.id).await {
                        tracing::warn!(%deployment_id, %error, "Cannot remove a commands key issued during a revocation");
                        return None;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%deployment_id, %error, "Cannot read the commands key generation");
                return None;
            }
        }
    }
    tracing::warn!(%deployment_id, "The commands key was revoked repeatedly while issuing it");
    None
}

/// Tell the vendor that a deployment runs on a long-lived credential and how
/// to end it. The log reaches the deployment's log stream, which the vendor
/// reads; the app's own logs never see it.
async fn warn_vendor(stores: &CommandsKeyStores<'_>, deployment_id: &str, receivers: &[String]) {
    let resources = receivers.join(", ");
    tracing::warn!(
        %deployment_id,
        %resources,
        "Issued the commands key: command receivers without a capability file need a long-lived credential"
    );
    stores
        .log_buffer
        .push(LogEntry {
            timestamp: Utc::now(),
            deployment_id: deployment_id.to_owned(),
            body: format!(
                "Command receivers ({resources}) run on a long-lived commands-only key: \
                 they were built before capability files, or this platform does not give \
                 them one. Redeploy on a current build where the platform supports \
                 capability files to end the key."
            ),
            severity: "WARN".to_owned(),
            resource_name: None,
            attributes: vec![("event".to_owned(), "legacy-command-receivers".to_owned())],
        })
        .await;
}

/// Whether the deployment's commands key is accepted right now.
pub async fn exists(state: &AppState, deployment_id: &str) -> Result<bool, AlienError> {
    let generation = state
        .token_store
        .commands_key_generation(deployment_id)
        .await?;
    let Some(raw) = derive(
        &state.config.response_signing_key,
        deployment_id,
        generation,
    ) else {
        return Ok(false);
    };
    Ok(state
        .token_store
        .validate_token(&sha256_hash(&raw))
        .await?
        .is_some())
}

/// Delete the deployment's commands keys and move its generation forward, so
/// the same key is never issued again. A failure is logged and returns
/// `false`; the next sync tries again.
pub async fn revoke(state: &AppState, deployment_id: &str) -> bool {
    match state.token_store.revoke_commands_keys(deployment_id).await {
        Ok(()) => {
            tracing::info!(
                %deployment_id,
                "Revoked the commands key: no command receiver built before capability files is left"
            );
            true
        }
        Err(error) => {
            tracing::warn!(%deployment_id, %error, "Cannot revoke the commands key");
            false
        }
    }
}

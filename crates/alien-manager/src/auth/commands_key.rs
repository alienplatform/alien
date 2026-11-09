//! The commands-only key of a deployment's command receivers built before
//! capability files.
//!
//! Such a receiver reads its credential from `ALIEN_COMMANDS_TOKEN` and cannot
//! read the capability file. The manager gives it this key instead of the
//! deployment token: the key leases and answers commands for its deployment
//! and does nothing else (see [`Role::CommandsReceiver`](super::Role)).
//!
//! The raw key is derived from the manager's signing key and the deployment
//! id, so every sync hands the same key to the pull target and nothing but its
//! hash is stored. The key stays valid until the manager revokes it, which it
//! does once the deployment's current and desired releases have no such
//! receiver left. Because the key is derived, minting again after a revocation
//! gives the same key.

use alien_error::AlienError;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::dev::LogEntry;
use crate::ids::sha256_hash;
use crate::routes::AppState;
use crate::traits::token_store::{CreateTokenParams, TokenType};

/// Raw key length after the prefix, in hex characters. Matches generated tokens.
const KEY_HEX_LEN: usize = 40;

/// The raw key of `deployment_id`, or `None` when the manager has no signing
/// key to derive it from. A guessable key would be worse than none.
fn derive(signing_key: &[u8], deployment_id: &str) -> Option<String> {
    if signing_key.is_empty() {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_key).expect("HMAC accepts any key");
    mac.update(format!("commands-receiver-key.v1:{deployment_id}").as_bytes());
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
    let Some(raw) = derive(&state.config.response_signing_key, deployment_id) else {
        tracing::warn!(%deployment_id, "The manager has no signing key, so it cannot issue the commands key a command receiver built before capability files needs");
        return None;
    };
    let key_hash = sha256_hash(&raw);
    match state.token_store.validate_token(&key_hash).await {
        Ok(Some(_)) => return Some(raw),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%deployment_id, %error, "Cannot look up the commands key");
            return None;
        }
    }
    let created = state
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
        Ok(_) => {
            warn_vendor(state, deployment_id, receivers).await;
            Some(raw)
        }
        // A concurrent sync may have issued it first.
        Err(error) => match state.token_store.validate_token(&key_hash).await {
            Ok(Some(_)) => Some(raw),
            _ => {
                tracing::warn!(%deployment_id, %error, "Cannot issue the commands key");
                None
            }
        },
    }
}

/// Tell the vendor that a deployment runs on a long-lived credential and how
/// to end it. The log reaches the deployment's log stream, which the vendor
/// reads; the app's own logs never see it.
async fn warn_vendor(state: &AppState, deployment_id: &str, receivers: &[String]) {
    let resources = receivers.join(", ");
    tracing::warn!(
        %deployment_id,
        %resources,
        "Issued the commands key: command receivers built before capability files need a long-lived credential"
    );
    state
        .log_buffer
        .push(LogEntry {
            timestamp: Utc::now(),
            deployment_id: deployment_id.to_owned(),
            body: format!(
                "Command receivers built before capability files ({resources}) run on a \
                 long-lived commands-only key. Rebuild them with a current `alien build` and \
                 release the deployment again; the key is revoked once no release of the \
                 deployment needs it."
            ),
            severity: "WARN".to_owned(),
            resource_name: None,
            attributes: vec![("event".to_owned(), "legacy-command-receivers".to_owned())],
        })
        .await;
}

/// Whether the deployment's commands key is accepted right now.
pub async fn exists(state: &AppState, deployment_id: &str) -> Result<bool, AlienError> {
    let Some(raw) = derive(&state.config.response_signing_key, deployment_id) else {
        return Ok(false);
    };
    Ok(state
        .token_store
        .validate_token(&sha256_hash(&raw))
        .await?
        .is_some())
}

/// Delete the deployment's commands keys. A failure is logged and returns
/// `false`; the next sync tries again.
pub async fn revoke(state: &AppState, deployment_id: &str) -> bool {
    let tokens = match state.token_store.list_tokens().await {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::warn!(%deployment_id, %error, "Cannot list the commands keys to revoke");
            return false;
        }
    };
    let mut revoked_all = true;
    for token in tokens.into_iter().filter(|token| {
        token.token_type == TokenType::CommandsReceiver
            && token.deployment_id.as_deref() == Some(deployment_id)
    }) {
        match state.token_store.delete_token(&token.id).await {
            Ok(()) => tracing::info!(
                %deployment_id,
                key_prefix = %token.key_prefix,
                "Revoked the commands key: no command receiver built before capability files is left"
            ),
            Err(error) => {
                revoked_all = false;
                tracing::warn!(%deployment_id, %error, "Cannot revoke a commands key");
            }
        }
    }
    revoked_all
}

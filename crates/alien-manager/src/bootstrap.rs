//! First-start setup shared by the `alien-manager` binary and `alien serve`:
//! the admin token, the key that signs command responses, and the key that
//! signs air-gapped bundles.

use std::path::Path;

use alien_core::bundle_signature::BundleSigningKey;
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::traits::{CreateTokenParams, TokenStore, TokenType};

/// Environment variable that supplies the admin token (for example from a
/// Kubernetes Secret or a cloud secret store) instead of generating one.
pub const ADMIN_TOKEN_ENV: &str = "ALIEN_ADMIN_TOKEN";

/// Admin token file written by earlier versions of the manager image.
const LEGACY_ADMIN_TOKEN_FILE: &str = "admin-token";

/// Key that signs command responses, generated once per state directory.
const SIGNING_KEY_FILE: &str = "response-signing-key";

/// Seed of the key that signs air-gapped bundles, generated once per state
/// directory. Environments trust its public key, so it must persist.
const BUNDLE_SIGNING_KEY_FILE: &str = "bundle-signing-key";

/// How the admin token was established on this start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminToken {
    /// Generated now. Shown once; only its hash is stored.
    Generated(String),
    /// Supplied through [`ADMIN_TOKEN_ENV`] and registered.
    Provided,
    /// Already registered by an earlier start.
    Existing,
}

/// Make sure an admin token exists.
///
/// A token in [`ADMIN_TOKEN_ENV`] is registered if it isn't already, so an
/// installation can manage the token as a secret. Otherwise the first start
/// generates one; later starts keep the registered token.
pub async fn ensure_admin_token(
    token_store: &dyn TokenStore,
    state_dir: &Path,
    provided: Option<String>,
) -> Result<AdminToken> {
    if let Some(token) = provided.filter(|token| !token.trim().is_empty()) {
        let token = token.trim().to_string();
        if !token.starts_with(TokenType::Admin.prefix()) {
            return Err(AlienError::new(ErrorData::ServerInitFailed {
                reason: format!(
                    "{ADMIN_TOKEN_ENV} must start with {}",
                    TokenType::Admin.prefix()
                ),
            }));
        }
        register(token_store, &token).await?;
        return Ok(AdminToken::Provided);
    }

    // Earlier images kept the token in a file; keep honoring it.
    let legacy = state_dir.join(LEGACY_ADMIN_TOKEN_FILE);
    if let Ok(token) = std::fs::read_to_string(&legacy) {
        register(token_store, token.trim()).await?;
        return Ok(AdminToken::Existing);
    }

    let has_admin = token_store
        .list_tokens()
        .await
        .context(ErrorData::ServerInitFailed {
            reason: "Failed to list tokens".to_string(),
        })?
        .iter()
        .any(|token| token.token_type == TokenType::Admin);
    if has_admin {
        return Ok(AdminToken::Existing);
    }

    let token = format!(
        "{}{}",
        TokenType::Admin.prefix(),
        uuid::Uuid::new_v4().simple()
    );
    register(token_store, &token).await?;
    Ok(AdminToken::Generated(token))
}

async fn register(token_store: &dyn TokenStore, token: &str) -> Result<()> {
    let key_hash = hex::encode(Sha256::digest(token.as_bytes()));
    let known =
        token_store
            .validate_token(&key_hash)
            .await
            .context(ErrorData::ServerInitFailed {
                reason: "Failed to check the admin token".to_string(),
            })?;
    if known.is_some() {
        return Ok(());
    }
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: token[..12.min(token.len())].to_string(),
            key_hash,
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .context(ErrorData::ServerInitFailed {
            reason: "Failed to register the admin token".to_string(),
        })?;
    Ok(())
}

/// The key that signs command responses: read from the state directory, or
/// generated and stored (owner-only) on first start.
pub fn response_signing_key(state_dir: &Path) -> Result<Vec<u8>> {
    secret_file_32(&state_dir.join(SIGNING_KEY_FILE)).map(|key| key.to_vec())
}

/// The key that signs air-gapped bundles: read from the state directory, or
/// generated and stored (owner-only) on first start.
pub fn bundle_signing_key(state_dir: &Path) -> Result<BundleSigningKey> {
    let seed = secret_file_32(&state_dir.join(BUNDLE_SIGNING_KEY_FILE))?;
    Ok(BundleSigningKey::from_seed(seed))
}

/// Read a 32-byte secret from `path`, creating it (owner-only) when missing.
fn secret_file_32(path: &Path) -> Result<[u8; 32]> {
    match std::fs::read(path) {
        Ok(bytes) => {
            return bytes.try_into().map_err(|_| {
                AlienError::new(ErrorData::ServerInitFailed {
                    reason: format!("{} is not a 32-byte key", path.display()),
                })
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(AlienError::new(ErrorData::ServerInitFailed {
                reason: format!("Failed to read {}: {e}", path.display()),
            }))
        }
    }
    let key: [u8; 32] = rand::random();
    alien_core::file_utils::write_secret_file(path, &key)
        .into_alien_error()
        .context(ErrorData::ServerInitFailed {
            reason: format!("Failed to write {}", path.display()),
        })?;
    Ok(key)
}

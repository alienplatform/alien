use async_trait::async_trait;

use alien_error::AlienError;

pub use super::token_store::TokenType;

use crate::auth::Subject;

/// Validates Bearer tokens and resolves the caller's identity.
///
/// Default: `TokenDbValidator` — hashes the token and looks up via TokenStore.
/// Dev mode: `PermissiveAuthValidator` — accepts any token.
///
/// Validators construct the unified [`Subject`] directly: they pull the bearer
/// from the `Authorization` header, use it to look up identity, and place the
/// raw bearer into `Subject.bearer_token` for any downstream code that needs
/// to forward it. There is no separate per-handler bearer extraction.
#[async_trait]
pub trait AuthValidator: Send + Sync {
    /// Validate the Bearer token from the Authorization header.
    /// Returns None if no token is provided (for optional-auth endpoints).
    /// Returns Err for invalid tokens.
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError>;
}

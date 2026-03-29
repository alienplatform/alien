use crate::traits::{CreateTokenParams, TokenRecord, TokenStore};
use alien_error::{AlienError, GenericError};
use async_trait::async_trait;

/// No-op token store for platform mode.
///
/// Token validation is handled by `PlatformTokenValidator` via the Platform API's
/// `/v1/whoami` endpoint. Local token issuance and storage are not needed.
pub struct NullTokenStore;

#[async_trait]
impl TokenStore for NullTokenStore {
    async fn create_token(&self, _params: CreateTokenParams) -> Result<TokenRecord, AlienError> {
        Err(AlienError::new(GenericError {
            message: "Token creation is not supported in platform mode; tokens are managed by the Platform API".to_string(),
        }))
    }

    async fn validate_token(&self, _key_hash: &str) -> Result<Option<TokenRecord>, AlienError> {
        Ok(None)
    }
}

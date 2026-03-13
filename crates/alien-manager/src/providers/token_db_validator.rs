use crate::ids::sha256_hash;
use crate::traits::auth_validator::{AuthSubject, AuthValidator, TokenScope};
use crate::traits::token_store::TokenStore;
use alien_error::{AlienError, GenericError};
use async_trait::async_trait;
use std::sync::Arc;

pub struct TokenDbValidator {
    token_store: Arc<dyn TokenStore>,
}

impl TokenDbValidator {
    pub fn new(token_store: Arc<dyn TokenStore>) -> Self {
        Self { token_store }
    }
}

#[async_trait]
impl AuthValidator for TokenDbValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<AuthSubject>, AlienError> {
        let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());

        let token = match auth_header {
            Some(header) => match header.strip_prefix("Bearer ") {
                Some(t) => t,
                None => {
                    let mut err = AlienError::new(GenericError {
                        message: "Authorization header must use Bearer scheme".to_string(),
                    });
                    err.code = "UNAUTHORIZED".to_string();
                    err.http_status_code = Some(401);
                    return Err(err);
                }
            },
            None => return Ok(None),
        };

        let hash = sha256_hash(token);

        match self.token_store.validate_token(&hash).await? {
            Some(record) => Ok(Some(AuthSubject {
                token_id: record.id,
                scope: TokenScope {
                    token_type: record.token_type,
                    deployment_group_id: record.deployment_group_id,
                    deployment_id: record.deployment_id,
                },
            })),
            None => {
                let mut err = AlienError::new(GenericError {
                    message: "Invalid token".to_string(),
                });
                err.code = "UNAUTHORIZED".to_string();
                err.http_status_code = Some(401);
                Err(err)
            }
        }
    }
}

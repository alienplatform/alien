use crate::traits::auth_validator::{AuthSubject, AuthValidator, TokenScope, TokenType};
use alien_error::AlienError;
use async_trait::async_trait;

pub struct PermissiveAuthValidator;

impl PermissiveAuthValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AuthValidator for PermissiveAuthValidator {
    async fn validate(
        &self,
        _headers: &http::HeaderMap,
    ) -> Result<Option<AuthSubject>, AlienError> {
        Ok(Some(AuthSubject {
            token_id: "dev".to_string(),
            scope: TokenScope {
                token_type: TokenType::Admin,
                deployment_group_id: None,
                deployment_id: None,
            },
            workspace_id: None,
        }))
    }
}

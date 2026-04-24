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

    /// Validate Basic auth: decode base64, extract password as the token.
    /// Username is ignored — only the password (deployment token) is validated.
    async fn validate_basic_auth(
        &self,
        basic_encoded: &str,
    ) -> Result<Option<AuthSubject>, AlienError> {
        use base64::engine::{general_purpose::STANDARD as BASE64, Engine as _};

        let decoded = BASE64.decode(basic_encoded.trim()).map_err(|_| {
            let mut err = AlienError::new(GenericError {
                message: "Invalid Basic auth encoding".to_string(),
            });
            err.code = "UNAUTHORIZED".to_string();
            err.http_status_code = Some(401);
            err
        })?;

        let decoded_str = String::from_utf8(decoded).map_err(|_| {
            let mut err = AlienError::new(GenericError {
                message: "Invalid Basic auth encoding".to_string(),
            });
            err.code = "UNAUTHORIZED".to_string();
            err.http_status_code = Some(401);
            err
        })?;

        // Format: "username:password" — we use the password as the token
        let token = decoded_str
            .split_once(':')
            .map(|(_, password)| password)
            .unwrap_or(&decoded_str);

        let hash = sha256_hash(token);

        match self.token_store.validate_token(&hash).await? {
            Some(record) => Ok(Some(AuthSubject {
                token_id: record.id,
                scope: TokenScope {
                    token_type: record.token_type,
                    deployment_group_id: record.deployment_group_id,
                    deployment_id: record.deployment_id,
                },
                workspace_id: None,
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

#[async_trait]
impl AuthValidator for TokenDbValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<AuthSubject>, AlienError> {
        let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());

        let token = match auth_header {
            Some(header) => {
                if let Some(t) = header.strip_prefix("Bearer ") {
                    t
                } else if let Some(basic) = header.strip_prefix("Basic ") {
                    // Support Basic auth for OCI registry proxy compatibility.
                    // Docker/containerd/kubelet use Basic auth when pulling images.
                    // The password field contains the deployment token.
                    return self.validate_basic_auth(basic).await;
                } else {
                    let mut err = AlienError::new(GenericError {
                        message: "Authorization header must use Bearer or Basic scheme".to_string(),
                    });
                    err.code = "UNAUTHORIZED".to_string();
                    err.http_status_code = Some(401);
                    return Err(err);
                }
            }
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
                workspace_id: None,
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

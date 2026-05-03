use crate::auth::{Role, Scope, Subject, SubjectKind};
use crate::ids::sha256_hash;
use crate::traits::auth_validator::{AuthValidator, TokenType};
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
    ) -> Result<Option<Subject>, AlienError> {
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
            Some(record) => Ok(Some(record_to_subject(&record, token.to_string()))),
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
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());

        let token = match auth_header {
            Some(header) => {
                if let Some(t) = header.strip_prefix("Bearer ") {
                    t.to_string()
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

        let hash = sha256_hash(&token);

        match self.token_store.validate_token(&hash).await? {
            Some(record) => Ok(Some(record_to_subject(&record, token))),
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

/// Build a unified [`Subject`] from an OSS token record. OSS is single-tenant
/// — `workspace_id` is always `"default"` and the project_id on every scope
/// variant is `"default"`.
fn record_to_subject(record: &crate::traits::token_store::TokenRecord, bearer: String) -> Subject {
    let (scope, role) = match (&record.token_type, record.deployment_group_id.as_deref(), record.deployment_id.as_deref()) {
        (TokenType::Admin, _, _) => (Scope::Workspace, Role::WorkspaceAdmin),
        (TokenType::DeploymentGroup, Some(group_id), _) => (
            Scope::DeploymentGroup {
                project_id: "default".to_string(),
                deployment_group_id: group_id.to_string(),
            },
            Role::DeploymentGroupDeployer,
        ),
        (TokenType::DeploymentGroup, None, _) => {
            // OSS DG tokens always carry a group_id; defensive fallback.
            (Scope::Workspace, Role::WorkspaceMember)
        }
        (TokenType::Deployment, _, Some(deployment_id)) => (
            Scope::Deployment {
                project_id: "default".to_string(),
                deployment_id: deployment_id.to_string(),
            },
            Role::DeploymentManager,
        ),
        (TokenType::Deployment, _, None) => {
            // Unscoped deployment token shouldn't exist; fail closed.
            (Scope::Workspace, Role::DeploymentViewer)
        }
    };

    Subject {
        kind: SubjectKind::ServiceAccount {
            id: record.id.clone(),
        },
        workspace_id: "default".to_string(),
        scope,
        role,
        bearer_token: bearer,
    }
}

use crate::auth::{Role, Scope, Subject, SubjectKind};
use crate::traits::auth_validator::AuthValidator;
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
        headers: &http::HeaderMap,
    ) -> Result<Option<Subject>, AlienError> {
        // Capture the bearer if one is present so token passthrough still
        // works in dev mode. The contract is symmetric across all
        // `AuthValidator` impls.
        let bearer = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("")
            .to_string();

        Ok(Some(Subject {
            kind: SubjectKind::ServiceAccount {
                id: "dev".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: bearer,
        }))
    }
}

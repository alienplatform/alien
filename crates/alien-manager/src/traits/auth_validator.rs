use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

pub use super::token_store::TokenType;

/// The scope of access a token grants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenScope {
    pub token_type: TokenType,
    pub deployment_group_id: Option<String>,
    pub deployment_id: Option<String>,
}

/// The authenticated caller's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSubject {
    pub token_id: String,
    pub scope: TokenScope,
    /// Workspace ID the caller belongs to. Set in platform mode from the whoami response.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

impl AuthSubject {
    /// True if this is an Admin token (OSS operator with full access).
    pub fn is_admin(&self) -> bool {
        self.scope.token_type == TokenType::Admin
    }

    /// True if this subject has full access to all resources.
    ///
    /// This is the primary scope-bypass check. Returns true for:
    /// - Admin tokens (OSS operator)
    /// - Workspace-wide DeploymentGroup tokens (platform: no specific group restriction)
    ///
    /// In platform mode, workspace/manager-scoped tokens map to DeploymentGroup
    /// with no group_id, giving them workspace-wide access without the Admin role
    /// (which would bypass workspace isolation in multi-tenant managers).
    pub fn has_full_access(&self) -> bool {
        self.is_admin()
            || (self.scope.token_type == TokenType::DeploymentGroup
                && self.scope.deployment_group_id.is_none())
    }

    pub fn is_deployment_group(&self) -> bool {
        self.scope.token_type == TokenType::DeploymentGroup
    }

    pub fn is_deployment(&self) -> bool {
        self.scope.token_type == TokenType::Deployment
    }

    /// Check if this subject can access a specific deployment group.
    pub fn can_access_group(&self, group_id: &str) -> bool {
        self.has_full_access()
            || self
                .scope
                .deployment_group_id
                .as_deref()
                .map_or(false, |id| id == group_id)
    }

    /// Check if this subject can access a specific deployment.
    pub fn can_access_deployment(&self, deployment_id: &str) -> bool {
        self.has_full_access()
            || self
                .scope
                .deployment_id
                .as_deref()
                .map_or(false, |id| id == deployment_id)
    }
}

/// Validates Bearer tokens and resolves the caller's identity.
///
/// Default: `TokenDbValidator` — hashes the token and looks up via TokenStore.
/// Dev mode: `PermissiveAuthValidator` — accepts any token.
#[async_trait]
pub trait AuthValidator: Send + Sync {
    /// Validate the Bearer token from the Authorization header.
    /// Returns None if no token is provided (for optional-auth endpoints).
    /// Returns Err for invalid tokens.
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<AuthSubject>, AlienError>;
}

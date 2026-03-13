use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

/// Token type determines access scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenType {
    Admin,
    DeploymentGroup,
    Deployment,
}

impl std::fmt::Display for TokenType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenType::Admin => write!(f, "admin"),
            TokenType::DeploymentGroup => write!(f, "deployment-group"),
            TokenType::Deployment => write!(f, "deployment"),
        }
    }
}

impl TokenType {
    /// Token prefix for generating tokens of this type.
    pub fn prefix(&self) -> &'static str {
        match self {
            TokenType::Admin => "ax_admin_",
            TokenType::DeploymentGroup => "ax_dg_",
            TokenType::Deployment => "ax_deploy_",
        }
    }
}

/// A stored token record (never contains the raw token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    pub token_type: TokenType,
    pub key_prefix: String,
    pub key_hash: String,
    pub deployment_group_id: Option<String>,
    pub deployment_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Parameters for creating a new token.
#[derive(Debug, Clone)]
pub struct CreateTokenParams {
    pub token_type: TokenType,
    pub key_prefix: String,
    pub key_hash: String,
    pub deployment_group_id: Option<String>,
    pub deployment_id: Option<String>,
}

/// Persistence for API tokens with SHA-256 hashed storage.
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn create_token(&self, params: CreateTokenParams) -> Result<TokenRecord, AlienError>;

    /// Look up a token by its SHA-256 hash.
    async fn validate_token(&self, key_hash: &str) -> Result<Option<TokenRecord>, AlienError>;
}

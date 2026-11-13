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
    /// Leases and answers commands for one deployment and nothing else. Held by
    /// command receivers built before capability files; see
    /// `Role::CommandsReceiver`.
    CommandsReceiver,
}

impl std::fmt::Display for TokenType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenType::Admin => write!(f, "admin"),
            TokenType::DeploymentGroup => write!(f, "deployment-group"),
            TokenType::Deployment => write!(f, "deployment"),
            TokenType::CommandsReceiver => write!(f, "commands-receiver"),
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
            TokenType::CommandsReceiver => "ax_cmds_",
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
    /// Create a token. Commands-receiver tokens are refused: they go through
    /// [`TokenStore::create_commands_key`], which checks the key generation.
    async fn create_token(&self, params: CreateTokenParams) -> Result<TokenRecord, AlienError>;

    /// Look up a token by its SHA-256 hash.
    async fn validate_token(&self, key_hash: &str) -> Result<Option<TokenRecord>, AlienError>;

    /// Delete a token by ID. Returns error if token not found.
    async fn delete_token(&self, id: &str) -> Result<(), AlienError>;

    /// List all tokens (returns key_prefix for identification, never key_hash).
    async fn list_tokens(&self) -> Result<Vec<TokenRecord>, AlienError>;

    /// The deployment's commands-key generation. It starts at 0 and only grows.
    async fn commands_key_generation(&self, deployment_id: &str) -> Result<u64, AlienError>;

    /// Store a commands key of `deployment_id` only while the deployment's
    /// commands-key generation is still `generation`, the one the key was
    /// derived at. The check and the insert are one atomic step, so a key of a
    /// generation that a revocation has passed is never stored.
    ///
    /// Returns `Ok(None)`, storing nothing, when the generation has moved on;
    /// the caller derives the key again at the new generation. Returns the new
    /// record otherwise. `create_token` refuses commands-receiver tokens, so
    /// this is the only way to store one.
    async fn create_commands_key(
        &self,
        deployment_id: &str,
        key_prefix: &str,
        key_hash: &str,
        generation: u64,
    ) -> Result<Option<TokenRecord>, AlienError>;

    /// Delete every commands key of the deployment and move its generation
    /// forward, in one step. The commands key is derived from the generation,
    /// so a revoked key is never issued again.
    async fn revoke_commands_keys(&self, deployment_id: &str) -> Result<(), AlienError>;
}

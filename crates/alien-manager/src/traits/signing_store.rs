//! Storage for signed statements the manager carries but never trusts: the
//! project's signer list versions, changes waiting for a signature, and
//! signed deployment config. Bytes are kept exactly as received; enforcers
//! verify them.

use alien_core::SignedStatement;
use alien_release_trust::Signer;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

/// Outcome of storing a signer list version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerListWrite {
    /// The version was new and is stored.
    Added,
    /// The same bytes were already stored for this version.
    AlreadyStored,
    /// Different bytes are already stored for this version.
    Conflict,
}

/// What a pending change asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum PendingChangeKind {
    /// A device asks to be added to the signer list.
    Device,
    /// A prepared statement waits for signatures.
    Statement,
}

/// Where a pending change stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum PendingChangeStatus {
    Pending,
    Approved,
    Rejected,
}

/// A change waiting for a signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeRecord {
    pub id: String,
    pub project_id: String,
    pub kind: PendingChangeKind,
    pub status: PendingChangeStatus,
    pub summary: String,
    pub deployment_id: Option<String>,
    /// For device changes.
    pub device: Option<Signer>,
    /// For statement changes: base64 payload bytes to sign.
    pub payload: Option<String>,
    /// Signatures gathered so far.
    pub signatures: Vec<alien_release_trust::Signature>,
    /// The signed statement that resolved the change.
    pub statement: Option<SignedStatement>,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait SigningStore: Send + Sync {
    /// Every stored signer list version of `project_id`, oldest first.
    async fn signer_lists(&self, project_id: &str) -> Result<Vec<SignedStatement>, AlienError>;

    /// Store signer list `version`.
    async fn add_signer_list(
        &self,
        project_id: &str,
        version: u64,
        statement: &SignedStatement,
    ) -> Result<SignerListWrite, AlienError>;

    async fn create_pending_change(&self, change: &PendingChangeRecord) -> Result<(), AlienError>;

    /// Pending changes of `project_id`, newest first.
    async fn pending_changes(
        &self,
        project_id: &str,
    ) -> Result<Vec<PendingChangeRecord>, AlienError>;

    async fn get_pending_change(&self, id: &str)
        -> Result<Option<PendingChangeRecord>, AlienError>;

    async fn update_pending_change(&self, change: &PendingChangeRecord) -> Result<(), AlienError>;

    /// Store a signed config statement for a deployment.
    async fn add_config_statement(
        &self,
        deployment_id: &str,
        sequence: u64,
        statement: &SignedStatement,
    ) -> Result<(), AlienError>;

    /// The config statement with the highest sequence for a deployment.
    async fn latest_config_statement(
        &self,
        deployment_id: &str,
    ) -> Result<Option<SignedStatement>, AlienError>;
}

//! Storage for signed statements the manager carries but never trusts: the
//! project's signer list versions, prepared next lists waiting for an
//! admin, and signed deployment config. Bytes are kept exactly as received;
//! enforcers verify them.

use alien_core::SignedStatement;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use alien_error::AlienError;

/// One stored signer list version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerListRecord {
    /// Version, from the statement's sequence.
    pub version: u64,
    /// The statement, as received.
    pub statement: SignedStatement,
    /// Who stored it, for display.
    pub created_by: String,
    /// When it was stored.
    pub created_at: DateTime<Utc>,
}

/// Outcome of storing a signer list version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerListWrite {
    /// The version was new and is stored.
    Added,
    /// The same bytes were already stored for this version.
    AlreadyStored,
    /// Different bytes are already stored for this version.
    Conflict(SignerListRecord),
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

/// The body of the next signer list, prepared for an admin to review and
/// sign. A device request, a removal and a list refresh all take this form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeRecord {
    pub id: String,
    pub project_id: String,
    pub status: PendingChangeStatus,
    /// Display only.
    pub summary: String,
    /// Signer list version the body was prepared from.
    pub based_on_version: u64,
    /// `SignerListBody` JSON.
    pub body: serde_json::Value,
    /// Who filed it, display only.
    pub requested_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    /// The signer list version that approved it.
    pub signer_list_version: Option<u64>,
}

#[async_trait]
pub trait SigningStore: Send + Sync {
    /// Every stored signer list version of `project_id`, oldest first.
    async fn signer_lists(&self, project_id: &str) -> Result<Vec<SignerListRecord>, AlienError>;

    /// Store signer list `record.version`. Versions never change once stored.
    async fn add_signer_list(
        &self,
        project_id: &str,
        record: &SignerListRecord,
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

    /// Every config statement stored for a deployment, lowest sequence first.
    async fn config_statements(
        &self,
        deployment_id: &str,
    ) -> Result<Vec<SignedStatement>, AlienError>;

    /// The config statement with the highest sequence for a deployment.
    async fn latest_config_statement(
        &self,
        deployment_id: &str,
    ) -> Result<Option<SignedStatement>, AlienError>;
}

//! Turso database for Agent state
//!
//! Storage model:
//! - `state` table: Key-value for deployment_state (includes target_release)
//! - `deployment_config` table: Key-value for deployment configuration
//! - `approvals` table: Approval records (linked by release_id)
//! - `telemetry` table: Buffer for offline telemetry push
//!
//! Uses Turso with AEGIS-256 encryption for data at rest.

use alien_core::{DeploymentConfig, DeploymentState, ReleaseInfo};
use alien_error::{Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Builder, Connection, Database, EncryptionOpts};

use crate::error::{ErrorData, Result};

// =============================================================================
// Types
// =============================================================================

/// Approval status for a target release
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    /// Auto-approved (no manual approval required)
    #[default]
    Auto,
    /// Pending manual approval
    Pending,
    /// Approved by user
    Approved,
    /// Rejected by user
    Rejected,
}

impl ApprovalStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::Auto => "auto",
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "pending" => ApprovalStatus::Pending,
            "approved" => ApprovalStatus::Approved,
            "rejected" => ApprovalStatus::Rejected,
            _ => ApprovalStatus::Auto,
        }
    }
}

/// Approval record
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    /// Release metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_info: Option<ReleaseInfo>,
    pub deployment_config: DeploymentConfig,
    pub status: ApprovalStatus,
    pub reason: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
}

// =============================================================================
// Database
// =============================================================================

/// Agent database for persisting state across restarts
pub struct AgentDb {
    conn: Arc<Mutex<Connection>>,
}

impl AgentDb {
    /// Create or open the agent database with encryption
    pub async fn new(data_dir: &str, encryption_key: &str) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to create data directory".to_string(),
            })?;

        let db_path = Path::new(data_dir).join("agent.db");
        let db_path_str = db_path.to_string_lossy();

        let encryption_opts = EncryptionOpts {
            cipher: "aegis256".to_string(),
            hexkey: encryption_key.to_string(),
        };

        let db: Database = Builder::new_local(&db_path_str)
            .experimental_encryption(true)
            .with_encryption(encryption_opts)
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to open encrypted database".to_string(),
            })?;

        let conn = db
            .connect()
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to connect to database".to_string(),
            })?;

        Self::run_migrations(&conn).await?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn run_migrations(conn: &Connection) -> Result<()> {
        let schema = include_str!("schema.sql");
        for statement in schema.split(';') {
            let mut lines: Vec<&str> = statement.lines().collect();
            while !lines.is_empty() {
                let first = lines[0].trim();
                if first.is_empty() || first.starts_with("--") {
                    lines.remove(0);
                } else {
                    break;
                }
            }
            let stmt = lines.join("\n");
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            conn.execute(stmt, ())
                .await
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: format!("Failed to run migration: {}", &stmt[..stmt.len().min(50)]),
                })?;
        }
        Ok(())
    }

    // =========================================================================
    // Deployment State
    // =========================================================================

    /// Get the current deployment state
    pub async fn get_deployment_state(&self) -> Result<Option<DeploymentState>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query("SELECT value FROM state WHERE key = 'deployment_state'", ())
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query deployment_state".to_string(),
            })?;

        match rows
            .next()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to fetch deployment_state row".to_string(),
            })? {
            Some(row) => {
                let json: String =
                    row.get(0)
                        .into_alien_error()
                        .context(ErrorData::DatabaseError {
                            message: "Failed to read deployment_state value".to_string(),
                        })?;
                let state = serde_json::from_str(&json).into_alien_error().context(
                    ErrorData::DatabaseError {
                        message: "Failed to parse deployment_state".to_string(),
                    },
                )?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }

    /// Set the current deployment state
    pub async fn set_deployment_state(&self, state: &DeploymentState) -> Result<()> {
        let conn = self.conn.lock().await;

        let json =
            serde_json::to_string(state)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to serialize deployment_state".to_string(),
                })?;

        conn.execute(
            "INSERT INTO state (key, value, updated_at) VALUES ('deployment_state', ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            (json,),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to set deployment_state".to_string(),
        })?;

        Ok(())
    }

    /// Get the current release info (the release that's currently deployed)
    pub async fn get_current_release_info(&self) -> Result<Option<ReleaseInfo>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT value FROM state WHERE key = 'current_release_info'",
                (),
            )
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query current_release_info".to_string(),
            })?;

        match rows
            .next()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to fetch current_release_info row".to_string(),
            })? {
            Some(row) => {
                let json: String =
                    row.get(0)
                        .into_alien_error()
                        .context(ErrorData::DatabaseError {
                            message: "Failed to read current_release_info value".to_string(),
                        })?;
                let info = serde_json::from_str(&json).into_alien_error().context(
                    ErrorData::DatabaseError {
                        message: "Failed to parse current_release_info".to_string(),
                    },
                )?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    /// Set the current release info (called when deployment completes)
    pub async fn set_current_release_info(&self, info: &ReleaseInfo) -> Result<()> {
        let conn = self.conn.lock().await;

        let json =
            serde_json::to_string(info)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to serialize current_release_info".to_string(),
                })?;

        conn.execute(
            "INSERT INTO state (key, value, updated_at) VALUES ('current_release_info', ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            (json,),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to set current_release_info".to_string(),
        })?;

        Ok(())
    }

    // =========================================================================
    // Deployment Config
    // =========================================================================

    const KEY_DEPLOYMENT_CONFIG: &str = "deployment_config";

    /// Get the deployment configuration
    pub async fn get_deployment_config(&self) -> Result<Option<DeploymentConfig>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT value FROM state WHERE key = ?",
                (Self::KEY_DEPLOYMENT_CONFIG,),
            )
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query deployment_config".to_string(),
            })?;

        match rows
            .next()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to fetch deployment_config row".to_string(),
            })? {
            Some(row) => {
                let json: String =
                    row.get(0)
                        .into_alien_error()
                        .context(ErrorData::DatabaseError {
                            message: "Failed to read deployment_config value".to_string(),
                        })?;
                let config = serde_json::from_str(&json).into_alien_error().context(
                    ErrorData::DatabaseError {
                        message: "Failed to parse deployment_config".to_string(),
                    },
                )?;
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    /// Set the deployment configuration
    pub async fn set_deployment_config(&self, config: &DeploymentConfig) -> Result<()> {
        let conn = self.conn.lock().await;

        let json =
            serde_json::to_string(config)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to serialize deployment_config".to_string(),
                })?;

        conn.execute(
            "INSERT INTO state (key, value, updated_at) VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            (Self::KEY_DEPLOYMENT_CONFIG, json),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to set deployment_config".to_string(),
        })?;

        Ok(())
    }

    /// Clear the deployment configuration
    pub async fn clear_deployment_config(&self) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "DELETE FROM state WHERE key = ?",
            (Self::KEY_DEPLOYMENT_CONFIG,),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to clear deployment_config".to_string(),
        })?;

        Ok(())
    }

    // =========================================================================
    // Approvals
    // =========================================================================

    /// Create a new approval record
    pub async fn create_approval(&self, approval: &Approval) -> Result<()> {
        let conn = self.conn.lock().await;

        let release_info_json = approval
            .release_info
            .as_ref()
            .map(|info| serde_json::to_string(info))
            .transpose()
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to serialize approval release_info".to_string(),
            })?;
        let config_json = serde_json::to_string(&approval.deployment_config)
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to serialize approval deployment_config".to_string(),
            })?;

        conn.execute(
            "INSERT INTO approvals (id, release_info, deployment_config, status, reason, created_at, decided_at, decided_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                approval.id.to_string(),
                release_info_json,
                config_json,
                approval.status.as_str().to_string(),
                approval.reason.clone(),
                approval.created_at.to_string(),
                approval.decided_at.clone(),
                approval.decided_by.clone(),
            ),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to create approval".to_string(),
        })?;

        Ok(())
    }

    /// Get pending approvals
    pub async fn get_pending_approvals(&self) -> Result<Vec<Approval>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT id, release_info, deployment_config, status, reason, created_at, decided_at, decided_by FROM approvals WHERE status = 'pending' ORDER BY created_at DESC",
                (),
            )
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query pending approvals".to_string(),
            })?;

        let mut approvals = Vec::new();
        while let Some(row) =
            rows.next()
                .await
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to fetch approval row".to_string(),
                })?
        {
            let id: String = row
                .get(0)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to read approval id".to_string(),
                })?;
            let release_info_json: Option<String> = row.get(1).ok();
            let config_json: String =
                row.get(2)
                    .into_alien_error()
                    .context(ErrorData::DatabaseError {
                        message: "Failed to read approval deployment_config".to_string(),
                    })?;
            let status_str: String = row.get(3).unwrap_or_else(|_| "pending".to_string());
            let reason: Option<String> = row.get(4).ok();
            let created_at: String =
                row.get(5)
                    .into_alien_error()
                    .context(ErrorData::DatabaseError {
                        message: "Failed to read approval created_at".to_string(),
                    })?;
            let decided_at: Option<String> = row.get(6).ok();
            let decided_by: Option<String> = row.get(7).ok();

            let release_info = release_info_json
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to parse approval release_info".to_string(),
                })?;
            let deployment_config = serde_json::from_str(&config_json)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to parse approval deployment_config".to_string(),
                })?;

            approvals.push(Approval {
                id,
                release_info,
                deployment_config,
                status: ApprovalStatus::from_str(&status_str),
                reason,
                created_at,
                decided_at,
                decided_by,
            });
        }

        Ok(approvals)
    }

    /// Get approval status for a specific release
    pub async fn get_approval_status_for_release(
        &self,
        release_id: &str,
    ) -> Result<Option<ApprovalStatus>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT status FROM approvals WHERE json_extract(release_info, '$.releaseId') = ? ORDER BY created_at DESC LIMIT 1",
                (release_id.to_string(),),
            )
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query approval status".to_string(),
            })?;

        match rows
            .next()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to fetch approval status row".to_string(),
            })? {
            Some(row) => {
                let status_str: String =
                    row.get(0)
                        .into_alien_error()
                        .context(ErrorData::DatabaseError {
                            message: "Failed to read approval status".to_string(),
                        })?;
                Ok(Some(ApprovalStatus::from_str(&status_str)))
            }
            None => Ok(None),
        }
    }

    /// Update an approval decision
    pub async fn decide_approval(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        reason: Option<&str>,
        decided_by: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE approvals SET status = ?, reason = ?, decided_at = datetime('now'), decided_by = ? WHERE id = ?",
            (
                status.as_str().to_string(),
                reason.map(|s| s.to_string()),
                decided_by.map(|s| s.to_string()),
                approval_id.to_string(),
            ),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to update approval".to_string(),
        })?;

        Ok(())
    }

    // =========================================================================
    // Telemetry Buffer
    // =========================================================================

    /// Store telemetry for later push
    pub async fn store_telemetry(&self, telemetry_type: &str, data: &[u8]) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO telemetry (type, data, created_at) VALUES (?, ?, datetime('now'))",
            (telemetry_type.to_string(), data.to_vec()),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to store telemetry".to_string(),
        })?;

        Ok(())
    }

    /// Get pending telemetry to push
    pub async fn get_pending_telemetry(&self, limit: u32) -> Result<Vec<(i64, String, Vec<u8>)>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query(
                "SELECT id, type, data FROM telemetry ORDER BY created_at LIMIT ?",
                (limit as i64,),
            )
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query telemetry".to_string(),
            })?;

        let mut results = Vec::new();
        while let Some(row) =
            rows.next()
                .await
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to fetch telemetry row".to_string(),
                })?
        {
            let id: i64 = row
                .get(0)
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to read telemetry id".to_string(),
                })?;
            let telemetry_type: String =
                row.get(1)
                    .into_alien_error()
                    .context(ErrorData::DatabaseError {
                        message: "Failed to read telemetry type".to_string(),
                    })?;
            let data: Vec<u8> =
                row.get(2)
                    .into_alien_error()
                    .context(ErrorData::DatabaseError {
                        message: "Failed to read telemetry data".to_string(),
                    })?;

            results.push((id, telemetry_type, data));
        }

        Ok(results)
    }

    /// Delete telemetry after successful push
    pub async fn delete_telemetry(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().await;

        for id in ids {
            conn.execute("DELETE FROM telemetry WHERE id = ?", (*id,))
                .await
                .into_alien_error()
                .context(ErrorData::DatabaseError {
                    message: "Failed to delete telemetry".to_string(),
                })?;
        }

        Ok(())
    }

    // =========================================================================
    // Initialization Data
    // =========================================================================

    /// Get deployment ID from initialization
    pub async fn get_deployment_id(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().await;

        let mut rows = conn
            .query("SELECT value FROM state WHERE key = 'deployment_id'", ())
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to query deployment_id".to_string(),
            })?;

        match rows
            .next()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseError {
                message: "Failed to fetch deployment_id row".to_string(),
            })? {
            Some(row) => {
                let deployment_id: String =
                    row.get(0)
                        .into_alien_error()
                        .context(ErrorData::DatabaseError {
                            message: "Failed to read deployment_id value".to_string(),
                        })?;
                Ok(Some(deployment_id))
            }
            None => Ok(None),
        }
    }

    /// Set deployment ID from initialization
    pub async fn set_deployment_id(&self, deployment_id: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO state (key, value, updated_at) VALUES ('deployment_id', ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            (deployment_id.to_string(),),
        )
        .await
        .into_alien_error()
        .context(ErrorData::DatabaseError {
            message: "Failed to set deployment_id".to_string(),
        })?;

        Ok(())
    }
}

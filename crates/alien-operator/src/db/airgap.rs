use alien_core::{DeploymentState, DeploymentStatus};
use alien_error::{Context, IntoAlienError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use turso::Connection;

use super::OperatorDb;
use crate::error::{ErrorData, Result};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AirgapReceipt {
    pub sequence: u64,
    pub target_hash: String,
    pub attempt: Option<String>,
}

fn database_error(message: &str) -> ErrorData {
    ErrorData::DatabaseError {
        message: message.into(),
    }
}

async fn read_json<T: DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let mut rows = conn
        .query("SELECT value FROM state WHERE key = ?", (key,))
        .await
        .into_alien_error()
        .context(database_error("Reading air-gapped state"))?;
    let Some(row) = rows
        .next()
        .await
        .into_alien_error()
        .context(database_error("Reading air-gapped state row"))?
    else {
        return Ok(None);
    };
    let value: String = row
        .get(0)
        .into_alien_error()
        .context(database_error("Reading air-gapped state value"))?;
    serde_json::from_str(&value)
        .map(Some)
        .into_alien_error()
        .context(database_error("Decoding air-gapped state"))
}

async fn write_json<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<()> {
    let value = serde_json::to_string(value)
        .into_alien_error()
        .context(database_error("Encoding air-gapped state"))?;
    conn.execute("INSERT INTO state (key, value, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at", (key, value)).await
        .into_alien_error().context(database_error("Saving air-gapped state"))?;
    Ok(())
}

impl OperatorDb {
    /// Stop new telemetry writes under the same lock used by both ingestion paths.
    /// The marker survives restarts until a newer signed target is installed.
    pub(crate) async fn seal_airgap_telemetry(&self) -> Result<()> {
        let conn = self.conn.lock().await;
        write_json(&conn, "airgap_telemetry_sealed", &true).await
    }

    pub(super) async fn ensure_telemetry_writable(conn: &Connection) -> Result<()> {
        if read_json::<bool>(conn, "airgap_telemetry_sealed")
            .await?
            .unwrap_or(false)
        {
            return Err(alien_error::AlienError::new(ErrorData::TelemetrySealed));
        }
        Ok(())
    }

    pub(crate) async fn get_airgap_receipt(&self) -> Result<Option<AirgapReceipt>> {
        let conn = self.conn.lock().await;
        read_json(&conn, "airgap_receipt").await
    }

    pub(crate) async fn record_airgap_target(&self, receipt: &AirgapReceipt) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .into_alien_error()
            .context(database_error("Beginning air-gapped target receipt"))?;
        let result = async {
            write_json(&conn, "airgap_sequence", &receipt.sequence).await?;
            write_json(&conn, "airgap_receipt", receipt).await?;
            write_json(&conn, "airgap_telemetry_sealed", &false).await
        }
        .await;
        if let Err(error) = result {
            let _ = conn.execute("ROLLBACK", ()).await;
            return Err(error);
        }
        conn.execute("COMMIT", ())
            .await
            .into_alien_error()
            .context(database_error("Committing air-gapped target receipt"))?;
        Ok(())
    }

    /// Acknowledge one explicit site attempt together with its retry request.
    /// A crash cannot leave a consumed attempt without the retry it requested.
    pub(crate) async fn request_airgap_retry(
        &self,
        expected: &DeploymentState,
        previous: Option<&AirgapReceipt>,
        receipt: &AirgapReceipt,
    ) -> Result<bool> {
        let conn = self.conn.lock().await;
        let stored: Option<AirgapReceipt> = read_json(&conn, "airgap_receipt").await?;
        let sequence: Option<u64> = read_json(&conn, "airgap_sequence").await?;
        if stored.as_ref() != previous
            || sequence != Some(receipt.sequence)
            || stored.as_ref().is_some_and(|stored| {
                stored.target_hash != receipt.target_hash || stored.attempt == receipt.attempt
            })
        {
            return Ok(false);
        }
        let mut next = expected.clone();
        if matches!(
            next.status,
            DeploymentStatus::PreflightsFailed
                | DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::RefreshFailed
        ) {
            next.retry_requested = true;
        }
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .into_alien_error()
            .context(database_error("Beginning air-gapped retry"))?;
        let result = async {
            if !Self::compare_and_set_deployment_state_locked(&conn, expected, &next).await? {
                return Ok(false);
            }
            write_json(&conn, "airgap_receipt", receipt).await?;
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => {
                conn.execute("COMMIT", ())
                    .await
                    .into_alien_error()
                    .context(database_error("Committing air-gapped retry"))?;
                Ok(true)
            }
            other => {
                conn.execute("ROLLBACK", ())
                    .await
                    .into_alien_error()
                    .context(database_error("Rolling back air-gapped retry"))?;
                other
            }
        }
    }

    /// Read the attempt and state under one lock so an old failure cannot be
    /// published as the outcome of a newly acknowledged retry.
    pub(crate) async fn airgap_status_snapshot(
        &self,
    ) -> Result<(Option<DeploymentState>, Option<u64>, Option<String>)> {
        let conn = self.conn.lock().await;
        let receipt: Option<AirgapReceipt> = read_json(&conn, "airgap_receipt").await?;
        Ok((
            read_json(&conn, "deployment_state").await?,
            read_json(&conn, "airgap_sequence").await?,
            receipt.and_then(|receipt| receipt.attempt),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receipt_failure_rolls_back_retry_and_old_attempts_cannot_repeat_it() {
        let directory = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(directory.path().to_str().unwrap(), &"22".repeat(32))
            .await
            .unwrap();
        let failed: DeploymentState = serde_json::from_value(serde_json::json!({
            "platform": "kubernetes", "status": "update-failed", "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        })).unwrap();
        db.set_deployment_state(&failed).await.unwrap();
        let first = AirgapReceipt {
            sequence: 9,
            target_hash: "verified-target".into(),
            attempt: Some("first".into()),
        };
        db.record_airgap_target(&first).await.unwrap();
        let retry = AirgapReceipt {
            attempt: Some("retry".into()),
            ..first.clone()
        };
        db.conn.lock().await.execute(
            "CREATE TRIGGER reject_airgap_receipt BEFORE INSERT ON state WHEN NEW.key = 'airgap_receipt' BEGIN SELECT RAISE(ABORT, 'forced receipt failure'); END", (),
        ).await.unwrap();
        assert!(db
            .request_airgap_retry(&failed, Some(&first), &retry)
            .await
            .is_err());
        assert!(
            !db.get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .retry_requested
        );
        assert_eq!(db.get_airgap_receipt().await.unwrap(), Some(first.clone()));
        db.conn
            .lock()
            .await
            .execute("DROP TRIGGER reject_airgap_receipt", ())
            .await
            .unwrap();
        assert!(db
            .request_airgap_retry(&failed, Some(&first), &retry)
            .await
            .unwrap());
        db.set_deployment_state(&failed).await.unwrap();
        assert!(!db
            .request_airgap_retry(&failed, Some(&first), &retry)
            .await
            .unwrap());
        assert!(!db
            .request_airgap_retry(&failed, Some(&retry), &retry)
            .await
            .unwrap());
        assert!(
            !db.get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .retry_requested
        );

        let mut running = failed.clone();
        running.status = DeploymentStatus::Running;
        db.set_deployment_state(&running).await.unwrap();
        let observe = AirgapReceipt {
            attempt: Some("observe".into()),
            ..retry.clone()
        };
        assert!(!db
            .request_airgap_retry(&failed, Some(&retry), &observe)
            .await
            .unwrap());
        assert!(db
            .request_airgap_retry(&running, Some(&retry), &observe)
            .await
            .unwrap());
        let (state, sequence, attempt) = db.airgap_status_snapshot().await.unwrap();
        assert_eq!(
            serde_json::to_value(state.unwrap()).unwrap(),
            serde_json::to_value(running).unwrap()
        );
        assert_eq!(sequence, Some(9));
        assert_eq!(attempt.as_deref(), Some("observe"));
    }
}

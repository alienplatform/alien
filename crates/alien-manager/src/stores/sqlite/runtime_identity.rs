//! One-use enrollment codes and daily operator keys for the standalone manager.
//!
//! Only SHA-256 hashes are stored. Each check-and-change is one conditional
//! statement, so two copies of a key or code race to exactly one winner.

use std::sync::Arc;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use chrono::{DateTime, Duration, Utc};

use super::database::{RowParser, SqliteDatabase};
use crate::ids::{generate_token, sha256_hash};

const CODE_LIFETIME: Duration = Duration::hours(1);
const KEY_LIFETIME: Duration = Duration::hours(24);

/// The current daily key of one deployment's operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyKeyState {
    pub deployment_id: String,
    pub expires_at: DateTime<Utc>,
    pub revision: u64,
}

pub struct SqliteRuntimeIdentityStore {
    db: Arc<SqliteDatabase>,
}

impl SqliteRuntimeIdentityStore {
    pub fn new(db: Arc<SqliteDatabase>) -> Self {
        Self { db }
    }

    async fn run(&self, sql: &str, params: impl turso::IntoParams) -> Result<u64, AlienError> {
        let conn = self.db.conn().lock().await;
        conn.execute(sql, params)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Runtime identity update failed".to_string(),
            })
    }

    async fn row(
        &self,
        sql: &str,
        params: impl turso::IntoParams,
    ) -> Result<Option<turso::Row>, AlienError> {
        let conn = self.db.conn().lock().await;
        let mut rows = conn
            .query(sql, params)
            .await
            .into_alien_error()
            .context(GenericError {
                message: "Runtime identity query failed".to_string(),
            })?;
        rows.next().await.into_alien_error().context(GenericError {
            message: "Failed to read runtime identity row".to_string(),
        })
    }

    async fn key_state(
        &self,
        deployment_id: &str,
    ) -> Result<Option<(String, DailyKeyState)>, AlienError> {
        let Some(row) = self
            .row(
                "SELECT key_hash, expires_at, revision FROM runtime_daily_keys WHERE deployment_id = ?1",
                [deployment_id],
            )
            .await?
        else {
            return Ok(None);
        };
        let p = RowParser::new(&row);
        Ok(Some((
            p.string(0, "key_hash")?,
            DailyKeyState {
                deployment_id: deployment_id.to_owned(),
                expires_at: timestamp(p.i64(1, "expires_at")?)?,
                revision: p.i64(2, "revision")?.try_into().unwrap_or_default(),
            },
        )))
    }

    /// Issue a one-hour, one-use code that enrolls this deployment's operator.
    pub async fn issue_enrollment_code(
        &self,
        deployment_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(String, DateTime<Utc>), AlienError> {
        let (code, _, code_hash) = generate_token("ax_enroll_");
        let expires_at = now + CODE_LIFETIME;
        self.run(
            "INSERT INTO runtime_enrollment_codes (code_hash, deployment_id, expires_at) VALUES (?1, ?2, ?3)",
            (code_hash, deployment_id.to_owned(), expires_at.timestamp()),
        )
        .await?;
        Ok((code, expires_at))
    }

    /// Consume a code and install the operator's first key. A retry with the
    /// same code and key hash, after a lost response, gets the same answer.
    pub async fn enroll(
        &self,
        code: &str,
        initial_key_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<DailyKeyState>, AlienError> {
        let code_hash = sha256_hash(code);
        let used = self
            .run(
                "UPDATE runtime_enrollment_codes SET initial_key_hash = ?2 \
                 WHERE code_hash = ?1 AND initial_key_hash IS NULL AND expires_at > ?3",
                (
                    code_hash.clone(),
                    initial_key_hash.to_owned(),
                    now.timestamp(),
                ),
            )
            .await?;
        let Some(row) = self
            .row(
                "SELECT deployment_id, initial_key_hash FROM runtime_enrollment_codes WHERE code_hash = ?1",
                [code_hash],
            )
            .await?
        else {
            return Ok(None);
        };
        let p = RowParser::new(&row);
        let deployment_id = p.string(0, "deployment_id")?;
        if p.optional_string(1, "initial_key_hash")?.as_deref() != Some(initial_key_hash) {
            return Ok(None);
        }
        if used == 1 {
            // A new code replaces any earlier operator of this deployment.
            self.run(
                "INSERT INTO runtime_daily_keys (deployment_id, key_hash, expires_at, revision) \
                 VALUES (?1, ?2, ?3, 1) ON CONFLICT(deployment_id) DO UPDATE SET \
                 key_hash = excluded.key_hash, expires_at = excluded.expires_at, \
                 revision = runtime_daily_keys.revision + 1",
                (
                    deployment_id.clone(),
                    initial_key_hash.to_owned(),
                    (now + KEY_LIFETIME).timestamp(),
                ),
            )
            .await?;
        }
        Ok(self
            .key_state(&deployment_id)
            .await?
            .filter(|(hash, state)| hash == initial_key_hash && state.expires_at > now)
            .map(|(_, state)| state))
    }

    /// Replace an unexpired current key with its successor. The old key stops
    /// working at once; a stale copy loses the race and gets nothing.
    pub async fn rotate(
        &self,
        deployment_id: &str,
        key: &str,
        successor_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<DailyKeyState>, AlienError> {
        let changed = self
            .run(
                "UPDATE runtime_daily_keys SET key_hash = ?3, expires_at = ?4, revision = revision + 1 \
                 WHERE deployment_id = ?1 AND key_hash = ?2 AND expires_at > ?5 AND key_hash <> ?3",
                (
                    deployment_id.to_owned(),
                    sha256_hash(key),
                    successor_hash.to_owned(),
                    (now + KEY_LIFETIME).timestamp(),
                    now.timestamp(),
                ),
            )
            .await?;
        if changed != 1 {
            return Ok(None);
        }
        Ok(self.key_state(deployment_id).await?.map(|(_, state)| state))
    }

    /// The deployment's key state when `key` is its current, unexpired key.
    pub async fn verify_key(
        &self,
        deployment_id: &str,
        key: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<DailyKeyState>, AlienError> {
        let hash = sha256_hash(key);
        Ok(self
            .key_state(deployment_id)
            .await?
            .filter(|(current, state)| *current == hash && state.expires_at > now)
            .map(|(_, state)| state))
    }
}

fn timestamp(seconds: i64) -> Result<DateTime<Utc>, AlienError> {
    DateTime::from_timestamp(seconds, 0).ok_or_else(|| {
        AlienError::new(GenericError {
            message: "Invalid runtime identity timestamp".to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (SqliteRuntimeIdentityStore, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::new(&directory.path().join("manager.db").to_string_lossy())
            .await
            .unwrap();
        (SqliteRuntimeIdentityStore::new(Arc::new(db)), directory)
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_900_000_000 + seconds, 0).unwrap()
    }

    #[tokio::test]
    async fn code_enrolls_once_and_only_within_the_hour() {
        let (store, _directory) = store().await;
        let (code, expires_at) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        assert_eq!(expires_at, at(3600));
        let key_hash = sha256_hash("ax_day_v1_first");

        let enrolled = store
            .enroll(&code, &key_hash, at(10))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(enrolled.deployment_id, "dep-1");
        assert_eq!(enrolled.expires_at, at(10) + KEY_LIFETIME);
        // The same request after a lost response gets the same answer.
        assert_eq!(
            store.enroll(&code, &key_hash, at(20)).await.unwrap(),
            Some(enrolled)
        );
        // A second key cannot take the used code.
        assert!(store
            .enroll(&code, &sha256_hash("ax_day_v1_thief"), at(20))
            .await
            .unwrap()
            .is_none());

        let (late, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        assert!(store
            .enroll(&late, &sha256_hash("ax_day_v1_late"), at(3600))
            .await
            .unwrap()
            .is_none());
        assert!(store
            .enroll("ax_enroll_unknown", &key_hash, at(10))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn rotation_retires_the_old_key_and_a_stale_copy_loses() {
        let (store, _directory) = store().await;
        let (code, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        store
            .enroll(&code, &sha256_hash("ax_day_v1_first"), at(0))
            .await
            .unwrap()
            .unwrap();
        assert!(store
            .verify_key("dep-1", "ax_day_v1_first", at(1))
            .await
            .unwrap()
            .is_some());
        assert!(store
            .verify_key("dep-2", "ax_day_v1_first", at(1))
            .await
            .unwrap()
            .is_none());

        let rotated = store
            .rotate(
                "dep-1",
                "ax_day_v1_first",
                &sha256_hash("ax_day_v1_second"),
                at(100),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rotated.revision, 2);
        assert_eq!(rotated.expires_at, at(100) + KEY_LIFETIME);
        assert!(store
            .verify_key("dep-1", "ax_day_v1_first", at(101))
            .await
            .unwrap()
            .is_none());
        assert!(store
            .verify_key("dep-1", "ax_day_v1_second", at(101))
            .await
            .unwrap()
            .is_some());
        // A copy of the old key cannot rotate to its own successor.
        assert!(store
            .rotate(
                "dep-1",
                "ax_day_v1_first",
                &sha256_hash("ax_day_v1_clone"),
                at(102)
            )
            .await
            .unwrap()
            .is_none());
        // An expired key can neither be used nor rotated.
        let expired = at(100) + KEY_LIFETIME;
        assert!(store
            .verify_key("dep-1", "ax_day_v1_second", expired)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .rotate(
                "dep-1",
                "ax_day_v1_second",
                &sha256_hash("ax_day_v1_third"),
                expired
            )
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn keys_survive_a_manager_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manager.db");
        let open = || async {
            SqliteRuntimeIdentityStore::new(Arc::new(
                SqliteDatabase::new(&path.to_string_lossy()).await.unwrap(),
            ))
        };
        let first = open().await;
        let (code, _) = first.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        first
            .enroll(&code, &sha256_hash("ax_day_v1_first"), at(0))
            .await
            .unwrap()
            .unwrap();
        drop(first);
        let second = open().await;
        assert!(second
            .verify_key("dep-1", "ax_day_v1_first", at(1))
            .await
            .unwrap()
            .is_some());
    }
}

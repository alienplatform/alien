//! One-use enrollment codes and daily operator keys for the standalone manager.
//!
//! Only SHA-256 hashes are stored. Each check-and-change is one conditional
//! statement, so two copies of a key or code race to exactly one winner.

use std::sync::Arc;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use chrono::{DateTime, Duration, Utc};

use turso::transaction::TransactionBehavior;

use super::database::{RowParser, SqliteDatabase};
use crate::ids::{generate_token, sha256_hash};

const CODE_LIFETIME: Duration = Duration::hours(1);
const KEY_LIFETIME: Duration = Duration::hours(24);
/// After a manager outage, the latest key may still rotate for a week past its
/// expiry. That week is the absolute deadline for any rotation.
const ROTATION_GRACE: Duration = Duration::days(7);

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

    /// Issue a code for a caller that holds only the deployment's token.
    /// Returns `None` when the deployment already has an operator key: a copy
    /// of the token must not be able to replace the key. The check and the
    /// insert are one statement, so no key can appear between them.
    pub async fn issue_token_enrollment_code(
        &self,
        deployment_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(String, DateTime<Utc>)>, AlienError> {
        let (code, _, code_hash) = generate_token("ax_enroll_");
        let expires_at = now + CODE_LIFETIME;
        let inserted = self
            .run(
                "INSERT INTO runtime_enrollment_codes (code_hash, deployment_id, expires_at, from_token) \
                 SELECT ?1, ?2, ?3, 1 \
                 WHERE NOT EXISTS (SELECT 1 FROM runtime_daily_keys WHERE deployment_id = ?2)",
                (code_hash, deployment_id.to_owned(), expires_at.timestamp()),
            )
            .await?;
        Ok((inserted == 1).then_some((code, expires_at)))
    }

    /// The deployment a still-unused, unexpired token-made `code` belongs to,
    /// when that deployment now has an operator key. This is why `enroll`
    /// refused it; no other refusal is reported here.
    pub async fn token_code_blocked_by_key(
        &self,
        code: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, AlienError> {
        let Some(row) = self
            .row(
                "SELECT deployment_id FROM runtime_enrollment_codes \
                 WHERE code_hash = ?1 AND from_token = 1 AND initial_key_hash IS NULL \
                 AND expires_at > ?2 AND EXISTS \
                 (SELECT 1 FROM runtime_daily_keys WHERE \
                  runtime_daily_keys.deployment_id = runtime_enrollment_codes.deployment_id)",
                (sha256_hash(code), now.timestamp()),
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(RowParser::new(&row).string(0, "deployment_id")?))
    }

    /// Consume a code and install the operator's first key. A code made from a
    /// deployment token is refused once the deployment has a key, even when it
    /// was made before the key existed. A retry with the same code and key
    /// hash, after a lost response, gets the same answer.
    /// One transaction consumes the code, installs the key and retires every
    /// other unused code of the deployment, so a crash cannot leave a used
    /// code without its key, and a leftover code cannot replace the key later.
    pub async fn enroll(
        &self,
        code: &str,
        initial_key_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<DailyKeyState>, AlienError> {
        let failed = || GenericError {
            message: "Runtime identity enrollment failed".to_string(),
        };
        let code_hash = sha256_hash(code);
        let deployment_id = {
            let mut conn = self.db.conn().lock().await;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await
                .into_alien_error()
                .context(failed())?;
            let used = tx
                .execute(
                    "UPDATE runtime_enrollment_codes SET initial_key_hash = ?2 \
                     WHERE code_hash = ?1 AND initial_key_hash IS NULL AND expires_at > ?3 \
                     AND (from_token = 0 OR NOT EXISTS (SELECT 1 FROM runtime_daily_keys \
                          WHERE runtime_daily_keys.deployment_id = runtime_enrollment_codes.deployment_id))",
                    (
                        code_hash.clone(),
                        initial_key_hash.to_owned(),
                        now.timestamp(),
                    ),
                )
                .await
                .into_alien_error()
                .context(failed())?;
            let mut rows = tx
                .query(
                    "SELECT deployment_id, initial_key_hash FROM runtime_enrollment_codes WHERE code_hash = ?1",
                    [code_hash.clone()],
                )
                .await
                .into_alien_error()
                .context(failed())?;
            let Some(row) = rows.next().await.into_alien_error().context(failed())? else {
                return Ok(None);
            };
            let p = RowParser::new(&row);
            let deployment_id = p.string(0, "deployment_id")?;
            let owner = p.optional_string(1, "initial_key_hash")?;
            drop(rows);
            if owner.as_deref() != Some(initial_key_hash) {
                return Ok(None);
            }
            if used == 1 {
                // A new code replaces any earlier operator of this deployment.
                tx.execute(
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
                .await
                .into_alien_error()
                .context(failed())?;
                tx.execute(
                    "DELETE FROM runtime_enrollment_codes \
                     WHERE deployment_id = ?1 AND initial_key_hash IS NULL",
                    [deployment_id.clone()],
                )
                .await
                .into_alien_error()
                .context(failed())?;
            }
            tx.commit().await.into_alien_error().context(failed())?;
            deployment_id
        };
        Ok(self
            .key_state(&deployment_id)
            .await?
            .filter(|(hash, state)| hash == initial_key_hash && state.expires_at > now)
            .map(|(_, state)| state))
    }

    /// Replace the current key with its successor. The old key stops working
    /// at once; a stale copy loses the race and gets nothing. Only the latest
    /// issued key can rotate, and only until a week after it expired, so an
    /// operator survives a manager outage longer than a day. An expired key
    /// still cannot exchange for credentials.
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
                    (now - ROTATION_GRACE).timestamp(),
                ),
            )
            .await?;
        if changed != 1 {
            return Ok(None);
        }
        Ok(self.key_state(deployment_id).await?.map(|(_, state)| state))
    }

    /// Whether `code` is an unexpired enrollment code, used or not.
    pub async fn code_is_live(&self, code: &str, now: DateTime<Utc>) -> Result<bool, AlienError> {
        Ok(self
            .row(
                "SELECT 1 FROM runtime_enrollment_codes WHERE code_hash = ?1 AND expires_at > ?2",
                (sha256_hash(code), now.timestamp()),
            )
            .await?
            .is_some())
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
        // An expired key cannot be used. A week after expiry it cannot rotate.
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
                expired + ROTATION_GRACE
            )
            .await
            .unwrap()
            .is_none());
    }

    /// The manager was down for three days, so the operator's key expired.
    /// The latest key still rotates once; its stale copy does not.
    #[tokio::test]
    async fn the_latest_key_rotates_within_a_week_after_an_outage() {
        let (store, _directory) = store().await;
        let (code, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        store
            .enroll(&code, &sha256_hash("ax_day_v1_first"), at(0))
            .await
            .unwrap()
            .unwrap();
        let back = at(0) + KEY_LIFETIME + Duration::days(3);
        assert!(store
            .verify_key("dep-1", "ax_day_v1_first", back)
            .await
            .unwrap()
            .is_none());
        let rotated = store
            .rotate(
                "dep-1",
                "ax_day_v1_first",
                &sha256_hash("ax_day_v1_second"),
                back,
            )
            .await
            .unwrap()
            .expect("the latest key rotates after the outage");
        assert_eq!(rotated.expires_at, back + KEY_LIFETIME);
        assert!(store
            .verify_key("dep-1", "ax_day_v1_second", back)
            .await
            .unwrap()
            .is_some());
        // A clone of the old key is refused, which shows the copy.
        assert!(store
            .rotate(
                "dep-1",
                "ax_day_v1_first",
                &sha256_hash("ax_day_v1_clone"),
                back
            )
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn enrolling_retires_every_other_unused_code_of_the_deployment() {
        let (store, _directory) = store().await;
        let (first, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        let (leftover, _) = store.issue_enrollment_code("dep-1", at(1)).await.unwrap();
        let (other, _) = store.issue_enrollment_code("dep-2", at(1)).await.unwrap();
        let key_hash = sha256_hash("ax_day_v1_first");
        store
            .enroll(&first, &key_hash, at(10))
            .await
            .unwrap()
            .unwrap();
        // A code left in a release Secret cannot replace the running key.
        assert!(store
            .enroll(&leftover, &sha256_hash("ax_day_v1_thief"), at(20))
            .await
            .unwrap()
            .is_none());
        assert!(store
            .verify_key("dep-1", "ax_day_v1_first", at(21))
            .await
            .unwrap()
            .is_some());
        // A retry of the winning enrollment still gets its answer.
        assert!(store
            .enroll(&first, &key_hash, at(22))
            .await
            .unwrap()
            .is_some());
        // Codes of other deployments are untouched.
        assert!(store
            .enroll(&other, &sha256_hash("ax_day_v1_other"), at(23))
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_token_made_code_works_only_while_the_deployment_has_no_key() {
        let (store, _directory) = store().await;
        let (first, _) = store
            .issue_token_enrollment_code("dep-1", at(0))
            .await
            .unwrap()
            .expect("no key yet");
        let (admin, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        // Another deployment's key does not block this one.
        let (other, _) = store.issue_enrollment_code("dep-2", at(0)).await.unwrap();
        store
            .enroll(&other, &sha256_hash("ax_day_v1_other"), at(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store
                .token_code_blocked_by_key(&first, at(2))
                .await
                .unwrap(),
            None
        );

        store
            .enroll(&first, &sha256_hash("ax_day_v1_first"), at(2))
            .await
            .unwrap()
            .unwrap();
        // With the key in place, no token-made code can be issued.
        assert!(store
            .issue_token_enrollment_code("dep-1", at(3))
            .await
            .unwrap()
            .is_none());
        // An admin code made before enrollment was retired by it, and one
        // made afterwards still replaces the key.
        assert!(store
            .enroll(&admin, &sha256_hash("ax_day_v1_stale"), at(4))
            .await
            .unwrap()
            .is_none());
        let (admin, _) = store.issue_enrollment_code("dep-1", at(5)).await.unwrap();
        assert!(store
            .enroll(&admin, &sha256_hash("ax_day_v1_admin"), at(6))
            .await
            .unwrap()
            .is_some());

        // A token-made code left unused next to a key is refused, and the
        // refusal is told apart from an unknown or an expired code.
        let (leftover, _, hash) = generate_token("ax_enroll_");
        store
            .run(
                "INSERT INTO runtime_enrollment_codes \
                 (code_hash, deployment_id, expires_at, from_token) VALUES (?1, 'dep-1', ?2, 1)",
                (hash, at(3600).timestamp()),
            )
            .await
            .unwrap();
        assert!(store
            .enroll(&leftover, &sha256_hash("ax_day_v1_copier"), at(10))
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .token_code_blocked_by_key(&leftover, at(10))
                .await
                .unwrap(),
            Some("dep-1".to_owned())
        );
        assert_eq!(
            store
                .token_code_blocked_by_key(&leftover, at(3600))
                .await
                .unwrap(),
            None,
            "an expired code is just expired"
        );
        assert_eq!(
            store
                .token_code_blocked_by_key("ax_enroll_unknown", at(10))
                .await
                .unwrap(),
            None
        );
        assert!(store
            .verify_key("dep-1", "ax_day_v1_admin", at(11))
            .await
            .unwrap()
            .is_some());
    }

    /// The manager fails between consuming the code and installing the key.
    /// The operator's retry with the same code and key must still enroll.
    #[tokio::test]
    async fn a_failed_key_install_leaves_the_code_unused() {
        let (store, _directory) = store().await;
        let (code, _) = store.issue_enrollment_code("dep-1", at(0)).await.unwrap();
        let key_hash = sha256_hash("ax_day_v1_first");
        let rename = |from: &'static str, to: &'static str| {
            let db = store.db.clone();
            async move {
                db.conn()
                    .lock()
                    .await
                    .execute(&format!("ALTER TABLE {from} RENAME TO {to}"), ())
                    .await
                    .unwrap();
            }
        };
        rename("runtime_daily_keys", "runtime_daily_keys_away").await;
        assert!(store.enroll(&code, &key_hash, at(10)).await.is_err());
        rename("runtime_daily_keys_away", "runtime_daily_keys").await;

        let enrolled = store
            .enroll(&code, &key_hash, at(20))
            .await
            .unwrap()
            .expect("the retry enrolls");
        assert_eq!(enrolled.expires_at, at(20) + KEY_LIFETIME);
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

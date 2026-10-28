use alien_core::{DeploymentConfig, DeploymentState, DeploymentStatus, ReleaseInfo};
use alien_error::{Context, IntoAlienError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{database_error, read_json, write_json, OperatorDb};
use crate::error::Result;

const RETRY_DELAYS_SECONDS: [i64; 3] = [30, 60, 120];
const RETRY_KEY: &str = "automatic_deployment_retry";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AutomaticRetry {
    Idle,
    Scheduled { attempt: usize, delay_seconds: i64 },
    Requested { attempt: usize },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetryBudget {
    target_hash: String,
    attempts: usize,
    retry_at: Option<DateTime<Utc>>,
}

pub(crate) fn approved_target_hash(
    release: Option<&ReleaseInfo>,
    config: &DeploymentConfig,
) -> Result<String> {
    // A new execution claim refreshes credentials and metadata. Only changes
    // to the approved target grant another automatic retry budget.
    let release_identity = match release.and_then(|release| release.release_id.as_ref()) {
        Some(id) => serde_json::to_value(id),
        None => serde_json::to_value(release),
    }
    .into_alien_error()
    .context(database_error("Encoding automatic retry release"))?;
    let mut environment: Vec<_> = config
        .environment_variables
        .variables
        .iter()
        .map(|variable| (&variable.name, &variable.value))
        .collect();
    environment.sort_unstable();
    let mut target = serde_json::to_value((
        release_identity,
        &config.stack_settings,
        &config.input_values,
        environment,
    ))
    .into_alien_error()
    .context(database_error("Encoding automatic retry target"))?;
    target.sort_all_objects();
    let target_hash = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&target)
                .into_alien_error()
                .context(database_error("Hashing automatic retry target"))?
        )
    );
    Ok(target_hash)
}

impl OperatorDb {
    /// Schedule or request a bounded retry of the same approved target. The
    /// budget and retry flag commit together so restarts cannot reset the limit.
    pub(crate) async fn prepare_automatic_retry(
        &self,
        expected: &DeploymentState,
        config: &DeploymentConfig,
        now: DateTime<Utc>,
    ) -> Result<AutomaticRetry> {
        if expected.retry_requested
            || expected.target_release.is_none()
            || !matches!(
                expected.status,
                DeploymentStatus::ProvisioningFailed | DeploymentStatus::UpdateFailed
            )
        {
            return Ok(AutomaticRetry::Idle);
        }
        let target_hash = approved_target_hash(expected.target_release.as_ref(), config)?;
        let conn = self.conn.lock().await;
        let Some(stored) = Self::get_deployment_state_locked(&conn).await? else {
            return Ok(AutomaticRetry::Idle);
        };
        if serde_json::to_value(stored)
            .into_alien_error()
            .context(database_error("Comparing retry checkpoint"))?
            != serde_json::to_value(expected)
                .into_alien_error()
                .context(database_error("Comparing expected retry checkpoint"))?
        {
            return Ok(AutomaticRetry::Idle);
        }
        let previous: Option<RetryBudget> = read_json(&conn, RETRY_KEY).await?;
        let mut budget = previous
            .filter(|budget| budget.target_hash == target_hash)
            .unwrap_or(RetryBudget {
                target_hash,
                attempts: 0,
                retry_at: None,
            });
        let Some(&delay_seconds) = RETRY_DELAYS_SECONDS.get(budget.attempts) else {
            return Ok(AutomaticRetry::Idle);
        };
        let Some(retry_at) = budget.retry_at else {
            budget.retry_at = Some(now + chrono::Duration::seconds(delay_seconds));
            write_json(&conn, RETRY_KEY, &budget).await?;
            return Ok(AutomaticRetry::Scheduled {
                attempt: budget.attempts + 1,
                delay_seconds,
            });
        };
        if now < retry_at {
            return Ok(AutomaticRetry::Idle);
        }

        let mut next = expected.clone();
        next.retry_requested = true;
        budget.attempts += 1;
        budget.retry_at = None;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .into_alien_error()
            .context(database_error("Beginning automatic retry"))?;
        let result = async {
            if !Self::compare_and_set_deployment_state_locked(&conn, expected, &next).await? {
                return Ok(false);
            }
            write_json(&conn, RETRY_KEY, &budget).await?;
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => {
                conn.execute("COMMIT", ())
                    .await
                    .into_alien_error()
                    .context(database_error("Committing automatic retry"))?;
                Ok(AutomaticRetry::Requested {
                    attempt: budget.attempts,
                })
            }
            other => {
                conn.execute("ROLLBACK", ())
                    .await
                    .into_alien_error()
                    .context(database_error("Rolling back automatic retry"))?;
                other.map(|_| AutomaticRetry::Idle)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Platform, ReleaseInfo, Stack, StackState};

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn failed_state() -> DeploymentState {
        DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::ProvisioningFailed,
            current_release: None,
            target_release: Some(ReleaseInfo {
                release_id: Some("rel_test".into()),
                version: None,
                description: None,
                stack: Stack::new("app".into()).build(),
            }),
            stack_state: Some(StackState::new(Platform::Kubernetes)),
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn config() -> DeploymentConfig {
        serde_json::from_value(serde_json::json!({
            "environmentVariables": {"variables": [], "hash": "", "createdAt": ""}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn retries_are_delayed_bounded_and_survive_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_str().unwrap();
        let mut db = OperatorDb::new(path, KEY).await.unwrap();
        let failed = failed_state();
        let config = config();
        let mut now = Utc::now();
        db.set_deployment_state(&failed).await.unwrap();
        for (index, delay) in RETRY_DELAYS_SECONDS.into_iter().enumerate() {
            let attempt = index + 1;
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Scheduled {
                    attempt,
                    delay_seconds: delay
                }
            );
            assert!(
                !db.get_deployment_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .retry_requested
            );
            now += chrono::Duration::seconds(delay - 1);
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Idle
            );
            drop(db);
            db = OperatorDb::new(path, KEY).await.unwrap();
            now += chrono::Duration::seconds(1);
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Requested { attempt }
            );
            let requested = db.get_deployment_state().await.unwrap().unwrap();
            let mut expected = failed.clone();
            expected.retry_requested = true;
            assert_eq!(
                serde_json::to_value(requested).unwrap(),
                serde_json::to_value(expected).unwrap(),
                "only the retry request changes; controllers and target remain intact"
            );
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Idle,
                "a stale reader cannot consume the same retry twice"
            );
            // The executor consumed the request, then this target failed again.
            db.set_deployment_state(&failed).await.unwrap();
            now += chrono::Duration::seconds(1);
        }
        drop(db);
        let db = OperatorDb::new(path, KEY).await.unwrap();
        assert_eq!(
            db.prepare_automatic_retry(&failed, &config, now + chrono::Duration::days(1))
                .await
                .unwrap(),
            AutomaticRetry::Idle
        );
        let mut manual = failed.clone();
        manual.retry_requested = true;
        db.set_deployment_state(&manual).await.unwrap();
        assert_eq!(
            db.prepare_automatic_retry(&manual, &config, now)
                .await
                .unwrap(),
            AutomaticRetry::Idle
        );
        assert!(
            db.get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .retry_requested,
            "the automatic limit must not consume explicit retries"
        );
    }

    #[tokio::test]
    async fn a_new_target_supersedes_the_old_retry_and_gets_its_own_budget() {
        let directory = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(directory.path().to_str().unwrap(), KEY)
            .await
            .unwrap();
        let failed = failed_state();
        let config = config();
        let now = Utc::now();
        db.set_deployment_state(&failed).await.unwrap();
        db.prepare_automatic_retry(&failed, &config, now)
            .await
            .unwrap();
        let mut newer = failed.clone();
        newer.target_release.as_mut().unwrap().release_id = Some("rel_new".into());
        db.set_deployment_state(&newer).await.unwrap();
        assert_eq!(
            db.prepare_automatic_retry(&failed, &config, now + chrono::Duration::seconds(31))
                .await
                .unwrap(),
            AutomaticRetry::Idle
        );
        assert_eq!(
            serde_json::to_value(db.get_deployment_state().await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(&newer).unwrap()
        );
        assert_eq!(
            db.prepare_automatic_retry(&newer, &config, now)
                .await
                .unwrap(),
            AutomaticRetry::Scheduled {
                attempt: 1,
                delay_seconds: 30
            }
        );
        let mut corrected = config.clone();
        corrected.input_values.insert("enabled".into(), true.into());
        assert_eq!(
            db.prepare_automatic_retry(&newer, &corrected, now + chrono::Duration::seconds(31))
                .await
                .unwrap(),
            AutomaticRetry::Scheduled {
                attempt: 1,
                delay_seconds: 30
            },
            "a corrected config on the same release is a new target"
        );
    }

    #[tokio::test]
    async fn approved_values_and_anonymous_releases_reset_the_budget() {
        let directory = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(directory.path().to_str().unwrap(), KEY)
            .await
            .unwrap();
        let mut failed = failed_state();
        failed.target_release.as_mut().unwrap().release_id = None;
        let mut config = config();
        let now = Utc::now();
        db.set_deployment_state(&failed).await.unwrap();
        db.prepare_automatic_retry(&failed, &config, now)
            .await
            .unwrap();
        config
            .environment_variables
            .variables
            .push(alien_core::EnvironmentVariable {
                name: "MODE".into(),
                value: "first".into(),
                var_type: alien_core::EnvironmentVariableType::Plain,
                target_resources: None,
            });
        for change in 0..4 {
            match change {
                0 => {}
                1 => config.environment_variables.variables[0].value = "second".into(),
                2 => config.stack_settings.airgapped = true,
                _ => failed.target_release.as_mut().unwrap().version = Some("2".into()),
            }
            db.set_deployment_state(&failed).await.unwrap();
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now + chrono::Duration::seconds(31))
                    .await
                    .unwrap(),
                AutomaticRetry::Scheduled {
                    attempt: 1,
                    delay_seconds: 30
                },
                "approved target change {change} must get a fresh budget"
            );
        }
    }

    #[tokio::test]
    async fn refreshed_claim_metadata_does_not_reset_the_retry_budget() {
        let directory = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(directory.path().to_str().unwrap(), KEY)
            .await
            .unwrap();
        let mut failed = failed_state();
        let mut config = config();
        let mut now = Utc::now();
        db.set_deployment_state(&failed).await.unwrap();
        for (index, delay) in RETRY_DELAYS_SECONDS.into_iter().enumerate() {
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Scheduled {
                    attempt: index + 1,
                    delay_seconds: delay
                }
            );
            let mut refreshed = serde_json::to_value(&config).unwrap();
            refreshed["deploymentToken"] = format!("refreshed-{index}").into();
            refreshed["managementConfig"] = serde_json::json!({"platform": "kubernetes"});
            refreshed["monitoring"] = serde_json::json!({
                "logsEndpoint": "http://localhost/v1/logs",
                "logsAuthHeader": format!("authorization=Bearer refreshed-{index}")
            });
            refreshed["domainMetadata"] = serde_json::json!({
                "baseDomain": "example.test", "publicSubdomain": format!("renewed-{index}"),
                "hostedZoneId": "local", "resources": {}
            });
            refreshed["environmentVariables"]["createdAt"] = now.to_rfc3339().into();
            config = serde_json::from_value(refreshed).unwrap();
            failed.target_release.as_mut().unwrap().description = Some(format!("metadata-{index}"));
            db.set_deployment_state(&failed).await.unwrap();
            now += chrono::Duration::seconds(delay);
            assert_eq!(
                db.prepare_automatic_retry(&failed, &config, now)
                    .await
                    .unwrap(),
                AutomaticRetry::Requested { attempt: index + 1 },
                "new claim metadata must consume the existing budget"
            );
            db.set_deployment_state(&failed).await.unwrap();
        }
        config.environment_variables.created_at = "another-claim".into();
        assert_eq!(
            db.prepare_automatic_retry(&failed, &config, now + chrono::Duration::days(1))
                .await
                .unwrap(),
            AutomaticRetry::Idle
        );
    }

    #[tokio::test]
    async fn setup_deletion_health_and_success_states_do_not_request_automatic_retries() {
        let directory = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(directory.path().to_str().unwrap(), KEY)
            .await
            .unwrap();
        for status in [
            DeploymentStatus::PreflightsFailed,
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::TeardownFailed,
            DeploymentStatus::RefreshFailed,
            DeploymentStatus::Running,
        ] {
            let mut state = failed_state();
            state.status = status;
            db.set_deployment_state(&state).await.unwrap();
            assert_eq!(
                db.prepare_automatic_retry(&state, &config(), Utc::now())
                    .await
                    .unwrap(),
                AutomaticRetry::Idle
            );
            assert!(
                !db.get_deployment_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .retry_requested
            );
        }
    }
}

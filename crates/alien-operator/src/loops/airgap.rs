//! Air-gapped delivery: the same targets sync would deliver, carried in by
//! hand.
//!
//! `alien-deploy sync` writes a bundle's target into a Secret in the
//! Operator's namespace. This loop reads it and hands a newer target to the
//! same code path sync uses. A second loop writes the deployment's state into
//! a status Secret that `alien-deploy sync` reads into the site's report.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::db::AirgapReceipt;
use alien_core::sync::TargetDeployment;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_k8s_clients::{
    ErrorData as KubernetesErrorData, KubernetesClient, KubernetesClientConfig,
    KubernetesClientConfigExt,
};
use k8s_openapi::{
    api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::ObjectMeta, ByteString,
};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::{
    error::{ErrorData, Result},
    OperatorState,
};

/// Key holding the target JSON in the target Secret.
pub const TARGET_KEY: &str = "target.json";
/// Key holding the bundle sequence (higher wins) in the target Secret.
pub const SEQUENCE_KEY: &str = "sequence";
/// A new value identifies an explicit invocation of site sync.
pub const ATTEMPT_KEY: &str = "attempt";
/// Key holding the deployment state JSON in the status Secret.
pub const STATUS_KEY: &str = "status.json";
/// Key holding the token that authorizes telemetry export, in the status
/// Secret.
pub const EXPORT_TOKEN_KEY: &str = "export-token";
/// Key holding the highest telemetry batch ID the vendor has received, in the
/// target Secret. Taken from the signed bundle manifest.
pub const TELEMETRY_ACK_KEY: &str = "telemetry-ack";

/// Telemetry kept for export before the oldest batches are dropped.
const DEFAULT_TELEMETRY_BUFFER_BYTES: u64 = 512 * 1024 * 1024;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const STATUS_INTERVAL: Duration = Duration::from_secs(10);

/// Apply targets from `secret_name` as bundles arrive.
pub async fn run_airgap_target_loop(state: Arc<OperatorState>, secret_name: String) {
    info!(secret = %secret_name, "Air-gapped: waiting for targets from bundles");
    loop {
        if let Err(e) = apply_pending_target(&state, &secret_name).await {
            warn!(error = %e, "Air-gapped: failed to apply bundle target");
        }
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    }
}

/// Write the deployment state to `secret_name` for export, and keep the
/// telemetry buffer within its limit.
pub async fn run_airgap_status_loop(
    state: Arc<OperatorState>,
    secret_name: String,
    export_token: String,
) {
    let buffer_bytes = match std::env::var("AIRGAP_TELEMETRY_BUFFER_BYTES") {
        Ok(value) => match value.parse() {
            Ok(bytes) => bytes,
            Err(_) => {
                error!(value, "AIRGAP_TELEMETRY_BUFFER_BYTES must be a number of bytes; status export disabled");
                return;
            }
        },
        Err(_) => DEFAULT_TELEMETRY_BUFFER_BYTES,
    };
    loop {
        match state.db.prune_telemetry(buffer_bytes).await {
            Ok(0) => {}
            Ok(dropped) => warn!(
                dropped,
                buffer_bytes, "Air-gapped: telemetry buffer full, dropped the oldest batches"
            ),
            Err(e) => warn!(error = %e, "Air-gapped: failed to prune telemetry"),
        }
        if let Err(e) = write_status(&state, &secret_name, &export_token).await {
            warn!(error = %e, "Air-gapped: failed to write status");
        }
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = tokio::time::sleep(STATUS_INTERVAL) => {}
        }
    }
}

async fn apply_pending_target(state: &OperatorState, secret_name: &str) -> Result<()> {
    let (client, namespace) = client(state).await?;
    let secret = match client.get_secret(&namespace, secret_name).await {
        Ok(secret) => secret,
        Err(e)
            if matches!(
                e.error.as_ref(),
                Some(KubernetesErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(())
        }
        Err(e) => {
            return Err(e).context(ErrorData::ConfigurationError {
                message: format!("reading Secret {secret_name}"),
            })
        }
    };
    apply_target_data(state, secret_name, secret.data.unwrap_or_default()).await
}

async fn apply_target_data(
    state: &OperatorState,
    secret_name: &str,
    data: BTreeMap<String, ByteString>,
) -> Result<()> {
    // The vendor has received telemetry up to here: free it. Independent of
    // whether this bundle's target is new.
    if let Some(value) = data.get(TELEMETRY_ACK_KEY) {
        let ack: i64 = std::str::from_utf8(&value.0)
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Secret {secret_name} has a non-numeric '{TELEMETRY_ACK_KEY}'"
                    ),
                })
            })?;
        let freed = state.db.delete_telemetry_through(ack).await?;
        if freed > 0 {
            info!(
                freed,
                through = ack,
                "Air-gapped: freed telemetry the vendor received"
            );
        }
    }
    let sequence: u64 = data
        .get(SEQUENCE_KEY)
        .and_then(|value| std::str::from_utf8(&value.0).ok())
        .and_then(|value| value.trim().parse().ok())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Secret {secret_name} has no numeric '{SEQUENCE_KEY}'"),
            })
        })?;
    let saved_sequence = state.db.get_airgap_sequence().await?;
    if saved_sequence.is_some_and(|saved| saved > sequence) {
        return Ok(());
    }
    let target: TargetDeployment = serde_json::from_slice(
        &data
            .get(TARGET_KEY)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Secret {secret_name} has no '{TARGET_KEY}'"),
                })
            })?
            .0,
    )
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: format!("parsing the target in Secret {secret_name}"),
    })?;

    let attempt = data
        .get(ATTEMPT_KEY)
        .and_then(|value| std::str::from_utf8(&value.0).ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let receipt = AirgapReceipt {
        sequence,
        target_hash: target_hash(&target)?,
        attempt,
    };
    if saved_sequence == Some(sequence) {
        let previous = state.db.get_airgap_receipt().await?;
        if let Some(previous) = &previous {
            if previous.target_hash != receipt.target_hash {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "A different target already used this air-gapped sequence".into(),
                }));
            }
            if previous.attempt == receipt.attempt {
                return Ok(());
            }
        }
        // Old CLIs have no explicit attempt marker and keep their existing behavior.
        if receipt.attempt.is_none() {
            return Ok(());
        }
        let Some(current) = state.db.get_deployment_state().await? else {
            return Ok(());
        };
        if previous.is_none() {
            // Upgrade from an Operator that stored only the sequence. A failed
            // target retains its full release and config, so compare both before retrying.
            let stored_config = state.db.get_deployment_config().await?;
            let stored_release = current
                .target_release
                .as_ref()
                .or(current.current_release.as_ref());
            if stored_config.is_none() && current.status == alien_core::DeploymentStatus::Running {
                // Completed legacy targets discarded their config. Keep the old
                // no-op behavior without trusting an unverifiable replacement.
                return Ok(());
            }
            let matches = match (stored_release, stored_config) {
                (Some(release), Some(config)) => {
                    target_hash(&TargetDeployment {
                        release_info: release.clone(),
                        config,
                    })? == receipt.target_hash
                }
                _ => false,
            };
            if !matches {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: "The saved air-gapped target cannot be verified for retry; use a newer signed bundle".into(),
                }));
            }
        }
        state
            .db
            .request_airgap_retry(&current, previous.as_ref(), &receipt)
            .await?;
        return Ok(());
    }

    info!(
        sequence,
        release_id = target.release_info.release_id.as_deref().unwrap_or("-"),
        "Air-gapped: applying bundle target"
    );
    // First bundle: seed the resource prefix the chart named its service
    // accounts with, as registration does for connected Operators.
    if state.db.get_deployment_state().await?.is_none() {
        if let Some(prefix) = &state.config.resource_prefix {
            state
                .db
                .set_deployment_state(&alien_core::DeploymentState {
                    platform: state.config.platform,
                    status: alien_core::DeploymentStatus::Pending,
                    current_release: None,
                    target_release: None,
                    stack_state: Some(alien_core::StackState::with_resource_prefix(
                        state.config.platform,
                        prefix.clone(),
                    )),
                    error: None,
                    environment_info: None,
                    runtime_metadata: None,
                    retry_requested: false,
                    protocol_version: alien_core::deployment_protocol_version_for_platform(
                        state.config.platform,
                        state.config.base_platform,
                    ),
                })
                .await?;
        }
    }
    crate::loops::sync::accept_target(state, &target, false).await?;
    state.db.record_airgap_target(&receipt).await?;
    Ok(())
}

fn target_hash(target: &TargetDeployment) -> Result<String> {
    let mut value =
        serde_json::to_value(target)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Encoding the air-gapped target".into(),
            })?;
    value.sort_all_objects();
    let bytes =
        serde_json::to_vec(&value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Encoding the air-gapped target".into(),
            })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Allow one minute per resource, including sequential dependency checks.
/// A deadline leaves the last checkpoint intact; only an observed error fails health.
async fn bounded_health_refresh(
    resource_count: usize,
    refresh: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    let timeout = Duration::from_secs(60).saturating_mul(resource_count.max(1) as u32);
    match tokio::time::timeout(timeout, refresh).await {
        Ok(result) => result,
        Err(_) => {
            warn!(
                resource_count,
                "Health refresh deadline reached; exporting last checkpoint"
            );
            Ok(())
        }
    }
}

async fn write_status(state: &OperatorState, secret_name: &str, export_token: &str) -> Result<()> {
    let resource_count = state
        .db
        .get_deployment_state()
        .await?
        .and_then(|deployment| deployment.stack_state)
        .map_or(0, |stack| stack.resources.len());
    let refresh = bounded_health_refresh(
        resource_count,
        super::deployment::refresh_airgap_deployment(state),
    )
    .await;
    let (deployment_state, sequence, attempt) = state.db.airgap_status_snapshot().await?;
    let Some(mut deployment_state) = deployment_state else {
        return Ok(());
    };
    if let Err(error) = refresh {
        if matches!(
            deployment_state.status,
            alien_core::DeploymentStatus::Running | alien_core::DeploymentStatus::RefreshFailed
        ) {
            deployment_state.status = alien_core::DeploymentStatus::RefreshFailed;
            deployment_state.error = Some(error.into_generic());
        }
    }
    let mut status = exported_status(&state.db, deployment_state, sequence).await?;
    if let Some(attempt) = attempt {
        status["attempt"] = attempt.into();
    }
    let bytes =
        serde_json::to_vec(&status)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "serializing air-gapped status".to_string(),
            })?;

    let (client, namespace) = client(state).await?;
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(secret_name.to_string()),
            labels: Some(BTreeMap::from([(
                "app.kubernetes.io/managed-by".into(),
                "alien-deploy-sync".into(),
            )])),
            ..Default::default()
        },
        data: Some(BTreeMap::from([
            (STATUS_KEY.to_string(), ByteString(bytes)),
            (
                EXPORT_TOKEN_KEY.to_string(),
                ByteString(export_token.as_bytes().to_vec()),
            ),
        ])),
        ..Default::default()
    };
    match client.get_secret(&namespace, secret_name).await {
        Ok(existing) => {
            let mut updated = secret;
            updated.metadata = existing.metadata;
            client
                .update_secret(&namespace, secret_name, &updated)
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!("updating Secret {secret_name}"),
                })?;
        }
        Err(e)
            if matches!(
                e.error.as_ref(),
                Some(KubernetesErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            client.create_secret(&namespace, &secret).await.context(
                ErrorData::ConfigurationError {
                    message: format!("creating Secret {secret_name}"),
                },
            )?;
        }
        Err(e) => {
            return Err(e).context(ErrorData::ConfigurationError {
                message: format!("reading Secret {secret_name}"),
            })
        }
    }
    Ok(())
}

async fn exported_status(
    db: &crate::db::OperatorDb,
    deployment_state: alien_core::DeploymentState,
    sequence: Option<u64>,
) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "deploymentId": db.get_deployment_id().await?,
        "writtenAt": chrono::Utc::now().to_rfc3339(),
        "sequence": sequence,
        "state": deployment_state,
        "heartbeats": db.get_pending_heartbeats().await?,
    }))
}

async fn client(state: &OperatorState) -> Result<(KubernetesClient, String)> {
    let namespace = state.config.namespace.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "air-gapped mode needs the Operator's namespace".to_string(),
        })
    })?;
    let config =
        KubernetesClientConfig::try_incluster()
            .await
            .context(ErrorData::ConfigurationError {
                message: "loading in-cluster credentials".to_string(),
            })?;
    let client = KubernetesClient::new(config)
        .await
        .context(ErrorData::ConfigurationError {
            message: "creating Kubernetes client".to_string(),
        })?;
    Ok((client, namespace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::OperatorConfig, db::OperatorDb};
    use alien_core::{
        DeploymentConfig, DeploymentStatus, EnvironmentVariablesSnapshot, Platform, ReleaseInfo,
        Stack,
    };
    use tokio_util::sync::CancellationToken;

    fn retry_target() -> TargetDeployment {
        TargetDeployment {
            release_info: ReleaseInfo {
                release_id: Some("release".into()),
                version: None,
                description: None,
                stack: Stack::new("site".into()).build(),
            },
            config: DeploymentConfig::builder()
                .stack_settings(Default::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .allow_frozen_changes(false)
                .external_bindings(Default::default())
                .build(),
        }
    }

    fn retry_data(
        target: &TargetDeployment,
        sequence: u64,
        attempt: &str,
    ) -> BTreeMap<String, ByteString> {
        BTreeMap::from([
            (
                TARGET_KEY.into(),
                ByteString(serde_json::to_vec(target).unwrap()),
            ),
            (
                SEQUENCE_KEY.into(),
                ByteString(sequence.to_string().into_bytes()),
            ),
            (ATTEMPT_KEY.into(), ByteString(attempt.as_bytes().to_vec())),
        ])
    }

    async fn retry_state(directory: &tempfile::TempDir) -> OperatorState {
        let key = "11".repeat(32);
        OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Kubernetes)
                .data_dir(directory.path().to_str().unwrap())
                .encryption_key(&key)
                .build(),
            db: Arc::new(
                OperatorDb::new(directory.path().to_str().unwrap(), &key)
                    .await
                    .unwrap(),
            ),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn legacy_failed_target_is_verified_before_retry_and_completed_target_stays_unchanged() {
        for failed in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let state = retry_state(&directory).await;
            let target = retry_target();
            crate::loops::sync::accept_target(&state, &target, false)
                .await
                .unwrap();
            state.db.set_airgap_sequence(7).await.unwrap();
            let mut deployment = state.db.get_deployment_state().await.unwrap().unwrap();
            if failed {
                deployment.status = DeploymentStatus::UpdateFailed;
            } else {
                deployment.status = DeploymentStatus::Running;
                deployment.current_release = deployment.target_release.take();
                state.db.clear_deployment_config().await.unwrap();
            }
            state.db.set_deployment_state(&deployment).await.unwrap();
            if failed {
                let mut conflict = target.clone();
                conflict.config.allow_frozen_changes = true;
                assert!(
                    apply_target_data(&state, "target", retry_data(&conflict, 7, "conflict"))
                        .await
                        .is_err()
                );
                assert!(state.db.get_airgap_receipt().await.unwrap().is_none());
            }
            apply_target_data(&state, "target", retry_data(&target, 7, "retry"))
                .await
                .unwrap();
            let (observed, sequence, attempt) = state.db.airgap_status_snapshot().await.unwrap();
            assert_eq!(observed.unwrap().retry_requested, failed);
            assert_eq!(sequence, Some(7));
            assert_eq!(
                attempt.as_deref(),
                if failed { Some("retry") } else { None }
            );
        }
    }

    #[tokio::test]
    async fn explicit_same_bundle_attempt_retries_once_and_rejects_conflicting_targets() {
        for failure in [
            DeploymentStatus::PreflightsFailed,
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::RefreshFailed,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let state = retry_state(&directory).await;
            let target = retry_target();
            apply_target_data(&state, "target", retry_data(&target, 7, "first"))
                .await
                .unwrap();
            let mut failed = state.db.get_deployment_state().await.unwrap().unwrap();
            failed.status = failure;
            state.db.set_deployment_state(&failed).await.unwrap();
            apply_target_data(&state, "target", retry_data(&target, 7, "first"))
                .await
                .unwrap();
            assert!(
                !state
                    .db
                    .get_deployment_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .retry_requested
            );
            apply_target_data(&state, "target", retry_data(&target, 7, "retry"))
                .await
                .unwrap();
            let (requested, sequence, attempt) = state.db.airgap_status_snapshot().await.unwrap();
            assert!(requested.unwrap().retry_requested);
            assert_eq!(sequence, Some(7));
            assert_eq!(attempt.as_deref(), Some("retry"));
            state.db.set_deployment_state(&failed).await.unwrap();
            apply_target_data(&state, "target", retry_data(&target, 7, "retry"))
                .await
                .unwrap();
            assert!(
                !state
                    .db
                    .get_deployment_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .retry_requested
            );
            let mut conflicting = target.clone();
            conflicting.config.allow_frozen_changes = true;
            assert!(
                apply_target_data(&state, "target", retry_data(&conflicting, 7, "conflict"))
                    .await
                    .is_err()
            );
            apply_target_data(&state, "target", retry_data(&target, 6, "older"))
                .await
                .unwrap();
            assert_eq!(
                state
                    .db
                    .get_airgap_receipt()
                    .await
                    .unwrap()
                    .unwrap()
                    .attempt
                    .as_deref(),
                Some("retry")
            );
            assert!(
                !state
                    .db
                    .get_deployment_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .retry_requested
            );
            if failure == DeploymentStatus::UpdateFailed {
                apply_target_data(&state, "target", retry_data(&conflicting, 8, "newer"))
                    .await
                    .unwrap();
                assert_eq!(state.db.get_airgap_sequence().await.unwrap(), Some(8));
                assert_eq!(
                    state
                        .db
                        .get_deployment_state()
                        .await
                        .unwrap()
                        .unwrap()
                        .status,
                    DeploymentStatus::UpdatePending
                );
            }
        }
    }

    #[tokio::test]
    async fn exported_status_carries_persisted_heartbeats() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::OperatorDb::new(
            dir.path().to_str().unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .await
        .unwrap();
        let heartbeat = serde_json::json!({
         "deploymentId": "dep_1", "resourceId": "web", "resourceType": "container",
         "controllerPlatform": "kubernetes", "backend": "kubernetes", "observedAt": "2026-01-01T00:00:00Z",
         "data": {"resourceType": "container", "data": {"backend": "kubernetes", "namespace": "site", "name": "web",
         "workloadKind": "deployment", "status": {"health": "healthy", "lifecycle": "running", "stale": false, "partial": false, "collectionIssues": []},
         "replicas": {"desired": 7, "current": 7, "ready": 7, "available": 7, "updated": 7}, "pods": [], "events": []}}, "raw": []
        });
        db.set_pending_heartbeats(&[serde_json::from_value(heartbeat.clone()).unwrap()])
            .await
            .unwrap();
        let state = serde_json::from_value(
            serde_json::json!({"status": "running", "platform": "kubernetes", "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION}),
        )
        .unwrap();
        let exported = exported_status(&db, state, Some(3)).await.unwrap();
        let observed: Vec<alien_core::ResourceHeartbeat> =
            serde_json::from_value(exported["heartbeats"].clone()).unwrap();
        assert_eq!(observed, vec![serde_json::from_value(heartbeat).unwrap()]);
        assert_eq!(exported["sequence"], 3);
        assert!(exported["writtenAt"]
            .as_str()
            .unwrap()
            .parse::<chrono::DateTime<chrono::Utc>>()
            .is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn healthy_multi_resource_refresh_can_exceed_thirty_seconds() {
        let started = tokio::time::Instant::now();
        let mut checked = 0;
        bounded_health_refresh(9, async {
            for _ in 0..9 {
                tokio::time::sleep(Duration::from_secs(10)).await;
                checked += 1;
            }
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(checked, 9);
        assert_eq!(started.elapsed(), Duration::from_secs(90));
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_deadline_does_not_invent_a_deployment_failure() {
        let started = tokio::time::Instant::now();
        bounded_health_refresh(9, std::future::pending())
            .await
            .unwrap();
        assert_eq!(started.elapsed(), Duration::from_secs(540));
        let error = bounded_health_refresh(9, async {
            Err(AlienError::new(ErrorData::ConfigurationError {
                message: "observed resource failure".into(),
            }))
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("observed resource failure"));
    }
}

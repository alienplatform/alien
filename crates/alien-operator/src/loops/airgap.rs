//! Air-gapped delivery: the same targets sync would deliver, carried in by
//! hand.
//!
//! `alien-deploy sync` writes a bundle's target into a Secret in the
//! Operator's namespace. This loop reads it and hands a newer target to the
//! same code path sync uses. A second loop writes the deployment's state into
//! a status Secret that `alien-deploy sync` reads into the site's report.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use alien_core::sync::TargetDeployment;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_k8s_clients::{
    ErrorData as KubernetesErrorData, KubernetesClient, KubernetesClientConfig,
    KubernetesClientConfigExt,
};
use k8s_openapi::{
    api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::ObjectMeta, ByteString,
};
use tracing::{error, info, warn};

use crate::{
    error::{ErrorData, Result},
    OperatorState,
};

/// Key holding the target JSON in the target Secret.
pub const TARGET_KEY: &str = "target.json";
/// Key holding the bundle sequence (higher wins) in the target Secret.
pub const SEQUENCE_KEY: &str = "sequence";
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
    let data = secret.data.unwrap_or_default();
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
    if state.db.get_airgap_sequence().await?.unwrap_or(0) >= sequence {
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
    state.db.set_airgap_sequence(sequence).await?;
    Ok(())
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
    let sequence = state.db.get_airgap_sequence().await?;
    let Some(mut deployment_state) = state.db.get_deployment_state().await? else {
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
    if state.db.get_airgap_sequence().await? != sequence {
        return Ok(());
    }
    let status = serde_json::json!({
        "deploymentId": state.db.get_deployment_id().await?,
        "writtenAt": chrono::Utc::now().to_rfc3339(),
        "sequence": sequence,
        "state": deployment_state,
    });
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

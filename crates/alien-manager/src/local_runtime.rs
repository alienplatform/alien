//! Durable desired runtime state for local deployments. Deployment leases serialize
//! application with reconciliation; KV versions prevent an old completion from
//! overwriting a newer stop or resume request.

use alien_bindings::traits::{Kv, PutCondition, PutOptions};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeStatus {
    /// Whether processes should be running.
    pub desired_running: bool,
    /// Whether the last completed lifecycle operation left processes running.
    pub observed_running: bool,
}

impl Default for LocalRuntimeStatus {
    fn default() -> Self {
        Self {
            desired_running: true,
            observed_running: true,
        }
    }
}

pub(crate) fn key(id: &str) -> String {
    format!("local-runtime:{id}")
}

pub(crate) async fn read(
    kv: &dyn Kv,
    id: &str,
) -> Result<Option<(LocalRuntimeStatus, String)>, AlienError> {
    let Some(entry) = kv.get(&key(id)).await.context(GenericError {
        message: "Read local runtime desired state".to_string(),
    })?
    else {
        return Ok(None);
    };
    let state = serde_json::from_slice(&entry.value)
        .into_alien_error()
        .context(GenericError {
            message: "Decode local runtime desired state".to_string(),
        })?;
    Ok(Some((state, entry.version)))
}

pub(crate) async fn write(
    kv: &dyn Kv,
    id: &str,
    state: &LocalRuntimeStatus,
    version: Option<String>,
) -> Result<bool, AlienError> {
    let bytes = serde_json::to_vec(state)
        .into_alien_error()
        .context(GenericError {
            message: "Encode local runtime desired state".to_string(),
        })?;
    kv.put(
        &key(id),
        bytes,
        Some(PutOptions {
            condition: version.map(PutCondition::Version).unwrap_or_default(),
            ..Default::default()
        }),
    )
    .await
    .context(GenericError {
        message: "Persist local runtime desired state".to_string(),
    })
}

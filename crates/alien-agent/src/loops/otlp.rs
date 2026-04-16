//! Telemetry loop - pushes buffered telemetry to manager

use crate::AgentState;
use alien_error::{Context, IntoAlienError};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

/// Run the telemetry loop
///
/// This loop:
/// 1. Gets buffered telemetry from local database
/// 2. Pushes to manager via raw HTTP (protobuf data)
/// 3. Deletes pushed telemetry on success
pub async fn run_telemetry_loop(state: Arc<AgentState>) {
    let interval = Duration::from_secs(state.config.telemetry_interval_seconds);

    let sync_config = match &state.config.sync {
        Some(config) => config,
        None => {
            info!("Sync configuration not provided, telemetry loop disabled");
            return;
        }
    };

    // Create HTTP client with authentication
    let client = match create_authenticated_client(&sync_config.token) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create authenticated HTTP client");
            return;
        }
    };

    info!(
        interval_seconds = state.config.telemetry_interval_seconds,
        "Starting telemetry loop"
    );

    loop {
        match push_telemetry(&state, &client, &sync_config.url).await {
            Ok(count) => {
                if count > 0 {
                    debug!(pushed = count, "Pushed telemetry to manager");
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to push telemetry");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Telemetry loop shutting down");
                return;
            }
        }
    }
}

/// Create an authenticated HTTP client
fn create_authenticated_client(token: &str) -> crate::error::Result<reqwest::Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    let auth_value = format!("Bearer {}", token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Invalid auth token".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-agent"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to build HTTP client".to_string(),
        })
}

async fn push_telemetry(
    state: &AgentState,
    client: &reqwest::Client,
    base_url: &url::Url,
) -> crate::error::Result<usize> {
    // Get pending telemetry
    let pending = state.db.get_pending_telemetry(100).await?;

    if pending.is_empty() {
        return Ok(0);
    }

    let mut pushed_ids = Vec::new();

    for (id, telemetry_type, data) in &pending {
        let endpoint_path = match telemetry_type.as_str() {
            "logs" => "v1/logs",
            "metrics" => "v1/metrics",
            "traces" => "v1/traces",
            _ => {
                debug!(id = id, telemetry_type = %telemetry_type, "Unknown telemetry type");
                continue;
            }
        };

        let endpoint = match base_url.join(endpoint_path) {
            Ok(url) => url,
            Err(e) => {
                debug!(id = id, error = %e, "Failed to construct endpoint URL");
                continue;
            }
        };

        let result = client
            .post(endpoint)
            .header("Content-Type", "application/x-protobuf")
            .body(data.clone())
            .send()
            .await;

        match result {
            Ok(response) if response.status().is_success() => {
                match response.json::<TelemetryResponse>().await {
                    Ok(resp) if resp.accepted => {
                        pushed_ids.push(*id);
                    }
                    Ok(_) => {
                        debug!(id = id, "Telemetry not accepted, will retry");
                    }
                    Err(e) => {
                        debug!(id = id, error = %e, "Failed to parse telemetry response");
                    }
                }
            }
            Ok(response) => {
                debug!(
                    id = id,
                    status = response.status().as_u16(),
                    "Telemetry request failed, will retry"
                );
            }
            Err(e) => {
                debug!(id = id, error = %e, "Failed to push telemetry entry, will retry");
            }
        }
    }

    // Delete successfully pushed telemetry
    if !pushed_ids.is_empty() {
        state.db.delete_telemetry(&pushed_ids).await?;
    }

    Ok(pushed_ids.len())
}

/// Response from telemetry endpoints
#[derive(serde::Deserialize)]
struct TelemetryResponse {
    accepted: bool,
}

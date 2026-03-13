//! Horizon API client for container orchestration.
//!
//! This module provides a thin wrapper around the `horizon-client-sdk`
//! for use by container resource controllers.

pub use horizon_client_sdk::types::*;
pub use horizon_client_sdk::Client as HorizonClient;

use alien_core::CapacityGroup as AlienCapacityGroup;
use alien_core::ContainerStatus as AlienContainerStatus;
use alien_error::AlienError;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

use std::num::NonZero;

use crate::error::{ErrorData, Result};

/// Convert a Horizon SDK `ContainerStatus` to the canonical alien-core `ContainerStatus`.
///
/// This is an exhaustive match — adding a new variant to the Horizon SDK enum will cause a
/// compile error here, forcing an explicit decision on how to map the new status.
pub fn horizon_container_status_to_alien(
    status: horizon_client_sdk::types::ContainerStatus,
) -> AlienContainerStatus {
    use horizon_client_sdk::types::ContainerStatus as HorizonStatus;
    match status {
        HorizonStatus::Pending => AlienContainerStatus::Pending,
        HorizonStatus::Running => AlienContainerStatus::Running,
        HorizonStatus::Stopped => AlienContainerStatus::Stopped,
        HorizonStatus::Failing => AlienContainerStatus::Failing,
    }
}

/// Create a Horizon client authenticated with a management token.
pub fn create_horizon_client(
    api_url: &str,
    management_token: &str,
) -> std::result::Result<HorizonClient, reqwest::header::InvalidHeaderValue> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", management_token))?,
    );

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client");

    Ok(HorizonClient::new_with_client(api_url, http_client))
}

/// Convert alien-core capacity groups to Horizon SDK capacity groups.
///
/// Each group must have a `profile` set (resolved during preflights/mutations).
/// Returns an error if any group fails conversion — never silently drops groups.
pub fn to_horizon_capacity_groups(
    groups: &[AlienCapacityGroup],
    resource_id: &str,
) -> Result<Vec<horizon_client_sdk::types::CapacityGroup>> {
    groups
        .iter()
        .map(|g| {
            let profile = g.profile.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Capacity group '{}' has no profile (not resolved by preflights?)",
                        g.group_id
                    ),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            let cpu: f64 = profile.cpu.parse().map_err(|_| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Capacity group '{}': invalid CPU value '{}'",
                        g.group_id, profile.cpu
                    ),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            let ephemeral_storage_bytes = NonZero::new(profile.ephemeral_storage_bytes)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Capacity group '{}': ephemeral_storage_bytes must be > 0",
                            g.group_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;
            let max_size = NonZero::new(g.max_size as u64).ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Capacity group '{}': max_size must be > 0", g.group_id),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            let group_id = g.group_id.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Capacity group '{}': invalid group_id: {}", g.group_id, e),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

            Ok(horizon_client_sdk::types::CapacityGroup {
                group_id,
                profile: horizon_client_sdk::types::MachineProfile {
                    cpu,
                    memory_bytes: profile.memory_bytes as i64,
                    ephemeral_storage_bytes,
                    gpu: None,
                },
                min_size: g.min_size as u64,
                max_size,
            })
        })
        .collect()
}

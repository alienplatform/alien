//! Managers service - fetches managers and DeepStore tokens
//!
//! Used by the Logs view to get available log sources in platform mode.

use alien_platform_api::Client;
use tracing::debug;

use crate::tui::state::managers::{ManagerItem, ManagerStatus};

/// Service for fetching managers
#[derive(Clone)]
pub struct ManagersService {
    sdk: Client,
}

impl ManagersService {
    pub fn new(sdk: Client) -> Self {
        Self { sdk }
    }

    /// List all managers for the current workspace
    pub async fn list(&self) -> Result<Vec<ManagerItem>, String> {
        debug!("Fetching managers");

        let response = self
            .sdk
            .list_managers()
            .send()
            .await
            .map_err(|e| format!("Failed to fetch managers: {}", e))?;

        // The SDK returns a Vec directly
        let items: Vec<ManagerItem> = response
            .into_inner()
            .into_iter()
            .map(|am| ManagerItem {
                id: am.id.to_string(),
                name: am.name,
                status: ManagerStatus::from_str(&am.status.to_string()),
                has_deepstore: am.url.is_some(),
                url: am.url,
            })
            .collect();

        debug!(count = items.len(), "Fetched managers");
        Ok(items)
    }

    /// Get a DeepStore token for querying logs
    ///
    /// This token is used to authenticate with the manager's DeepStore proxy.
    /// The token includes scopes that determine which logs the user can access.
    pub async fn get_deepstore_token(
        &self,
        manager_id: &str,
        project_id: &str,
    ) -> Result<DeepstoreCredentials, String> {
        debug!(manager_id = %manager_id, "Fetching DeepStore token");

        let project = project_id
            .parse()
            .map_err(|e| format!("Invalid project ID: {}", e))?;

        let response = self
            .sdk
            .generate_deepstore_token()
            .id(manager_id)
            .body(alien_platform_api::types::GenerateDeepstoreTokenRequest {
                project: Some(project),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to get DeepStore token: {}", e))?;

        let body = response.into_inner();

        Ok(DeepstoreCredentials {
            token: body.access_token,
            database_id: body.database_id,
        })
    }
}

/// DeepStore credentials for log queries
#[derive(Debug, Clone)]
pub struct DeepstoreCredentials {
    /// JWT token for authenticating with DeepStore
    pub token: String,
    /// Database ID for log storage
    pub database_id: String,
}

//! Deployment groups service - SDK calls for deployment group operations

use crate::tui::state::DeploymentGroupItem;
use alien_platform_api::{Client, SdkResultExt};

/// Service for deployment group SDK calls
#[derive(Clone)]
pub struct DeploymentGroupsService {
    sdk: Client,
    /// Project ID to filter by (platform mode)
    project_id: Option<String>,
}

impl DeploymentGroupsService {
    pub fn new(sdk: Client, project_id: Option<String>) -> Self {
        Self { sdk, project_id }
    }

    /// List all deployment groups (filtered by project if set)
    pub async fn list(&self) -> Result<Vec<DeploymentGroupItem>, String> {
        let mut builder = self.sdk.list_deployment_groups();

        // Apply project filter if set
        if let Some(ref project_id) = self.project_id {
            builder = builder.project(project_id);
        }

        let result = builder.send().await.into_sdk_error();
        match result {
            Ok(response) => {
                let items = response
                    .into_inner()
                    .items
                    .into_iter()
                    .map(|g| DeploymentGroupItem {
                        id: g.id.to_string(),
                        name: g.name.to_string(),
                        max_deployments: g.max_deployments.get(),
                        created_at: g.created_at,
                    })
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to load deployment groups: {}", e)),
        }
    }
}

//! Releases service - SDK calls for release operations

use crate::tui::state::ReleaseItem;
use alien_platform_api::{Client, SdkResultExt};

/// Service for release SDK calls
#[derive(Clone)]
pub struct ReleasesService {
    sdk: Client,
    /// Project ID to filter by (platform mode)
    project_id: Option<String>,
}

impl ReleasesService {
    pub fn new(sdk: Client, project_id: Option<String>) -> Self {
        Self { sdk, project_id }
    }

    /// List all releases (filtered by project if set)
    pub async fn list(&self) -> Result<Vec<ReleaseItem>, String> {
        let mut builder = self.sdk.list_releases();

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
                    .map(|r| ReleaseItem {
                        id: r.id.to_string(),
                        project_id: r.project_id.to_string(),
                        created_at: r.created_at,
                    })
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to load releases: {}", e)),
        }
    }
}

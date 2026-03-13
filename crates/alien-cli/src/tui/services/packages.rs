//! Packages service - SDK calls for package operations

use crate::tui::state::packages::{PackageItem, PackageStatus};
use alien_platform_api::{types, Client, SdkResultExt};

/// Service for package SDK calls
#[derive(Clone)]
pub struct PackagesService {
    sdk: Client,
    /// Project ID to filter by (platform mode)
    project_id: Option<String>,
}

impl PackagesService {
    pub fn new(sdk: Client, project_id: Option<String>) -> Self {
        Self { sdk, project_id }
    }

    /// List all packages (filtered by project if set)
    pub async fn list(&self) -> Result<Vec<PackageItem>, String> {
        let mut builder = self.sdk.list_packages();

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
                    .map(|pkg| PackageItem {
                        id: pkg.id.to_string(),
                        type_display: format!("{:?}", pkg.type_),
                        version: pkg.version.to_string(),
                        status: status_from_api(pkg.status),
                        created_at: pkg.created_at,
                    })
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to load packages: {}", e)),
        }
    }
}

fn status_from_api(status: types::PackageStatus) -> PackageStatus {
    match status {
        types::PackageStatus::Pending => PackageStatus::Pending,
        types::PackageStatus::Building => PackageStatus::Building,
        types::PackageStatus::Ready => PackageStatus::Ready,
        types::PackageStatus::Failed => PackageStatus::Failed,
        types::PackageStatus::Canceled => PackageStatus::Canceled,
    }
}

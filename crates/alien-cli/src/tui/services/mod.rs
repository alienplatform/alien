//! Services layer - all SDK/API calls isolated here
//!
//! Services handle all external calls (SDK, API).
//! They transform API responses into state types that views can render.

pub mod commands;
pub mod deployment_groups;
pub mod deployments;
pub mod logs;
pub mod managers;
pub mod packages;
pub mod releases;

pub use commands::CommandsService;
pub use deployment_groups::DeploymentGroupsService;
pub use deployments::DeploymentsService;
pub use logs::LogsService;
pub use managers::{DeepstoreCredentials, ManagersService};
pub use packages::PackagesService;
pub use releases::ReleasesService;

use alien_platform_api::Client;

/// Aggregated services for the app
pub struct AppServices {
    pub deployments: DeploymentsService,
    pub managers: ManagersService,
    pub deployment_groups: DeploymentGroupsService,
    pub commands: CommandsService,
    pub releases: ReleasesService,
    pub packages: PackagesService,
    pub logs: Option<LogsService>,
}

impl AppServices {
    /// Create services with the given SDK client and optional project filter
    pub fn new(sdk: Client, project_id: Option<String>) -> Self {
        Self {
            deployments: DeploymentsService::new(sdk.clone(), project_id.clone()),
            managers: ManagersService::new(sdk.clone()),
            deployment_groups: DeploymentGroupsService::new(sdk.clone(), project_id.clone()),
            commands: CommandsService::new(sdk.clone()),
            releases: ReleasesService::new(sdk.clone(), project_id.clone()),
            packages: PackagesService::new(sdk, project_id),
            logs: None, // Initialized dynamically based on selected manager
        }
    }

    /// Set the logs service
    pub fn with_logs(mut self, logs: LogsService) -> Self {
        self.logs = Some(logs);
        self
    }
}

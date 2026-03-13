//! Deployment detail view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_deployment_detail_demo;
use alien_cli::tui::app::AppMode;
use alien_cli::tui::state::{DeploymentDetailState, DeploymentStatus};
use alien_cli::tui::views::deployment_detail_view;

#[derive(Subcommand, Debug, Clone)]
pub enum DeploymentDetailDemo {
    /// Running deployment with resources
    Running,
    /// Deployment with many resources (scrolling)
    #[command(name = "many-resources")]
    ManyResources,
    /// Deployment with mixed resource statuses
    #[command(name = "mixed-resources")]
    MixedResources,
    /// Failed deployment with error (press E to view)
    Failed,
}

impl DeploymentDetailDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Running => {
                let mut state = DeploymentDetailState::new(
                    "dpl_demo".to_string(),
                    "demo-deployment".to_string(),
                    DeploymentStatus::Running,
                );
                state.update_resources(mock_data::mock_resources());
                state.update_metadata(mock_data::mock_deployment_metadata());
                ("Deployment Detail - Running", state)
            }
            Self::ManyResources => {
                let mut state = DeploymentDetailState::new(
                    "dpl_demo".to_string(),
                    "demo-deployment".to_string(),
                    DeploymentStatus::Running,
                );
                state.update_resources(mock_data::mock_many_resources(50));
                state.update_metadata(mock_data::mock_deployment_metadata());
                ("Deployment Detail - Many Resources (50)", state)
            }
            Self::MixedResources => {
                let mut state = DeploymentDetailState::new(
                    "dpl_demo".to_string(),
                    "demo-deployment".to_string(),
                    DeploymentStatus::Running,
                );
                state.update_resources(mock_data::mock_resources_various_statuses());
                state.update_metadata(mock_data::mock_deployment_metadata());
                ("Deployment Detail - Mixed Resources", state)
            }
            Self::Failed => {
                let mut state = DeploymentDetailState::new(
                    "dpl_demo".to_string(),
                    "failed-deployment".to_string(),
                    DeploymentStatus::ProvisioningFailed,
                );
                state.update_resources(mock_data::mock_resources_various_statuses());
                state.update_metadata(mock_data::mock_deployment_metadata_with_error());
                ("Deployment Detail - Failed (Press E to view error)", state)
            }
        };

        run_deployment_detail_demo(
            title,
            state,
            |frame, area, state| deployment_detail_view::render(frame, area, state),
            |key, state| deployment_detail_view::handle_key(key, state, AppMode::Dev),
        )
    }
}

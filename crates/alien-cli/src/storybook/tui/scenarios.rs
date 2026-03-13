//! Predefined test scenarios for TUI views

use alien_cli::tui::state::ListState;

/// Common scenarios for list views
#[derive(Debug, Clone, Copy)]
pub enum ListScenario {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Error state
    Error,
    /// Single item
    SingleItem,
    /// Few items (5)
    FewItems,
    /// Many items (50)
    ManyItems,
    /// Huge list (500)
    HugeList,
}

/// Create a list state from a scenario
pub fn create_list_state<T, F>(scenario: ListScenario, generator: F) -> ListState<T>
where
    F: Fn(usize) -> Vec<T>,
{
    match scenario {
        ListScenario::Empty => ListState::new(),
        ListScenario::Loading => ListState::loading(),
        ListScenario::Error => ListState::with_error("Failed to load data: Connection timeout"),
        ListScenario::SingleItem => ListState::with_items(generator(1)),
        ListScenario::FewItems => ListState::with_items(generator(5)),
        ListScenario::ManyItems => ListState::with_items(generator(50)),
        ListScenario::HugeList => ListState::with_items(generator(500)),
    }
}

/// Deployment detail scenarios
#[derive(Debug, Clone, Copy)]
pub enum DeploymentDetailScenario {
    /// Starting up
    Starting,
    /// Building
    Building,
    /// Deploying resources
    Deploying,
    /// Running successfully
    Running,
    /// Rebuilding after code change
    Rebuilding,
    /// Failed during deployment
    Failed,
    /// Running with many logs
    RunningManyLogs,
    /// Running with various resource statuses
    RunningMixedResources,
}

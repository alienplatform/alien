//! Command (ARC) state types

use chrono::{DateTime, Utc};

// Re-export CommandState from alien-core
pub use alien_core::CommandState;

/// Command item for list display
#[derive(Debug, Clone)]
pub struct CommandItem {
    pub id: String,
    pub name: String,
    pub state: CommandState,
    pub deployment_id: String,
    pub deployment_name: Option<String>,
    pub deployment_group_id: Option<String>,
    pub deployment_group_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

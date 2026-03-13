//! Release state types

use chrono::{DateTime, Utc};

/// Release item for list display
#[derive(Debug, Clone)]
pub struct ReleaseItem {
    pub id: String,
    pub project_id: String,
    pub created_at: DateTime<Utc>,
}

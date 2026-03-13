//! Deployment group state types

use chrono::{DateTime, Utc};

/// Deployment group item for list display
#[derive(Debug, Clone)]
pub struct DeploymentGroupItem {
    pub id: String,
    pub name: String,
    pub max_deployments: u64,
    pub created_at: DateTime<Utc>,
}

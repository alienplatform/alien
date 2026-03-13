//! Package state types

use chrono::{DateTime, Utc};

/// Package status for display
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageStatus {
    Pending,
    Building,
    Ready,
    Failed,
    Canceled,
}

impl PackageStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            PackageStatus::Ready | PackageStatus::Failed | PackageStatus::Canceled
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, PackageStatus::Ready)
    }
}

/// Package item for list display
#[derive(Debug, Clone)]
pub struct PackageItem {
    pub id: String,
    /// Package type as display string (e.g. "Cli", "Cloudformation")
    pub type_display: String,
    pub version: String,
    pub status: PackageStatus,
    pub created_at: DateTime<Utc>,
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Represents a scheduled event trigger, typically from a cron job or timer.
///
/// This struct aims to provide a common representation for scheduled events
/// across different providers like AWS CloudWatch Events, Google Cloud Scheduler,
/// and Azure Timer Triggers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScheduledEvent {
    /// The timestamp when the event was scheduled or triggered.
    pub timestamp: DateTime<Utc>,
}

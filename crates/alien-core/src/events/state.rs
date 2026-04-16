use alien_error::AlienError;
use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Represents the state of an event
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum EventState {
    /// Event has no specific state (simple events)
    None,
    /// Event has started (for scoped events)
    Started,
    /// Event completed successfully
    Success,
    /// Event failed with an error
    Failed {
        /// Error details
        error: Option<AlienError>,
    },
}

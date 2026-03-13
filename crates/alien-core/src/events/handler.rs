use crate::events::{AlienEvent, EventState};
use crate::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Represents a change to an event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventChange {
    /// A new event was created
    #[serde(rename_all = "camelCase")]
    Created {
        /// Unique identifier for the event
        id: String,
        /// Parent event ID if this is a child event
        parent_id: Option<String>,
        /// Timestamp when the event was created
        created_at: DateTime<Utc>,
        /// The actual event data
        event: AlienEvent,
        /// Initial state of the event
        state: EventState,
    },

    /// An existing event was updated
    #[serde(rename_all = "camelCase")]
    Updated {
        /// Unique identifier for the event
        id: String,
        /// Timestamp when the event was updated
        updated_at: DateTime<Utc>,
        /// The new event data
        event: AlienEvent,
    },

    /// An event's state changed
    #[serde(rename_all = "camelCase")]
    StateChanged {
        /// Unique identifier for the event
        id: String,
        /// Timestamp when the state changed
        updated_at: DateTime<Utc>,
        /// The new state
        new_state: EventState,
    },
}

/// Trait for handling events
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Called when any event change occurs
    async fn on_event_change(&self, change: EventChange) -> Result<()>;
}

/// A no-op event handler for testing
pub struct NoOpEventHandler;

#[async_trait]
impl EventHandler for NoOpEventHandler {
    async fn on_event_change(&self, _change: EventChange) -> Result<()> {
        Ok(())
    }
}

use crate::events::{AlienEvent, EventBus, EventState};
use crate::Result;
use alien_error::{AlienError, AlienErrorData};
use serde::{Deserialize, Serialize};
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Handle to an emitted event that allows updating it
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct EventHandle {
    /// Unique identifier for the event
    pub id: String,
    /// Parent event ID if this is a child event
    pub parent_id: Option<String>,
    /// Whether this is a no-op handle (when no event bus is available)
    #[serde(skip)]
    pub is_noop: bool,
}

impl EventHandle {
    /// Create a new event handle
    pub fn new(id: String, parent_id: Option<String>) -> Self {
        Self {
            id,
            parent_id,
            is_noop: false,
        }
    }

    /// Create a no-op event handle (used when no event bus is available)
    pub fn noop() -> Self {
        Self {
            id: "noop".to_string(),
            parent_id: None,
            is_noop: true,
        }
    }

    /// Update the event with new data
    pub async fn update(&self, event: AlienEvent) -> Result<()> {
        if self.is_noop {
            return Ok(());
        }
        EventBus::update(&self.id, event).await
    }

    /// Mark the event as completed successfully
    pub async fn complete(&self) -> Result<()> {
        if self.is_noop {
            return Ok(());
        }
        EventBus::update_state(&self.id, EventState::Success).await
    }

    /// Mark the event as failed with an error
    pub async fn fail<E>(&self, error: AlienError<E>) -> Result<()>
    where
        E: AlienErrorData + Clone + std::fmt::Debug + Serialize,
    {
        if self.is_noop {
            return Ok(());
        }
        EventBus::update_state(
            &self.id,
            EventState::Failed {
                error: Some(error.into_generic()),
            },
        )
        .await
    }

    /// Create a child scope with this handle as parent
    pub async fn as_parent<F, Fut, T>(&self, f: F) -> T
    where
        F: FnOnce(&EventHandle) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        if self.is_noop {
            return f(self).await;
        }
        let parent_id = self.id.clone();
        EventBus::with_parent(Some(parent_id), f).await
    }
}

use crate::events::{AlienEvent, EventChange, EventHandle, EventHandler, EventState};
use crate::{ErrorData, Result};
use alien_error::Context;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

tokio::task_local! {
    /// Task-local event bus instance
    static EVENT_BUS: EventBus;
}

tokio::task_local! {
    /// Task-local parent event ID for automatic hierarchy
    static PARENT_EVENT_ID: Option<String>;
}

/// The event bus for managing events within a task context
pub struct EventBus {
    /// Registered event handlers
    handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new event bus with handlers
    pub fn with_handlers(handlers: Vec<Arc<dyn EventHandler>>) -> Self {
        Self {
            handlers: Arc::new(RwLock::new(handlers)),
        }
    }

    /// Run a function with this event bus as the task-local context
    pub async fn run<F, Fut, T>(&self, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        EVENT_BUS.scope(self.clone(), f()).await
    }

    /// Register an event handler to the current task-local event bus
    pub async fn register_handler(handler: Arc<dyn EventHandler>) -> Result<()> {
        let bus = match EVENT_BUS.try_with(|bus| bus.clone()) {
            Ok(bus) => bus,
            Err(_) => return Ok(()), // No bus context, silently ignore
        };

        let mut handlers = bus.handlers.write().await;
        handlers.push(handler);
        Ok(())
    }

    /// Emit a new event using the current task-local event bus
    pub async fn emit(
        event: AlienEvent,
        parent_id: Option<String>,
        state: EventState,
    ) -> Result<EventHandle> {
        let bus = match EVENT_BUS.try_with(|bus| bus.clone()) {
            Ok(bus) => bus,
            Err(_) => return Ok(EventHandle::noop()), // No bus context, return no-op handle
        };

        // Generate unique ID
        let id = Uuid::new_v4().to_string();

        // Use provided parent_id or check thread-local context
        let effective_parent_id =
            parent_id.or_else(|| PARENT_EVENT_ID.try_with(|p| p.clone()).ok().flatten());

        let now = Utc::now();

        // Create the event change
        let change = EventChange::Created {
            id: id.clone(),
            parent_id: effective_parent_id.clone(),
            created_at: now,
            event: event.clone(),
            state: state.clone(),
        };

        // Notify handlers and collect any errors
        {
            let handlers = bus.handlers.read().await;
            for handler in handlers.iter() {
                handler
                    .on_event_change(change.clone())
                    .await
                    .context(ErrorData::GenericError {
                        message: "Event handler failed".to_string(),
                    })?;
            }
        }

        Ok(EventHandle::new(id, effective_parent_id))
    }

    /// Update an existing event using the current task-local event bus
    pub async fn update(id: &str, event: AlienEvent) -> Result<()> {
        let bus = match EVENT_BUS.try_with(|bus| bus.clone()) {
            Ok(bus) => bus,
            Err(_) => return Ok(()), // No bus context, silently ignore
        };

        let now = Utc::now();
        let change = EventChange::Updated {
            id: id.to_string(),
            updated_at: now,
            event,
        };

        // Notify handlers and collect any errors
        let handlers = bus.handlers.read().await;
        for handler in handlers.iter() {
            handler
                .on_event_change(change.clone())
                .await
                .context(ErrorData::GenericError {
                    message: "Event handler failed".to_string(),
                })?;
        }

        Ok(())
    }

    /// Update the state of an event using the current task-local event bus
    pub async fn update_state(id: &str, new_state: EventState) -> Result<()> {
        let bus = match EVENT_BUS.try_with(|bus| bus.clone()) {
            Ok(bus) => bus,
            Err(_) => return Ok(()), // No bus context, silently ignore
        };

        let now = Utc::now();
        let change = EventChange::StateChanged {
            id: id.to_string(),
            updated_at: now,
            new_state,
        };

        // Notify handlers and collect any errors
        let handlers = bus.handlers.read().await;
        for handler in handlers.iter() {
            if let Err(e) = handler.on_event_change(change.clone()).await {
                // Return the first handler error we encounter
                return Err(e).context(ErrorData::GenericError {
                    message: "Event handler failed".to_string(),
                });
            }
        }

        Ok(())
    }

    /// Run a function with a parent event context
    pub async fn with_parent<F, Fut, T>(parent_id: Option<String>, f: F) -> T
    where
        F: FnOnce(&EventHandle) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        // Create a dummy handle for the context
        let handle = EventHandle::new(parent_id.clone().unwrap_or_else(|| String::new()), None);

        if let Some(parent) = parent_id {
            PARENT_EVENT_ID.scope(Some(parent), f(&handle)).await
        } else {
            f(&handle).await
        }
    }

    /// Get the current event bus from task-local storage if available
    pub fn current() -> Option<Self> {
        EVENT_BUS.try_with(|bus| bus.clone()).ok()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alien_error::AlienError;

    use crate::ErrorData;

    use super::*;
    use std::sync::Mutex;

    struct TestHandler {
        changes: Arc<Mutex<Vec<EventChange>>>,
    }

    #[async_trait::async_trait]
    impl EventHandler for TestHandler {
        async fn on_event_change(&self, change: EventChange) -> Result<()> {
            let mut changes = self.changes.lock().unwrap();
            changes.push(change);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_event_emission() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            changes: changes.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        bus.run(|| async {
            let _handle = AlienEvent::BuildingStack {
                stack: "test".to_string(),
            }
            .emit()
            .await
            .unwrap();

            // Check that we got a Created change
            let changes = changes.lock().unwrap();
            assert_eq!(changes.len(), 1);
            match &changes[0] {
                EventChange::Created { event, .. } => match event {
                    AlienEvent::BuildingStack { stack } => assert_eq!(stack, "test"),
                    _ => panic!("Wrong event type"),
                },
                _ => panic!("Expected Created change"),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_event_hierarchy() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            changes: changes.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        bus.run(|| async {
            let parent = AlienEvent::BuildingStack {
                stack: "parent".to_string(),
            }
            .emit()
            .await
            .unwrap();

            // Use as_parent to establish context for child events
            parent
                .as_parent(|_| async {
                    AlienEvent::TestBuildImage {
                        image: "child".to_string(),
                        stage: "test".to_string(),
                    }
                    .emit()
                    .await
                    .unwrap();
                })
                .await;

            let changes = changes.lock().unwrap();
            assert_eq!(changes.len(), 2);

            // Check parent
            match &changes[0] {
                EventChange::Created { id, parent_id, .. } => {
                    assert_eq!(id, &parent.id);
                    assert_eq!(parent_id, &None);
                }
                _ => panic!("Expected Created change for parent"),
            }

            // Check child
            match &changes[1] {
                EventChange::Created { parent_id, .. } => {
                    assert_eq!(parent_id, &Some(parent.id.clone()));
                }
                _ => panic!("Expected Created change for child"),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_event_update() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            changes: changes.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        bus.run(|| async {
            let handle = AlienEvent::TestBuildImage {
                image: "test".to_string(),
                stage: "stage1".to_string(),
            }
            .emit()
            .await
            .unwrap();

            handle
                .update(AlienEvent::TestBuildImage {
                    image: "test".to_string(),
                    stage: "stage2".to_string(),
                })
                .await
                .unwrap();

            let changes = changes.lock().unwrap();
            assert_eq!(changes.len(), 2);

            // Check update
            match &changes[1] {
                EventChange::Updated { id, event, .. } => {
                    assert_eq!(id, &handle.id);
                    match event {
                        AlienEvent::TestBuildImage { stage, .. } => assert_eq!(stage, "stage2"),
                        _ => panic!("Wrong event type"),
                    }
                }
                _ => panic!("Expected Updated change"),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_scoped_success() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            changes: changes.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        bus.run(|| async {
            let result = AlienEvent::BuildingStack {
                stack: "test".to_string(),
            }
            .in_scope(|_handle| async move {
                // Emit a child event - this will automatically be a child due to in_scope
                AlienEvent::TestBuildImage {
                    image: "child".to_string(),
                    stage: "test".to_string(),
                }
                .emit()
                .await
                .unwrap();
                Ok::<_, AlienError<ErrorData>>(42)
            })
            .await
            .unwrap();

            assert_eq!(result, 42);

            let changes = changes.lock().unwrap();
            assert_eq!(changes.len(), 3); // Created (Started), Created (child), StateChanged (Success)

            // Check final state change
            match &changes[2] {
                EventChange::StateChanged { new_state, .. } => {
                    assert_eq!(new_state, &EventState::Success);
                }
                _ => panic!("Expected StateChanged to Success"),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_scoped_failure() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            changes: changes.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        bus.run(|| async {
            let result = AlienEvent::BuildingStack {
                stack: "test".to_string(),
            }
            .in_scope(|_handle| async move {
                Err::<i32, _>(AlienError::new(ErrorData::InvalidResourceUpdate { resource_id: "my_resource".to_string(), reason: "hummus".to_string() }))
            })
            .await;

            assert!(result.is_err());
            let err = result.err().unwrap();
            assert!(matches!(&err.error, Some(ErrorData::InvalidResourceUpdate { resource_id, .. }) if resource_id == "my_resource"));

            let changes = changes.lock().unwrap();
            assert_eq!(changes.len(), 2); // Created (Started), StateChanged (Failed)

            // Check final state change
            match &changes[1] {
                EventChange::StateChanged { new_state, .. } => match new_state {
                    EventState::Failed { error } => {
                        let error = error.as_ref().expect("Expected error to be present");
                        assert_eq!(error.message, "Resource 'my_resource' cannot be updated: hummus")
                    }
                    _ => panic!("Expected Failed state"),
                },
                _ => panic!("Expected StateChanged to Failed"),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_no_event_bus_context() {
        // Try to emit an event without an event bus context
        let result = AlienEvent::BuildingStack {
            stack: "test".to_string(),
        }
        .emit()
        .await;

        // Should succeed with a no-op handle
        assert!(result.is_ok());
        let handle = result.unwrap();
        assert!(handle.is_noop);
    }
}

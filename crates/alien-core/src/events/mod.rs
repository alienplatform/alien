//! # Alien Events System
//!
//! A Rust-based events system for user-facing events in Alien. This system is designed to handle
//! events that can be consumed by dashboards, CLIs, and other monitoring tools to show users
//! what's happening during builds, deployments, and other operations.
//!
//! ## Why Not Use Traces?
//!
//! This events system is **not** an alternative to traces - traces are for engineering observability
//! and internal debugging, while this events module is about user-facing events that you show in UIs
//! (either GUI or TUI). Key differences:
//!
//! - **Reliable User Flow**: Events are blocking (unlike non-blocking traces) because we want to
//!   guarantee users see critical progress updates and fail fast if we can't communicate status
//! - **Rich UI Components**: Events have strict schemas (see `AlienEvent` enum) enabling translation
//!   to custom React components, progress bars, and interactive UI elements that show meaningful progress
//! - **Flexible Granularity**: Unlike traces (spans + events), we only have events, with infinite
//!   hierarchy and unique IDs. Users can choose their view: high-level "Building" → "Deploying" or
//!   drill down to see "Building image", "Pushing image", etc. Each event can be scoped for perfect
//!   user control over detail level
//! - **Live Progress Updates**: Events can be updated with new information (e.g., "pushing image...
//!   layer 1/5, layer 2/5, layer 3/5") enabling real-time progress indicators, which is impossible
//!   with immutable traces
//! - **Clear Success/Failure States**: Each scoped event has explicit states (Started, Success, Failed)
//!   with detailed error information, giving users immediate feedback on what succeeded, what failed,
//!   and exactly why
//!
//! ## Key Features
//!
//! - **Global Event Bus**: Events can be emitted from anywhere in the code without passing around
//!   event bus instances, making it easy to add to large Rust workspaces with many crates.
//! - **Hierarchical Events**: Events can have parent-child relationships for organizing complex operations.
//! - **State Management**: Events can track their lifecycle (None, Started, Success, Failed).
//! - **Durable Execution Support**: Designed to work with frameworks like Temporal, Inngest, and Restate
//!   where processes can restart and state needs to be preserved externally.
//! - **Change-based Architecture**: Instead of storing events in memory, the system emits changes
//!   (Created, Updated, StateChanged) that handlers can persist or react to.
//! - **Macro Support**: The `#[alien_event]` macro provides a convenient way to instrument functions
//!   with events, similar to tracing's `#[instrument]` macro.
//!
//! ## Basic Usage
//!
//! ### Using the `#[alien_event]` Macro (Recommended)
//!
//! The easiest way to instrument functions with events is using the `#[alien_event]` macro:
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, EventBus, alien_event, Result};
//!
//! #[alien_event(AlienEvent::BuildingStack { stack: "my-stack".to_string() })]
//! async fn build_stack() -> Result<()> {
//!     // All events emitted within this function will automatically be children
//!     // of the BuildingStack event. The event will be marked as successful
//!     // if the function returns Ok, or failed if it returns an error.
//!     
//!     AlienEvent::BuildingImage {
//!         image: "api:latest".to_string(),
//!     }
//!     .emit()
//!     .await?;
//!     
//!     Ok(())
//! }
//!
//! # async fn example() -> Result<()> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     build_stack().await?;
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Simple Event Emission
//!
//! For more control, you can emit events manually:
//!
//! ```rust
//! use alien_core::{AlienEvent, EventBus};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     // Emit a simple event
//!     let handle = AlienEvent::BuildingStack {
//!         stack: "my-stack".to_string(),
//!     }
//!     .emit()
//!     .await?;
//!     
//!     println!("Emitted event with ID: {}", handle.id);
//!     Ok::<_, Box<dyn std::error::Error>>(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Event Updates
//!
//! ```rust
//! use alien_core::{AlienEvent, EventBus};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     // Emit an event and get a handle
//!     let handle = AlienEvent::BuildingImage {
//!         image: "api:latest".to_string(),
//!     }
//!     .emit()
//!     .await?;
//!     
//!     // Update the event with new information
//!     handle.update(AlienEvent::BuildingImage {
//!         image: "api:latest-v2".to_string(),
//!     }).await;
//!     
//!     // Mark as completed
//!     handle.complete().await;
//!     Ok::<_, Box<dyn std::error::Error>>(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Scoped Events with Automatic State Management
//!
//! You can also use `in_scope` directly for more control over the event lifecycle:
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, EventBus, ErrorData, Result};
//! use alien_error::AlienError;
//!
//! # async fn example() -> Result<()> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     // Use in_scope for automatic success/failure tracking
//!     // All events emitted within the scope automatically become children
//!     let result = AlienEvent::BuildingStack {
//!         stack: "my-stack".to_string(),
//!     }
//!     .in_scope(|_handle| async move {
//!         // This event will automatically be a child of BuildingStack
//!         AlienEvent::BuildingImage {
//!             image: "api:latest".to_string(),
//!         }
//!         .emit()
//!         .await?;
//!         
//!         // Do some work that might fail
//!         std::fs::create_dir_all("/tmp/build")
//!             .map_err(|e| AlienError::new(ErrorData::GenericError {
//!                 message: e.to_string(),
//!             }))?;
//!         
//!         // Return success
//!         Ok::<_, AlienError<ErrorData>>(42)
//!     })
//!     .await?;
//!     
//!     println!("Operation completed with result: {}", result);
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Event Hierarchy
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, EventBus, Result};
//!
//! # async fn example() -> Result<()> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     let parent = AlienEvent::BuildingStack {
//!         stack: "my-stack".to_string(),
//!     }
//!     .emit()
//!     .await?;
//!     
//!     // Create a parent context for multiple child events
//!     parent.as_parent(|_handle| async {
//!         // All events emitted here will be children of the parent
//!         AlienEvent::BuildingImage {
//!             image: "api:latest".to_string(),
//!         }
//!         .emit()
//!         .await?;
//!         
//!         AlienEvent::PushingImage {
//!             image: "api:latest".to_string(),
//!             progress: None,
//!         }
//!         .emit()
//!         .await?;
//!         
//!         Ok(())
//!     }).await?;
//!     
//!     // Complete the parent when all children are done
//!     parent.complete().await;
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Durable Execution Support
//!
//! The events system is designed to work with durable execution frameworks where processes
//! can restart and lose in-memory state. Instead of storing events in memory, the system
//! emits changes that external handlers can persist.
//!
//! ### Manual State Management for Durable Workflows
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, EventBus, EventState, Result};
//!
//! # async fn example() -> Result<()> {
//! let bus = EventBus::new();
//! bus.run(|| async {
//!     // In a durable execution framework like Temporal:
//!     
//!     // Step 1: Start a long-running operation
//!     let parent_handle = AlienEvent::BuildingStack {
//!         stack: "my-stack".to_string(),
//!     }
//!     .emit_with_state(EventState::Started)
//!     .await?;
//!     
//!     // Step 2: Perform work across multiple durable steps
//!     parent_handle.as_parent(|_handle| async {
//!         // ctx.run(|| { ... }) - durable step 1
//!         AlienEvent::BuildingImage {
//!             image: "api:latest".to_string(),
//!         }
//!         .emit()
//!         .await?;
//!         
//!         // ctx.run(|| { ... }) - durable step 2
//!         AlienEvent::PushingImage {
//!             image: "api:latest".to_string(),
//!             progress: None,
//!         }
//!         .emit()
//!         .await?;
//!         
//!         Ok(())
//!     }).await?;
//!     
//!     // Step 3: Complete the operation
//!     parent_handle.complete().await;
//!     Ok(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## The `#[alien_event]` Macro
//!
//! The `#[alien_event]` macro provides a convenient way to instrument async functions with events,
//! similar to how tracing's `#[instrument]` macro works for logging. It automatically:
//!
//! - Creates an event when the function starts (with `EventState::Started`)
//! - Establishes a parent context so all events emitted within the function become children
//! - Marks the event as successful (`EventState::Success`) if the function returns `Ok`
//! - Marks the event as failed (`EventState::Failed`) if the function returns an `Err`
//!
//! ### Basic Usage
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, alien_event, Result};
//!
//! #[alien_event(AlienEvent::BuildingStack { stack: "my-stack".to_string() })]
//! async fn build_stack() -> Result<()> {
//!     // Function implementation
//!     Ok(())
//! }
//! ```
//!
//! ### Dynamic Values
//!
//! You can use function parameters and expressions in the event definition:
//!
//! ```rust,ignore
//! use alien_core::{AlienEvent, alien_event, Result};
//!
//! #[alien_event(AlienEvent::BuildingStack { stack: format!("stack-{}", stack_id) })]
//! async fn build_dynamic_stack(stack_id: u32) -> Result<()> {
//!     // Function implementation
//!     Ok(())
//! }
//! ```
//!
//! ### Comparison with Manual Event Management
//!
//! The macro transforms this:
//!
//! ```rust,ignore
//! #[alien_event(AlienEvent::BuildingStack { stack: "my-stack".to_string() })]
//! async fn build_stack() -> Result<()> {
//!     // function body
//!     Ok(())
//! }
//! ```
//!
//! Into this:
//!
//! ```rust,ignore
//! async fn build_stack() -> Result<()> {
//!     AlienEvent::BuildingStack { stack: "my-stack".to_string() }
//!         .in_scope(|_event_handle| async move {
//!             // function body
//!             Ok(())
//!         })
//!         .await
//! }
//! ```
//!
//! ### Limitations
//!
//! - The macro only works with `async` functions
//! - For sync functions, use `AlienEvent::emit()` manually
//! - The event expression is evaluated when the function is called, not when the macro is expanded
//!
//! ## Event Handlers
//!
//! Event handlers receive changes and can persist them, update UIs, or trigger other actions:
//!
//! ```rust
//! use alien_core::{EventHandler, EventChange, EventBus, AlienEvent};
//! use async_trait::async_trait;
//!
//! struct PostgresEventHandler {
//!     // database connection pool, etc.
//! }
//!
//! #[async_trait]
//! impl EventHandler for PostgresEventHandler {
//!     async fn on_event_change(&self, change: EventChange) -> alien_core::Result<()> {
//!         match change {
//!             EventChange::Created { id, parent_id, created_at, event, state } => {
//!                 // Insert new event record into database
//!                 println!("Creating event {} with parent {:?}", id, parent_id);
//!             }
//!             EventChange::Updated { id, updated_at, event } => {
//!                 // Update event data in database
//!                 println!("Updating event {} at {}", id, updated_at);
//!             }
//!             EventChange::StateChanged { id, updated_at, new_state } => {
//!                 // Update event state in database
//!                 println!("Event {} state changed to {:?} at {}", id, new_state, updated_at);
//!             }
//!         }
//!         Ok(())
//!     }
//! }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let handler = std::sync::Arc::new(PostgresEventHandler {});
//! let bus = EventBus::with_handlers(vec![handler]);
//!
//! bus.run(|| async {
//!     // Events will now be persisted to PostgreSQL
//!     AlienEvent::BuildingStack {
//!         stack: "my-stack".to_string(),
//!     }
//!     .emit()
//!     .await?;
//!     Ok::<_, Box<dyn std::error::Error>>(())
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture Notes
//!
//! ### Change-Based Design
//!
//! Unlike traditional event systems that store complete event state, this system emits
//! incremental changes:
//!
//! - `EventChange::Created`: A new event was created with initial data and state
//! - `EventChange::Updated`: An event's data was updated
//! - `EventChange::StateChanged`: An event's state transitioned (None → Started → Success/Failed)
//!
//! This design is crucial for durable execution where the event bus itself doesn't persist
//! state across process restarts.
//!
//! ### Task-Local Context
//!
//! The event bus uses Tokio's task-local storage to provide a global context without
//! requiring explicit parameter passing. This makes it easy to add event emission to
//! existing codebases. The `#[alien_event]` macro leverages this design to provide
//! seamless instrumentation without requiring changes to function signatures.
//!
//! ### Error Handling
//!
//! Event emission is designed to be non-blocking and fault-tolerant. If no event bus
//! context is available, operations will return `EventBusError::NoEventBusContext`
//! but won't panic, allowing code to continue running even without event tracking.

mod event;
pub use event::*;

mod handler;
pub use handler::*;

mod state;
pub use state::*;

mod handle;
pub use handle::*;

mod bus;
pub use bus::*;

#[cfg(test)]
mod tests {
    use crate::events::{AlienEvent, EventBus, EventChange, EventHandler, EventState};
    use crate::{ErrorData, Result};
    use alien_error::{AlienError, GenericError};
    use async_trait::async_trait;
    use insta::assert_debug_snapshot;
    use rstest::*;
    use std::sync::{Arc, Mutex};

    /// Test event handler that captures all events for testing
    #[derive(Debug, Clone)]
    struct TestEventHandler {
        events: Arc<Mutex<Vec<EventChange>>>,
    }

    impl TestEventHandler {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn events(&self) -> Vec<EventChange> {
            self.events.lock().unwrap().clone()
        }

        #[allow(dead_code)]
        fn clear(&self) {
            self.events.lock().unwrap().clear();
        }
    }

    #[async_trait]
    impl EventHandler for TestEventHandler {
        async fn on_event_change(&self, change: EventChange) -> Result<()> {
            self.events.lock().unwrap().push(change);
            Ok(())
        }
    }

    /// Helper to run tests with event bus context
    async fn with_test_bus<F, Fut, R>(f: F) -> (R, TestEventHandler)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let handler = TestEventHandler::new();
        let bus = EventBus::with_handlers(vec![Arc::new(handler.clone())]);
        let result = bus.run(f).await;
        (result, handler)
    }

    #[tokio::test]
    async fn test_simple_event_emission() {
        let (result, handler) = with_test_bus(|| async {
            let handle = AlienEvent::TestBuildingStack {
                stack: "test-stack".to_string(),
            }
            .emit()
            .await
            .unwrap();

            assert!(!handle.id.is_empty());
            handle
        })
        .await;

        let events = handler.events();
        assert_eq!(events.len(), 1);

        match &events[0] {
            EventChange::Created {
                id,
                parent_id,
                event,
                state,
                ..
            } => {
                assert_eq!(id, &result.id);
                assert_eq!(parent_id, &None);
                assert!(
                    matches!(event, AlienEvent::TestBuildingStack { stack } if stack == "test-stack")
                );
                assert_eq!(state, &EventState::None);
            }
            _ => panic!("Expected Created event"),
        }
    }

    #[tokio::test]
    async fn test_event_update() {
        let (_, handler) = with_test_bus(|| async {
            let handle = AlienEvent::TestBuildImage {
                image: "api:latest".to_string(),
                stage: "stage 1".to_string(),
            }
            .emit()
            .await
            .unwrap();

            handle
                .update(AlienEvent::TestBuildImage {
                    image: "api:latest".to_string(),
                    stage: "stage 2".to_string(),
                })
                .await
                .unwrap();

            handle
                .update(AlienEvent::TestBuildImage {
                    image: "api:latest".to_string(),
                    stage: "stage 3".to_string(),
                })
                .await
                .unwrap();
        })
        .await;

        let events = handler.events();
        assert_eq!(events.len(), 3); // 1 create + 2 updates

        // Check updates
        match &events[1] {
            EventChange::Updated { event, .. } => {
                assert!(
                    matches!(event, AlienEvent::TestBuildImage { stage, .. } if stage == "stage 2")
                );
            }
            _ => panic!("Expected Updated event"),
        }

        match &events[2] {
            EventChange::Updated { event, .. } => {
                assert!(
                    matches!(event, AlienEvent::TestBuildImage { stage, .. } if stage == "stage 3")
                );
            }
            _ => panic!("Expected Updated event"),
        }
    }

    #[tokio::test]
    async fn test_scoped_event_success() {
        let (result, handler) = with_test_bus(|| async {
            AlienEvent::TestBuildingImage {
                image: "api:latest".to_string(),
            }
            .in_scope(|_handle| async move {
                // Emit child events
                AlienEvent::TestBuildImage {
                    image: "api:latest".to_string(),
                    stage: "compile".to_string(),
                }
                .emit()
                .await
                .unwrap();

                AlienEvent::TestBuildImage {
                    image: "api:latest".to_string(),
                    stage: "link".to_string(),
                }
                .emit()
                .await
                .unwrap();

                Ok::<_, AlienError<ErrorData>>(42)
            })
            .await
            .unwrap()
        })
        .await;

        assert_eq!(result, 42);

        let events = handler.events();

        // Should have: parent created (started), 2 children created, parent state changed to success
        assert!(events.len() >= 4);

        // Verify parent started
        match &events[0] {
            EventChange::Created { state, .. } => {
                assert_eq!(state, &EventState::Started);
            }
            _ => panic!("Expected Created event"),
        }

        // Verify parent completed successfully
        let last_event = events.last().unwrap();
        match last_event {
            EventChange::StateChanged { new_state, .. } => {
                assert_eq!(new_state, &EventState::Success);
            }
            _ => panic!("Expected StateChanged event"),
        }
    }

    #[tokio::test]
    async fn test_scoped_event_failure() {
        let (result, handler) = with_test_bus(|| async {
            AlienEvent::TestBuildingImage {
                image: "api:latest".to_string(),
            }
            .in_scope(|_handle| async move {
                // Emit a child event
                AlienEvent::TestBuildImage {
                    image: "api:latest".to_string(),
                    stage: "compile".to_string(),
                }
                .emit()
                .await
                .unwrap();

                // Then fail
                Err::<(), _>(AlienError::new(ErrorData::GenericError {
                    message: "Test error".to_string(),
                }))
            })
            .await
        })
        .await;

        assert!(result.is_err());

        let events = handler.events();

        // Verify parent failed
        let last_event = events.last().unwrap();
        match last_event {
            EventChange::StateChanged { new_state, .. } => {
                assert!(matches!(new_state, EventState::Failed { .. }));
            }
            _ => panic!("Expected StateChanged event"),
        }
    }

    #[tokio::test]
    async fn test_deep_hierarchy() {
        let (_, handler) = with_test_bus(|| async {
            AlienEvent::TestBuildingStack {
                stack: "root-stack".to_string(),
            }
            .in_scope(|_| async {
                // Level 1
                AlienEvent::TestBuildingImage {
                    image: "app1".to_string(),
                }
                .in_scope(|_| async {
                    // Level 2
                    AlienEvent::TestBuildImage {
                        image: "app1".to_string(),
                        stage: "compile".to_string(),
                    }
                    .in_scope(|_| async {
                        // Level 3
                        AlienEvent::TestBuildImage {
                            image: "app1".to_string(),
                            stage: "optimize".to_string(),
                        }
                        .emit()
                        .await
                        .unwrap();
                        Ok::<_, AlienError<ErrorData>>(())
                    })
                    .await
                    .unwrap();
                    Ok::<_, AlienError<ErrorData>>(())
                })
                .await
                .unwrap();

                // Another branch at level 1
                AlienEvent::TestBuildingImage {
                    image: "app2".to_string(),
                }
                .emit()
                .await
                .unwrap();

                Ok::<_, AlienError<ErrorData>>(())
            })
            .await
            .unwrap()
        })
        .await;

        // Create a hierarchy representation for snapshot testing
        let events = handler.events();
        let mut hierarchy = Vec::new();
        let mut id_map = std::collections::HashMap::new();
        let mut counter = 0;

        for event in &events {
            match event {
                EventChange::Created {
                    id,
                    parent_id,
                    event,
                    ..
                } => {
                    // Map real IDs to stable IDs for snapshot testing
                    let stable_id = id_map
                        .entry(id.clone())
                        .or_insert_with(|| {
                            counter += 1;
                            format!("event-{}", counter)
                        })
                        .clone();

                    let stable_parent_id = parent_id.as_ref().map(|p| {
                        id_map
                            .entry(p.clone())
                            .or_insert_with(|| {
                                counter += 1;
                                format!("event-{}", counter)
                            })
                            .clone()
                    });

                    hierarchy.push((stable_id, stable_parent_id, format!("{:?}", event)));
                }
                _ => {}
            }
        }

        assert_debug_snapshot!(hierarchy);
    }

    #[tokio::test]
    async fn test_wide_hierarchy() {
        let (_, handler) = with_test_bus(|| async {
            let parent = AlienEvent::TestBuildingStack {
                stack: "wide-stack".to_string(),
            }
            .emit()
            .await
            .unwrap();

            parent
                .as_parent(|_| async {
                    // Emit many child events
                    for i in 0..10 {
                        AlienEvent::TestBuildImage {
                            image: format!("image-{}", i),
                            stage: "build".to_string(),
                        }
                        .emit()
                        .await
                        .unwrap();
                    }
                    Ok::<_, ErrorData>(())
                })
                .await
                .unwrap();

            parent.complete().await.unwrap();
        })
        .await;

        let events = handler.events();

        // Count children
        let children_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    EventChange::Created {
                        parent_id: Some(_),
                        ..
                    }
                )
            })
            .count();

        assert_eq!(children_count, 10);
    }

    #[tokio::test]
    async fn test_durable_execution_simulation() {
        // Simulate a durable execution with multiple steps
        let (_, handler) = with_test_bus(|| async {
            // Step 1: Start parent event
            let parent_handle = AlienEvent::TestBuildingStack {
                stack: "durable-stack".to_string(),
            }
            .emit_with_state(EventState::Started)
            .await
            .unwrap();

            // Simulate ctx.run() boundaries
            let parent_id = parent_handle.id.clone();

            // Step 2: First child in separate "execution"
            parent_handle
                .as_parent(|_| async {
                    AlienEvent::TestBuildImage {
                        image: "api:latest".to_string(),
                        stage: "compile".to_string(),
                    }
                    .emit()
                    .await
                    .unwrap();
                    Ok::<_, ErrorData>(())
                })
                .await
                .unwrap();

            // Step 3: Second child in separate "execution"
            parent_handle
                .as_parent(|_| async {
                    AlienEvent::TestPushImage {
                        image: "api:latest".to_string(),
                    }
                    .emit()
                    .await
                    .unwrap();
                    Ok::<_, ErrorData>(())
                })
                .await
                .unwrap();

            // Step 4: Complete parent
            parent_handle.complete().await.unwrap();

            parent_id
        })
        .await;

        let events = handler.events();

        // Verify sequence of events
        assert!(events.len() >= 4); // parent created, 2 children, parent completed

        // Verify all events are properly linked
        let created_events: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                EventChange::Created { id, parent_id, .. } => Some((id.clone(), parent_id.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(created_events.len(), 3); // 1 parent + 2 children
    }

    #[tokio::test]
    async fn test_manual_state_management() {
        let (_, handler) = with_test_bus(|| async {
            let handle = AlienEvent::TestBuildingStack {
                stack: "manual-stack".to_string(),
            }
            .emit_with_state(EventState::Started)
            .await
            .unwrap();

            // Do some work...

            // Manually fail the event
            handle
                .fail(AlienError::new(GenericError {
                    message: "Something went wrong".to_string(),
                }))
                .await
                .unwrap();
        })
        .await;

        let events = handler.events();

        // Check initial state
        match &events[0] {
            EventChange::Created { state, .. } => {
                assert_eq!(state, &EventState::Started);
            }
            _ => panic!("Expected Created event"),
        }

        // Check failure
        match &events[1] {
            EventChange::StateChanged { new_state, .. } => match new_state {
                EventState::Failed { error } => {
                    let error = error.as_ref().expect("Expected error to be present");
                    assert_eq!(error.message, "Something went wrong");
                }
                _ => panic!("Expected Failed state"),
            },
            _ => panic!("Expected StateChanged event"),
        }
    }

    #[tokio::test]
    async fn test_concurrent_events() {
        let (_, handler) = with_test_bus(|| async {
            let handles = tokio::join!(
                AlienEvent::TestBuildImage {
                    image: "image1".to_string(),
                    stage: "build".to_string(),
                }
                .emit(),
                AlienEvent::TestBuildImage {
                    image: "image2".to_string(),
                    stage: "build".to_string(),
                }
                .emit(),
                AlienEvent::TestBuildImage {
                    image: "image3".to_string(),
                    stage: "build".to_string(),
                }
                .emit(),
            );

            assert!(handles.0.is_ok());
            assert!(handles.1.is_ok());
            assert!(handles.2.is_ok());
        })
        .await;

        let events = handler.events();
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn test_nested_scopes_with_errors() {
        let (result, handler) = with_test_bus(|| async {
            AlienEvent::TestBuildingStack {
                stack: "nested-error-stack".to_string(),
            }
            .in_scope(|_| async {
                // This should succeed
                AlienEvent::TestBuildImage {
                    image: "image1".to_string(),
                    stage: "stage1".to_string(),
                }
                .emit()
                .await
                .unwrap();

                // This scope should fail
                let inner_result = AlienEvent::TestBuildImage {
                    image: "image2".to_string(),
                    stage: "stage2".to_string(),
                }
                .in_scope(|_| async {
                    Err::<(), _>(AlienError::new(ErrorData::GenericError {
                        message: "Inner error".to_string(),
                    }))
                })
                .await;

                // Continue despite inner failure
                assert!(inner_result.is_err());

                Ok::<_, AlienError<ErrorData>>("Outer succeeded")
            })
            .await
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Outer succeeded");

        // Verify mixed success/failure states
        let state_changes: Vec<_> = handler
            .events()
            .iter()
            .filter_map(|e| match e {
                EventChange::StateChanged { id, new_state, .. } => {
                    Some((id.clone(), new_state.clone()))
                }
                _ => None,
            })
            .collect();

        assert!(state_changes
            .iter()
            .any(|(_, state)| matches!(state, EventState::Failed { .. })));
        assert!(state_changes
            .iter()
            .any(|(_, state)| matches!(state, EventState::Success)));
    }

    #[tokio::test]
    async fn test_no_event_bus_context() {
        // Test behavior when no event bus is present
        let result = AlienEvent::TestBuildingStack {
            stack: "no-context".to_string(),
        }
        .emit()
        .await;

        // Should succeed with a no-op handle
        assert!(result.is_ok());
        let handle = result.unwrap();
        assert!(handle.is_noop);
    }

    #[rstest]
    #[case("stack1", "image1", "stage1")]
    #[case("stack2", "image2", "stage2")]
    #[case("stack3", "image3", "stage3")]
    #[tokio::test]
    async fn test_parameterized_events(
        #[case] stack: &str,
        #[case] image: &str,
        #[case] stage: &str,
    ) {
        let (_, handler) = with_test_bus(|| async {
            AlienEvent::TestBuildingStack {
                stack: stack.to_string(),
            }
            .in_scope(|_| async move {
                AlienEvent::TestBuildImage {
                    image: image.to_string(),
                    stage: stage.to_string(),
                }
                .emit()
                .await
                .unwrap();
                Ok::<_, AlienError<ErrorData>>(())
            })
            .await
            .unwrap()
        })
        .await;

        let events = handler.events();
        assert!(events.len() >= 3); // parent created, child created, parent success
    }

    #[tokio::test]
    async fn test_complex_workflow_snapshot() {
        let (_, handler) = with_test_bus(|| async {
            // Simulate a complex deployment workflow
            AlienEvent::TestBuildingStack {
                stack: "production-stack".to_string(),
            }
            .in_scope(|_| async {
                // Build phase
                AlienEvent::TestBuildingImage {
                    image: "api-service".to_string(),
                }
                .in_scope(|_| async {
                    for stage in ["download-deps", "compile", "optimize", "build"] {
                        AlienEvent::TestBuildImage {
                            image: "api-service".to_string(),
                            stage: stage.to_string(),
                        }
                        .emit()
                        .await
                        .unwrap();
                    }
                    Ok::<_, AlienError<ErrorData>>(())
                })
                .await
                .unwrap();

                // Push phase
                AlienEvent::TestPushImage {
                    image: "api-service".to_string(),
                }
                .emit()
                .await
                .unwrap();

                // Deploy phase
                AlienEvent::TestDeployingStack {
                    stack: "api-service".to_string(),
                }
                .in_scope(|_| async {
                    AlienEvent::TestCreatingResource {
                        resource_type: "LoadBalancer".to_string(),
                        resource_name: "api-lb".to_string(),
                        details: Some("Updating target groups".to_string()),
                    }
                    .emit()
                    .await
                    .unwrap();

                    AlienEvent::TestPerformingHealthCheck {
                        target: "https://api.example.com/health".to_string(),
                        check_type: "HTTP".to_string(),
                    }
                    .emit()
                    .await
                    .unwrap();

                    Ok::<_, AlienError<ErrorData>>(())
                })
                .await
                .unwrap();

                Ok::<_, AlienError<ErrorData>>(())
            })
            .await
            .unwrap()
        })
        .await;

        // Create a structured view of all events for snapshot
        let events = handler.events();
        let mut id_map = std::collections::HashMap::new();
        let mut counter = 0;

        let snapshot_data: Vec<_> = events
            .iter()
            .map(|e| match e {
                EventChange::Created {
                    id,
                    parent_id,
                    event,
                    state,
                    ..
                } => {
                    let stable_id = id_map
                        .entry(id.clone())
                        .or_insert_with(|| {
                            counter += 1;
                            format!("event-{}", counter)
                        })
                        .clone();

                    let stable_parent_id = parent_id.as_ref().map(|p| {
                        id_map
                            .entry(p.clone())
                            .or_insert_with(|| {
                                counter += 1;
                                format!("event-{}", counter)
                            })
                            .clone()
                    });

                    format!(
                        "Created: id={}, parent={:?}, event={:?}, state={:?}",
                        stable_id, stable_parent_id, event, state
                    )
                }
                EventChange::Updated { id, event, .. } => {
                    let stable_id = id_map
                        .entry(id.clone())
                        .or_insert_with(|| {
                            counter += 1;
                            format!("event-{}", counter)
                        })
                        .clone();

                    format!("Updated: id={}, event={:?}", stable_id, event)
                }
                EventChange::StateChanged { id, new_state, .. } => {
                    let stable_id = id_map
                        .entry(id.clone())
                        .or_insert_with(|| {
                            counter += 1;
                            format!("event-{}", counter)
                        })
                        .clone();

                    format!("StateChanged: id={}, new_state={:?}", stable_id, new_state)
                }
            })
            .collect();

        assert_debug_snapshot!(snapshot_data);
    }

    #[tokio::test]
    async fn test_multi_tenancy_with_http_server() {
        use axum::{http::header::HeaderMap, http::StatusCode, response::Response};
        use futures::future::join_all;
        use std::collections::HashMap;
        use tokio::sync::mpsc;

        // Create a channel to collect all events from all tenants
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<(String, EventChange)>();

        // Create a test handler that captures events with tenant context
        #[derive(Clone)]
        struct TenantAwareEventHandler {
            tenant_id: String,
            sender: mpsc::UnboundedSender<(String, EventChange)>,
        }

        #[async_trait]
        impl EventHandler for TenantAwareEventHandler {
            async fn on_event_change(&self, change: EventChange) -> Result<()> {
                let _ = self.sender.send((self.tenant_id.clone(), change));
                Ok(())
            }
        }

        // Helper function to simulate business logic that emits events
        let business_logic = {
            let event_tx = event_tx.clone();
            move |tenant_id: String| {
                let event_tx = event_tx.clone();
                async move {
                    // Create a handler specific to this tenant
                    let handler = Arc::new(TenantAwareEventHandler {
                        tenant_id: tenant_id.clone(),
                        sender: event_tx,
                    });

                    // Create event bus for this tenant
                    let bus = EventBus::with_handlers(vec![handler]);

                    bus.run(|| async {
                        // Main scoped event
                        AlienEvent::TestBuildingStack {
                            stack: format!("tenant-{}-stack", tenant_id),
                        }
                        .in_scope(|_handle| async move {
                            // First nested event
                            AlienEvent::TestBuildImage {
                                image: format!("tenant-{}-api", tenant_id),
                                stage: "compile".to_string(),
                            }
                            .emit()
                            .await
                            .map_err(|e| {
                                AlienError::new(ErrorData::GenericError {
                                    message: e.to_string(),
                                })
                            })?;

                            // Call another function that emits more events
                            deploy_service(&tenant_id).await?;

                            Ok::<_, AlienError<ErrorData>>(format!(
                                "Success for tenant {}",
                                tenant_id
                            ))
                        })
                        .await
                    })
                    .await
                }
            }
        };

        // Another function that emits events
        async fn deploy_service(tenant_id: &str) -> Result<()> {
            AlienEvent::TestDeployingStack {
                stack: format!("tenant-{}-deployment", tenant_id),
            }
            .emit()
            .await?;

            AlienEvent::TestCreatingResource {
                resource_type: "LoadBalancer".to_string(),
                resource_name: format!("tenant-{}-lb", tenant_id),
                details: Some("Multi-tenant load balancer".to_string()),
            }
            .emit()
            .await?;

            Ok(())
        }

        // Create HTTP endpoint handler
        let hello_handler = {
            let business_logic = business_logic.clone();
            move |headers: HeaderMap| {
                let business_logic = business_logic.clone();
                async move {
                    let tenant_id = headers
                        .get("x-tenant-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("default")
                        .to_string();

                    match business_logic(tenant_id.clone()).await {
                        Ok(result) => Response::builder()
                            .status(StatusCode::OK)
                            .body(format!(
                                "Hello from tenant {}! Result: {}",
                                tenant_id, result
                            ))
                            .unwrap(),
                        Err(e) => Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(format!("Error for tenant {}: {}", tenant_id, e))
                            .unwrap(),
                    }
                }
            }
        };

        // Smoke-test the handler without binding a socket (CI environments can forbid it).
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant-id", "tenant-0000".parse().unwrap());
        let _ = hello_handler(headers).await;

        // Create 10000 concurrent simulated requests with unique tenant IDs
        // Instead of actual HTTP requests, we'll directly call the business logic
        let mut request_futures = Vec::new();

        for i in 0..10000 {
            let tenant_id = format!("tenant-{:04}", i);
            let business_logic = business_logic.clone();

            let request_future = async move {
                match business_logic(tenant_id.clone()).await {
                    Ok(_) => tenant_id,
                    Err(e) => panic!("Request failed for tenant {}: {}", tenant_id, e),
                }
            };

            request_futures.push(request_future);
        }

        // Execute all requests concurrently
        let completed_tenants = join_all(request_futures).await;
        assert_eq!(completed_tenants.len(), 10000);

        // Collect all events
        let mut all_events: Vec<(String, EventChange)> = Vec::new();

        // Give some time for all events to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Drain the event receiver
        while let Ok(event) = event_rx.try_recv() {
            all_events.push(event);
        }

        // Verify we received events
        assert!(!all_events.is_empty(), "No events were received");

        // Group events by tenant
        let mut events_by_tenant: HashMap<String, Vec<EventChange>> = HashMap::new();
        for (tenant_id, event) in all_events.clone() {
            events_by_tenant.entry(tenant_id).or_default().push(event);
        }

        // Verify we have events for all tenants
        assert_eq!(
            events_by_tenant.len(),
            10000,
            "Expected events for 10000 tenants, got {}",
            events_by_tenant.len()
        );

        // Verify each tenant has the expected number of events
        for (tenant_id, events) in &events_by_tenant {
            // Each tenant should have:
            // 1. BuildingStack (created + state changed to started + state changed to success)
            // 2. BuildImage (created)
            // 3. DeployingStack (created)
            // 4. CreatingResource (created)
            // Total: 6 events minimum
            assert!(
                events.len() >= 4,
                "Tenant {} has {} events, expected at least 4",
                tenant_id,
                events.len()
            );

            // Verify tenant-specific data in events
            for event in events {
                if let EventChange::Created {
                    event: alien_event, ..
                } = event
                {
                    match alien_event {
                        AlienEvent::TestBuildingStack { stack } => {
                            assert!(
                                stack.contains(tenant_id),
                                "Stack name '{}' should contain tenant ID '{}'",
                                stack,
                                tenant_id
                            );
                        }
                        AlienEvent::TestBuildImage { image, .. } => {
                            assert!(
                                image.contains(tenant_id),
                                "Image name '{}' should contain tenant ID '{}'",
                                image,
                                tenant_id
                            );
                        }
                        AlienEvent::TestDeployingStack { stack } => {
                            assert!(
                                stack.contains(tenant_id),
                                "Deployment stack '{}' should contain tenant ID '{}'",
                                stack,
                                tenant_id
                            );
                        }
                        AlienEvent::TestCreatingResource { resource_name, .. } => {
                            assert!(
                                resource_name.contains(tenant_id),
                                "Resource name '{}' should contain tenant ID '{}'",
                                resource_name,
                                tenant_id
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        // Verify event hierarchy for a sample tenant
        let sample_tenant = "tenant-0000";
        let sample_events = events_by_tenant.get(sample_tenant).unwrap();

        // Find the parent BuildingStack event
        let parent_event = sample_events
            .iter()
            .find(|e| {
                matches!(
                    e,
                    EventChange::Created {
                        event: AlienEvent::TestBuildingStack { .. },
                        ..
                    }
                )
            })
            .expect("Should have TestBuildingStack event");

        if let EventChange::Created { id: parent_id, .. } = parent_event {
            // Verify child events reference the parent
            let child_events: Vec<_> = sample_events.iter().filter(|e| {
                matches!(e, EventChange::Created { parent_id: Some(pid), .. } if pid == parent_id)
            }).collect();

            assert!(
                !child_events.is_empty(),
                "Should have child events for parent {}",
                parent_id
            );
        }

        println!("✅ Multi-tenancy test passed!");
        println!("   - Processed 10000 concurrent requests");
        println!("   - Verified {} unique tenants", events_by_tenant.len());
        println!("   - Total events captured: {}", all_events.len());
        println!(
            "   - Average events per tenant: {:.1}",
            all_events.len() as f64 / events_by_tenant.len() as f64
        );
    }

    #[tokio::test]
    async fn test_handler_failure() {
        // Create a handler that always fails
        struct FailingHandler;

        #[async_trait]
        impl EventHandler for FailingHandler {
            async fn on_event_change(&self, _change: EventChange) -> Result<()> {
                Err(AlienError::new(ErrorData::GenericError {
                    message: "Handler intentionally failed".to_string(),
                }))
            }
        }

        let failing_handler = Arc::new(FailingHandler);
        let bus = EventBus::with_handlers(vec![failing_handler]);

        let result = bus
            .run(|| async {
                // Try to emit an event - this should fail because the handler fails
                AlienEvent::TestBuildingStack {
                    stack: "test-stack".to_string(),
                }
                .emit()
                .await
            })
            .await;

        // Verify that the event emission failed due to handler failure
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Event handler failed"));
        assert!(error.to_string().contains("Handler intentionally failed"));
    }

    #[tokio::test]
    async fn test_mixed_handlers_one_fails() {
        // Create one successful handler and one failing handler
        let successful_handler = TestEventHandler::new();

        struct FailingHandler;

        #[async_trait]
        impl EventHandler for FailingHandler {
            async fn on_event_change(&self, _change: EventChange) -> Result<()> {
                Err(AlienError::new(ErrorData::GenericError {
                    message: "Second handler failed".to_string(),
                }))
            }
        }

        let failing_handler = Arc::new(FailingHandler);
        let bus =
            EventBus::with_handlers(vec![Arc::new(successful_handler.clone()), failing_handler]);

        let result = bus
            .run(|| async {
                // Try to emit an event - this should fail because one handler fails
                AlienEvent::TestBuildingStack {
                    stack: "test-stack".to_string(),
                }
                .emit()
                .await
            })
            .await;

        // Verify that the event emission failed
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Event handler failed"));
        assert!(error.to_string().contains("Second handler failed"));

        // The successful handler should have been called before the failing one
        let events = successful_handler.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            EventChange::Created { event, .. } => {
                assert!(
                    matches!(event, AlienEvent::TestBuildingStack { stack } if stack == "test-stack")
                );
            }
            _ => panic!("Expected Created event"),
        }
    }

    #[tokio::test]
    async fn test_handler_failure_in_scoped_event() {
        struct FailingHandler;

        #[async_trait]
        impl EventHandler for FailingHandler {
            async fn on_event_change(&self, _change: EventChange) -> Result<()> {
                Err(AlienError::new(ErrorData::GenericError {
                    message: "Handler failed during scoped event".to_string(),
                }))
            }
        }

        let failing_handler = Arc::new(FailingHandler);
        let bus = EventBus::with_handlers(vec![failing_handler]);

        let result = bus
            .run(|| async {
                // Try to use in_scope - the initial event emission will fail, but in_scope
                // is designed to be fault-tolerant and will continue with a no-op handle
                AlienEvent::TestBuildingStack {
                    stack: "test-stack".to_string(),
                }
                .in_scope(|handle| async move {
                    // This code will be reached because in_scope is fault-tolerant
                    // The handle should be a no-op handle
                    assert!(handle.is_noop);
                    Ok::<_, AlienError<ErrorData>>(42)
                })
                .await
            })
            .await;

        // Verify that the scoped event succeeded despite handler failure
        // This demonstrates the fault-tolerant design of the event system
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_handler_failure_during_update() {
        // Create a handler that succeeds on creation but fails on updates
        struct UpdateFailingHandler {
            call_count: Arc<Mutex<usize>>,
        }

        #[async_trait]
        impl EventHandler for UpdateFailingHandler {
            async fn on_event_change(&self, change: EventChange) -> Result<()> {
                let mut count = self.call_count.lock().unwrap();
                *count += 1;

                match change {
                    EventChange::Created { .. } => Ok(()), // Allow creation
                    EventChange::Updated { .. } => {
                        // Fail on updates
                        Err(AlienError::new(ErrorData::GenericError {
                            message: "Handler failed during update".to_string(),
                        }))
                    }
                    EventChange::StateChanged { .. } => Ok(()), // Allow state changes
                }
            }
        }

        let call_count = Arc::new(Mutex::new(0));
        let handler = Arc::new(UpdateFailingHandler {
            call_count: call_count.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        let result = bus
            .run(|| async {
                // Create an event - this should succeed
                let handle = AlienEvent::TestBuildImage {
                    image: "test-image".to_string(),
                    stage: "stage1".to_string(),
                }
                .emit()
                .await?;

                // Try to update the event - this should fail
                handle
                    .update(AlienEvent::TestBuildImage {
                        image: "test-image".to_string(),
                        stage: "stage2".to_string(),
                    })
                    .await
            })
            .await;

        // Verify that the update failed
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Event handler failed"));
        assert!(error.to_string().contains("Handler failed during update"));

        // Verify the handler was called twice (once for create, once for failed update)
        assert_eq!(*call_count.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_alien_event_macro() {
        use crate::alien_event;

        let (result, handler) = with_test_bus(|| async {
            // Test the macro with a simple function
            #[alien_event(AlienEvent::TestBuildingStack { stack: "macro-test".to_string() })]
            async fn test_macro_function() -> Result<String> {
                // Emit a child event
                AlienEvent::TestBuildImage {
                    image: "test-image".to_string(),
                    stage: "compile".to_string(),
                }
                .emit()
                .await?;

                Ok("success".to_string())
            }

            test_macro_function().await
        })
        .await;

        // Verify the function succeeded
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");

        let events = handler.events();

        // Should have: parent created (started), child created, parent state changed to success
        assert!(events.len() >= 3);

        // Verify parent started
        match &events[0] {
            EventChange::Created { state, event, .. } => {
                assert_eq!(state, &EventState::Started);
                assert!(
                    matches!(event, AlienEvent::TestBuildingStack { stack } if stack == "macro-test")
                );
            }
            _ => panic!("Expected Created event"),
        }

        // Verify child event
        let child_event = events.iter().find(|e| {
            matches!(
                e,
                EventChange::Created {
                    event: AlienEvent::TestBuildImage { .. },
                    ..
                }
            )
        });
        assert!(child_event.is_some());

        // Verify parent completed successfully
        let success_event = events.iter().find(|e| {
            matches!(
                e,
                EventChange::StateChanged {
                    new_state: EventState::Success,
                    ..
                }
            )
        });
        assert!(success_event.is_some());
    }

    #[tokio::test]
    async fn test_alien_event_macro_with_failure() {
        use crate::alien_event;

        let (result, handler) = with_test_bus(|| async {
            // Test the macro with a function that fails
            #[alien_event(AlienEvent::TestBuildingStack { stack: "macro-fail-test".to_string() })]
            async fn test_macro_failure() -> Result<String> {
                // Emit a child event
                AlienEvent::TestBuildImage {
                    image: "test-image".to_string(),
                    stage: "compile".to_string(),
                }
                .emit()
                .await?;

                // Then fail
                Err(AlienError::new(ErrorData::GenericError {
                    message: "Macro test failure".to_string(),
                }))
            }

            test_macro_failure().await
        })
        .await;

        // Verify the function failed
        assert!(result.is_err());

        let events = handler.events();

        // Verify parent failed
        let failure_event = events.iter().find(|e| {
            matches!(
                e,
                EventChange::StateChanged {
                    new_state: EventState::Failed { .. },
                    ..
                }
            )
        });
        assert!(failure_event.is_some());
    }

    #[tokio::test]
    async fn test_alien_event_macro_with_dynamic_values() {
        use crate::alien_event;

        let (result, handler) = with_test_bus(|| async {
            // Test the macro with dynamic values
            async fn test_with_id(id: u32) -> Result<String> {
                #[alien_event(AlienEvent::TestBuildingStack { stack: format!("stack-{}", id) })]
                async fn inner_function(id: u32) -> Result<String> {
                    Ok(format!("processed-{}", id))
                }

                inner_function(id).await
            }

            test_with_id(42).await
        })
        .await;

        // Verify the function succeeded
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "processed-42");

        let events = handler.events();

        // Verify the dynamic stack name
        match &events[0] {
            EventChange::Created { event, .. } => {
                assert!(
                    matches!(event, AlienEvent::TestBuildingStack { stack } if stack == "stack-42")
                );
            }
            _ => panic!("Expected Created event"),
        }
    }

    #[tokio::test]
    async fn test_handler_failure_during_state_change() {
        // Create a handler that succeeds on creation but fails on state changes
        struct StateChangeFailingHandler {
            call_count: Arc<Mutex<usize>>,
        }

        #[async_trait]
        impl EventHandler for StateChangeFailingHandler {
            async fn on_event_change(&self, change: EventChange) -> Result<()> {
                let mut count = self.call_count.lock().unwrap();
                *count += 1;

                match change {
                    EventChange::Created { .. } => Ok(()), // Allow creation
                    EventChange::Updated { .. } => Ok(()), // Allow updates
                    EventChange::StateChanged { .. } => {
                        // Fail on state changes
                        Err(AlienError::new(ErrorData::GenericError {
                            message: "Handler failed during state change".to_string(),
                        }))
                    }
                }
            }
        }

        let call_count = Arc::new(Mutex::new(0));
        let handler = Arc::new(StateChangeFailingHandler {
            call_count: call_count.clone(),
        });
        let bus = EventBus::with_handlers(vec![handler]);

        let result = bus
            .run(|| async {
                // Create an event - this should succeed
                let handle = AlienEvent::TestBuildImage {
                    image: "test-image".to_string(),
                    stage: "stage1".to_string(),
                }
                .emit()
                .await?;

                // Try to complete the event - this should fail due to state change handler failure
                handle.complete().await
            })
            .await;

        // Verify that the state change failed
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Event handler failed"));
        assert!(error
            .to_string()
            .contains("Handler failed during state change"));

        // Verify the handler was called twice (once for create, once for failed state change)
        assert_eq!(*call_count.lock().unwrap(), 2);
    }
}

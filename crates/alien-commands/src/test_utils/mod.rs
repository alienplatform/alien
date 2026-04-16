//! Test utilities for alien-commands integration testing
//!
//! This module provides test infrastructure for testing command server and runtime
//! components in isolation and together. Key features:
//!
//! - `TestCommandServer`: Easy-to-use test server with local storage backends
//! - `MockDispatcher`: Captures dispatch calls for testing push scenarios
//! - Helper functions for creating test data and assertions
//!
//! # Usage
//!
//! ```rust
//! use alien_commands::test_utils::TestCommandServer;
//!
//! #[tokio::test]
//! async fn test_command_flow() {
//!     let server = TestCommandServer::new().await;
//!     
//!     // Create command
//!     let response = server.create_command(create_test_command()).await;
//!     
//!     // Simulate agent processing
//!     let lease = server.acquire_lease("test-agent").await.unwrap();
//!     let result = server.submit_command_response(&lease.command_id, test_response()).await;
//!     
//!     assert!(result.is_ok());
//! }
//! ```

#[cfg(feature = "test-utils")]
pub mod dispatcher;
#[cfg(feature = "test-utils")]
pub mod helpers;
#[cfg(feature = "test-utils")]
pub mod server;

#[cfg(feature = "test-utils")]
pub use dispatcher::{MockDispatcher, MockDispatcherMode};
#[cfg(feature = "test-utils")]
pub use helpers::*;
#[cfg(feature = "test-utils")]
pub use server::TestCommandServer;

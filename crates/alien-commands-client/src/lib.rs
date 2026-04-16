//! Rust client SDK for invoking commands on Alien deployments.
//!
//! Mirrors the TypeScript `@alienplatform/sdk/commands` — a high-level client
//! for creating, polling, and decoding command results.
//!
//! # Example
//!
//! ```no_run
//! use alien_commands_client::{CommandsClient, CommandsClientConfig};
//!
//! # async fn example() -> Result<(), alien_commands_client::CommandError> {
//! let client = CommandsClient::new("http://localhost:9090/v1", "dep_123", "token");
//! let result: serde_json::Value = client.invoke("generate-report", serde_json::json!({
//!     "startDate": "2025-01-01",
//! })).await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod error;

pub use client::{CommandsClient, CommandsClientConfig, InvokeOptions};
pub use error::CommandError;

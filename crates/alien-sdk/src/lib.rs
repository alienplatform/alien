//! Alien SDK for Rust.
//!
//! Cloud-agnostic bindings for storage, KV, queues, vault, commands, and more.
//! Works on AWS, GCP, Azure, Kubernetes, and locally.
//!
//! This is the public-facing crate for Alien app developers. It re-exports
//! everything from [`alien_bindings`], giving users the `alien_sdk` import path.
//!
//! # Example
//!
//! ```no_run
//! use alien_sdk::AlienContext;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let ctx = AlienContext::init().await?;
//!
//!     ctx.on_command("hello", |params: serde_json::Value| async move {
//!         Ok(serde_json::json!({ "message": "Hello!" }))
//!     });
//!
//!     ctx.run().await?;
//!     Ok(())
//! }
//! ```

pub use alien_bindings::*;

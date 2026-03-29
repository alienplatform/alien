//! `alien-test` -- E2E test infrastructure for Alien.
//!
//! Provides helpers for spinning up an in-process alien-manager, creating and
//! managing deployments, running alien-agent containers in pull mode, loading
//! test credentials from `.env.test`, and cleaning up resources after tests.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use alien_test::{DeploymentModel, Language, e2e};
//! use alien_core::Platform;
//!
//! #[tokio::test]
//! async fn my_e2e_test() {
//!     let ctx = e2e::setup(Platform::Aws, DeploymentModel::Push, Language::Rust)
//!         .await
//!         .unwrap();
//!     // Run checks against ctx.deployment ...
//! }
//! ```

pub mod agent;
pub mod build_push;
pub mod cleanup;
pub mod config;
pub mod deployment;
pub mod e2e;
pub mod manager;
pub mod ngrok;
pub mod setup;

// Re-exports for convenience
pub use agent::TestAlienAgent;
pub use cleanup::{cleanup_agent_containers, cleanup_all, cleanup_deployments, cleanup_helm_release};
pub use config::TestConfig;
pub use deployment::TestDeployment;
pub use e2e::{Binding, DeploymentModel, E2eContext, Language};
pub use manager::TestManager;
pub use setup::setup_target;

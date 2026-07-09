//! `alien-test` -- E2E test infrastructure for Alien.
//!
//! Provides helpers for spinning up an in-process alien-manager, creating and
//! managing deployments, running alien-operator containers in pull mode, loading
//! test credentials from `.env.test`, and cleaning up resources after tests.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use alien_test::{DeploymentModel, TestApp, e2e};
//! use alien_core::Platform;
//!
//! #[tokio::test]
//! async fn my_e2e_test() {
//!     let ctx = e2e::setup(Platform::Aws, DeploymentModel::Push, TestApp::ComprehensiveRust)
//!         .await
//!         .unwrap();
//!     // Run checks against ctx.deployment ...
//! }
//! ```

pub mod build_push;
pub mod cleanup;
pub mod config;
pub mod deployment;
pub mod distribution;
pub mod e2e;
pub mod helm_values;
pub mod managed_secret;
pub mod manager;
pub mod ngrok;
pub mod operator;
#[cfg(feature = "e2e-os-service")]
pub mod os_service;
pub mod setup;

// Re-exports for convenience
pub use cleanup::{
    cleanup_agent_containers, cleanup_all, cleanup_deployments, cleanup_helm_release,
};
pub use config::TestConfig;
pub use deployment::TestDeployment;
pub use e2e::{Binding, DeploymentModel, DistributionFlow, TestApp, TestContext};
pub use manager::TestManager;
pub use operator::TestAlienOperator;
pub use setup::{setup_target, teardown_target};

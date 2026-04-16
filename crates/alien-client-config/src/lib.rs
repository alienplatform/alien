//! Cloud client configuration and credential loading for Alien
//!
//! This crate provides a unified interface for loading cloud credentials from environment variables
//! and performing cloud-agnostic impersonation operations.
//!
//! # Example
//!
//! ```rust,no_run
//! use alien_client_config::ClientConfigExt;
//! use alien_core::{ClientConfig, Platform};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Load AWS credentials from environment
//! let aws_config = ClientConfig::from_std_env(Platform::Aws).await?;
//!
//! // Load GCP credentials from environment
//! let gcp_config = ClientConfig::from_std_env(Platform::Gcp).await?;
//!
//! // Load Azure credentials from environment
//! let azure_config = ClientConfig::from_std_env(Platform::Azure).await?;
//! # Ok(())
//! # }
//! ```

mod client_config_ext;

pub use alien_core::{ClientConfig, ImpersonationConfig};
pub use client_config_ext::ClientConfigExt;

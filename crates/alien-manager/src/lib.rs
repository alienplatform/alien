//! alien-manager — control plane for Alien applications.
//!
//! Stores releases, deploys them to remote environments, dispatches commands
//! to running deployments, and forwards telemetry. Single binary, SQLite-backed,
//! no external dependencies.
//!
//! ## Provider Architecture
//!
//! alien-manager uses trait-based providers for its core subsystems. Each has a
//! default implementation and can be replaced when embedding the server.
//!
//! ```rust,ignore
//! let server = AlienManagerBuilder::new(config)
//!     .deployment_store(my_deployment_store)
//!     .credential_resolver(my_credential_resolver)
//!     .telemetry_backend(my_telemetry_backend)
//!     .auth_validator(my_auth_validator)
//!     .build()
//!     .await?;
//!
//! server.start(addr).await?;
//! ```

pub mod commands;
pub mod config;
pub mod error;
pub(crate) mod ids;
pub mod traits;

#[cfg(feature = "sqlite")]
pub mod stores;

#[cfg(feature = "openapi")]
pub mod api;
pub mod builder;
pub(crate) mod dev;
pub mod loops;
pub mod providers;
pub mod registry_access;
pub mod transports;
pub mod routes;
pub mod server;

// Re-export key types
pub use builder::AlienManagerBuilder;
pub use config::{DeepStoreConfig, GcpOAuthConfig, ManagerConfig, PlatformConfig};
pub use dev::LogBuffer;
pub use routes::RouterOptions;
pub use server::AlienManager;
pub use traits::*;

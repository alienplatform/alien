//! Postgres controllers.
//!
//! Only the Local controller lives here. Cloud Postgres controllers are registered
//! separately; Kubernetes/on-prem use external bindings (no controller needed).

#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "local")]
pub use local::LocalPostgresController;

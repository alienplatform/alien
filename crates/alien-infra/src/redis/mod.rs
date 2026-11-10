//! Redis controllers.
//!
//! Only the Local controller lives here. Cloud Redis controllers are registered
//! separately; Kubernetes and on-prem use external bindings (no controller needed).

#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "local")]
pub use local::LocalRedisController;

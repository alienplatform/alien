//! Platform API provider implementations for SaaS mode.
//!
//! When alien-manager runs with `MANAGER_API_KEY` set, these providers replace the
//! default SQLite-backed stores with implementations that delegate to the Alien
//! Platform API (deployments, releases, tokens, credentials, telemetry, commands).

pub mod command_registry;
pub mod credential_resolver;
pub mod deepstore_telemetry_backend;
pub mod deployment_store;
pub mod error;
pub mod extension;
pub mod null_token_store;
pub mod release_store;
pub mod token_validator;
pub mod utils;

pub use command_registry::PlatformCommandRegistry;
pub use credential_resolver::ImpersonationCredentialResolver;
pub use deepstore_telemetry_backend::DeepStoreTelemetryBackend;
pub use deployment_store::PlatformApiDeploymentStore;
pub use error::ErrorData as PlatformErrorData;
pub use extension::PlatformState;
pub use null_token_store::NullTokenStore;
pub use release_store::PlatformApiReleaseStore;
pub use token_validator::PlatformTokenValidator;

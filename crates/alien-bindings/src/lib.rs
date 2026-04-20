use alien_error::AlienError;

// Re-export core traits and types
pub use alien_context::AlienContext;
pub use alien_core::{BindingsMode, Platform};
pub use error::{ErrorData, Result};
pub use provider::BindingsProvider;
pub use traits::{
    ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, RegistryAuthMethod,
    AwsServiceAccountInfo, AzureServiceAccountInfo, Binding, BindingsProviderApi, Build, Container,
    Function, GcpServiceAccountInfo, ImpersonationRequest, Kv, Queue, RepositoryResponse,
    ServiceAccount, ServiceAccountInfo, Storage, Vault,
};
pub use wait_until::{DrainConfig, DrainResponse, WaitUntil, WaitUntilContext};

pub mod error;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod providers;
pub mod traits;
// Re-export presigned types from alien-core
pub mod presigned {
    pub use alien_core::presigned::*;
}
pub mod alien_context;
pub mod http_client;
pub mod provider;

mod wait_until;

#[cfg(feature = "grpc")]
pub use grpc::control;
#[cfg(feature = "grpc")]
pub use grpc::control_service::ControlGrpcServer;
#[cfg(feature = "grpc")]
pub use grpc::GrpcServerHandles;

/// Gets the current platform from the ALIEN_DEPLOYMENT_TYPE environment variable.
/// This is used by the runtime to determine which platform-specific implementations to use.
pub fn get_current_platform() -> Result<Platform> {
    let env_vars: std::collections::HashMap<String, String> = std::env::vars().collect();
    get_platform_from_env(&env_vars)
}

/// Gets the platform from a HashMap of environment variables.
pub fn get_platform_from_env(env: &std::collections::HashMap<String, String>) -> Result<Platform> {
    let deployment_type = env.get("ALIEN_DEPLOYMENT_TYPE").ok_or_else(|| {
        AlienError::new(ErrorData::EnvironmentVariableMissing {
            variable_name: "ALIEN_DEPLOYMENT_TYPE".to_string(),
        })
    })?;

    deployment_type.parse().map_err(|_| {
        AlienError::new(ErrorData::InvalidEnvironmentVariable {
            variable_name: "ALIEN_DEPLOYMENT_TYPE".to_string(),
            value: deployment_type.clone(),
            reason: "Cannot parse the ALIEN_DEPLOYMENT_TYPE environment variable".to_string(),
        })
    })
}

/// Parse ALIEN_BINDINGS_MODE from environment variables.
/// Defaults to Direct if not specified.
pub fn get_bindings_mode_from_env(
    env: &std::collections::HashMap<String, String>,
) -> Result<BindingsMode> {
    let mode_str = env
        .get("ALIEN_BINDINGS_MODE")
        .map(|s| s.as_str())
        .unwrap_or("direct");

    mode_str.parse().map_err(|reason: String| {
        AlienError::new(ErrorData::InvalidEnvironmentVariable {
            variable_name: "ALIEN_BINDINGS_MODE".to_string(),
            value: mode_str.to_string(),
            reason,
        })
    })
}

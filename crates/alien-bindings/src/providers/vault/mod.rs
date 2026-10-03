#[cfg(feature = "aws")]
pub mod aws_parameter_store;
#[cfg(feature = "azure")]
pub mod azure_key_vault;
#[cfg(feature = "gcp")]
pub mod gcp_secret_manager;
#[cfg(feature = "kubernetes")]
pub mod kubernetes_secret;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_parameter_store::*;
#[cfg(feature = "azure")]
pub use azure_key_vault::*;
#[cfg(feature = "gcp")]
pub use gcp_secret_manager::*;
#[cfg(feature = "kubernetes")]
pub use kubernetes_secret::*;
#[cfg(feature = "local")]
pub use local::*;

/// Wraps a backend's failure to read a secret: a "no such resource" answer
/// becomes [`crate::ErrorData::VaultSecretNotFound`] so callers can tell an
/// unwritten secret from an outage; anything else keeps `message`.
#[cfg(any(
    feature = "aws",
    feature = "azure",
    feature = "gcp",
    feature = "kubernetes"
))]
pub(crate) fn secret_read_error(
    error: alien_error::AlienError<alien_client_core::ErrorData>,
    vault: &str,
    secret_name: &str,
    message: String,
) -> alien_error::AlienError<crate::ErrorData> {
    use alien_error::ContextError;

    let data = if matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    ) {
        crate::ErrorData::VaultSecretNotFound {
            vault: vault.to_string(),
            secret_name: secret_name.to_string(),
        }
    } else {
        crate::ErrorData::CloudPlatformError {
            message,
            resource_id: None,
        }
    };
    error.context(data)
}

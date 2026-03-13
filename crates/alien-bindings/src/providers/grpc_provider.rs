use crate::{
    error::{Error, ErrorData},
    providers::{
        artifact_registry::grpc::GrpcArtifactRegistry, build::grpc::GrpcBuild,
        function::grpc::GrpcFunction, kv::grpc::GrpcKv, queue::grpc::GrpcQueue,
        service_account::grpc::GrpcServiceAccount, storage::grpc::GrpcStorage,
        vault::grpc::GrpcVault,
    },
    traits::{
        ArtifactRegistry, BindingsProviderApi, Build, Container, Function, Kv, Queue,
        ServiceAccount, Storage, Vault,
    },
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tonic::transport::Channel;

/// Creates a gRPC channel from an address string, handling URI scheme normalization
/// and connection establishment with proper error handling.
pub async fn create_grpc_channel(grpc_address: String) -> crate::error::Result<Channel> {
    tracing::debug!(
        "create_grpc_channel: Connecting to gRPC server at: {}",
        grpc_address
    );

    // Ensure the address has a scheme, default to http if not present
    let endpoint_uri = if grpc_address.contains("://") {
        grpc_address.clone()
    } else {
        format!("http://{}", grpc_address)
    };

    tracing::debug!("create_grpc_channel: Endpoint URI: {}", endpoint_uri);

    let endpoint = Channel::from_shared(endpoint_uri.clone())
        .into_alien_error()
        .context(ErrorData::GrpcConnectionFailed {
            endpoint: endpoint_uri.clone(),
            reason: "Invalid gRPC endpoint URI format".to_string(),
        })?
        .timeout(std::time::Duration::from_secs(30)) // Request timeout
        .connect_timeout(std::time::Duration::from_secs(5)) // Connection establishment timeout
        .http2_keep_alive_interval(std::time::Duration::from_secs(30)) // Send keep-alive pings
        .keep_alive_timeout(std::time::Duration::from_secs(10)) // Keep-alive response timeout
        .keep_alive_while_idle(true); // Keep connection alive even when idle

    tracing::debug!("create_grpc_channel: Attempting to connect to endpoint");
    let channel =
        endpoint
            .connect()
            .await
            .into_alien_error()
            .context(ErrorData::GrpcConnectionFailed {
                endpoint: grpc_address.clone(),
                reason: "Failed to establish gRPC connection".to_string(),
            })?;

    tracing::debug!(
        "create_grpc_channel: Successfully connected to {}",
        grpc_address
    );
    Ok(channel)
}

/// gRPC implementation of the `BindingsProvider` trait.
///
/// This provider connects to a gRPC endpoint specified by the
/// `ALIEN_BINDINGS_GRPC_ADDRESS` environment variable.
///
/// Uses a shared gRPC channel to avoid file descriptor exhaustion.
#[derive(Debug, Clone)]
pub struct GrpcBindingsProvider {
    env: HashMap<String, String>,
    /// Shared gRPC channel, created lazily on first use
    channel: Arc<tokio::sync::OnceCell<Channel>>,
}

impl GrpcBindingsProvider {
    /// Creates a new provider that reads environment variables from `std::env`.
    /// This is disabled on wasm32 targets.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Result<Self, Error> {
        Self::new_with_env(std::env::vars().collect())
    }

    /// Creates a new provider with an empty environment map.
    /// This is the default constructor on wasm32 targets.
    #[cfg(target_arch = "wasm32")]
    pub fn new() -> Result<Self, Error> {
        Self::new_with_env(HashMap::new())
    }

    /// Creates a new provider that reads environment variables from the provided map.
    pub fn new_with_env(env: HashMap<String, String>) -> Result<Self, Error> {
        Ok(Self {
            env,
            channel: Arc::new(tokio::sync::OnceCell::new()),
        })
    }

    // Helper function to get environment variable, returning crate::Error
    fn get_env_var(&self, var_name: &str) -> Result<String, Error> {
        self.env.get(var_name).cloned().ok_or_else(|| {
            AlienError::new(ErrorData::EnvironmentVariableMissing {
                variable_name: var_name.to_string(),
            })
        })
    }

    // Helper function to get gRPC address for any binding
    fn get_grpc_address(&self) -> Result<String, Error> {
        self.get_env_var("ALIEN_BINDINGS_GRPC_ADDRESS")
    }

    /// Get or create the shared gRPC channel.
    /// This ensures we only have ONE channel per GrpcBindingsProvider instance,
    /// preventing file descriptor exhaustion.
    async fn get_channel(&self) -> Result<Channel, Error> {
        let channel = self
            .channel
            .get_or_try_init(|| async {
                let grpc_address = self.get_grpc_address()?;
                tracing::debug!(
                    "GrpcBindingsProvider: Creating shared gRPC channel to {}",
                    grpc_address
                );
                create_grpc_channel(grpc_address).await
            })
            .await?;

        Ok(channel.clone())
    }

    /// Public method to get the shared channel for use by other components (e.g., WaitUntilContext).
    /// This enables the entire AlienContext to use a single gRPC channel.
    pub async fn get_shared_channel(&self) -> Result<Channel, Error> {
        self.get_channel().await
    }
}

#[async_trait]
impl BindingsProviderApi for GrpcBindingsProvider {
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>, Error> {
        tracing::debug!(
            "GrpcBindingsProvider::load_storage: Loading storage binding: {}",
            binding_name
        );

        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let storage = Arc::new(
            GrpcStorage::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC storage".to_string(),
                })?,
        );

        tracing::debug!(
            "GrpcBindingsProvider::load_storage: Successfully loaded storage binding: {}",
            binding_name
        );
        Ok(storage)
    }

    async fn load_build(&self, binding_name: &str) -> Result<Arc<dyn Build>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let build = Arc::new(
            GrpcBuild::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC build".to_string(),
                })?,
        );

        Ok(build)
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn ArtifactRegistry>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let artifact_registry = Arc::new(
            GrpcArtifactRegistry::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC artifact registry".to_string(),
                })?,
        );

        Ok(artifact_registry)
    }

    async fn load_vault(&self, binding_name: &str) -> Result<Arc<dyn Vault>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let vault = Arc::new(
            GrpcVault::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC vault".to_string(),
                })?,
        );

        Ok(vault)
    }

    async fn load_kv(&self, binding_name: &str) -> Result<Arc<dyn Kv>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let kv = Arc::new(
            GrpcKv::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC KV".to_string(),
                })?,
        );

        Ok(kv)
    }

    async fn load_queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let queue = Arc::new(
            GrpcQueue::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC Queue".to_string(),
                })?,
        );

        Ok(queue)
    }

    async fn load_function(&self, binding_name: &str) -> Result<Arc<dyn Function>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let function = Arc::new(
            GrpcFunction::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC Function".to_string(),
                })?,
        );

        Ok(function)
    }

    async fn load_container(&self, binding_name: &str) -> Result<Arc<dyn Container>, Error> {
        use crate::providers::container::GrpcContainer;

        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let container = Arc::new(
            GrpcContainer::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC Container".to_string(),
                })?,
        );

        Ok(container)
    }

    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn ServiceAccount>, Error> {
        let channel = self
            .get_channel()
            .await
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to get shared gRPC channel".to_string(),
            })?;

        let service_account = Arc::new(
            GrpcServiceAccount::new_from_channel(channel, binding_name.to_string())
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Failed to initialize gRPC service account".to_string(),
                })?,
        );

        Ok(service_account)
    }
}

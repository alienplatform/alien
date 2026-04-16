pub mod kubernetes;
pub use kubernetes::*;

// Re-export commonly used types for convenience
pub use kubernetes::KubernetesClientConfigExt;

// Re-export all client APIs
pub use kubernetes::deployments::DeploymentApi;
pub use kubernetes::jobs::JobApi;
pub use kubernetes::kubernetes_client::KubernetesClient;
pub use kubernetes::pods::PodApi;
pub use kubernetes::secrets::SecretsApi;
pub use kubernetes::services::ServiceApi;

// Re-export error types from alien-client-core
pub use alien_client_core::{Error, ErrorData, Result};

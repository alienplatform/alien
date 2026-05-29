pub mod kubernetes;
pub use kubernetes::*;

// Re-export commonly used types for convenience
pub use kubernetes::KubernetesClientConfigExt;

// Re-export all client APIs
pub use kubernetes::deployments::DeploymentApi;
pub use kubernetes::events::EventApi;
pub use kubernetes::jobs::JobApi;
pub use kubernetes::kubernetes_client::KubernetesClient;
pub use kubernetes::metrics::MetricsApi;
pub use kubernetes::nodes::NodeApi;
pub use kubernetes::optional::{
    optional_events_read, optional_kubernetes_read, optional_metrics_read, optional_nodes_read,
    OptionalKubernetesRead, OptionalKubernetesReadContext, OptionalKubernetesReadSource,
    OptionalKubernetesReadStatus,
};
pub use kubernetes::pods::PodApi;
pub use kubernetes::routes::RouteApi;
pub use kubernetes::secrets::SecretsApi;
pub use kubernetes::services::ServiceApi;
pub use kubernetes::version::{KubernetesVersion, VersionApi};

// Re-export error types from alien-client-core
pub use alien_client_core::{Error, ErrorData, Result};

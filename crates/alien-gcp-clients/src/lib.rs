pub mod gcp;
pub use gcp::*;

// Re-export commonly used types for convenience
pub use gcp::{GcpClientConfig, GcpClientConfigExt, GcpCredentials, GcpImpersonationConfig};

/// Platform module alias for GCP types (used by tests)
pub mod platform {
    pub use crate::gcp::{
        GcpClientConfig, GcpClientConfigExt, GcpCredentials, GcpImpersonationConfig,
    };
}

// Re-export all client APIs
pub use gcp::artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient};
pub use gcp::cloudbuild::{CloudBuildApi, CloudBuildClient};
pub use gcp::cloudrun::{CloudRunApi, CloudRunClient};
pub use gcp::compute::{ComputeApi, ComputeClient};
pub use gcp::firestore::{FirestoreApi, FirestoreClient};
pub use gcp::gcs::{GcsApi, GcsClient};
pub use gcp::iam::{IamApi, IamClient};
pub use gcp::pubsub::{PubSubApi, PubSubClient};
pub use gcp::resource_manager::{ResourceManagerApi, ResourceManagerClient};
pub use gcp::secret_manager::{SecretManagerApi, SecretManagerClient};
pub use gcp::service_usage::{ServiceUsageApi, ServiceUsageClient};

// Re-export error types from alien-client-core
pub use alien_client_core::{Error, ErrorData, Result};

// Re-export commonly used data types
pub use gcp::iam::{Binding, IamPolicy, Role, ServiceAccount};
pub use gcp::longrunning::Operation;

// Re-export Compute Engine types
pub use gcp::compute::{
    Firewall, Network, Operation as ComputeOperation, Router, RouterNat, Subnetwork,
};

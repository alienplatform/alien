#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "azure")]
mod azure;
mod error;
#[cfg(feature = "gcp")]
mod gcp;
#[cfg(feature = "kubernetes")]
mod kubernetes;

use alien_core::{Platform, ResourceHeartbeat};
use async_trait::async_trait;

#[cfg(feature = "aws")]
pub use aws::{aws_raw_identity, AwsObserveContext, AwsObserver};
#[cfg(feature = "azure")]
pub use azure::{azure_raw_identity, AzureObserveContext, AzureObserver};
pub use error::Result;
#[cfg(feature = "gcp")]
pub use gcp::{gcp_raw_identity, GcpObserveContext, GcpObserver};
#[cfg(feature = "kubernetes")]
pub use kubernetes::{
    alien_resource_id_from_labels, raw_identity, KubernetesObserveContext, KubernetesObserver,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveScope {
    pub namespace: String,
    pub label_selector: Option<String>,
}

#[async_trait]
pub trait Observer: Send + Sync {
    fn platform(&self) -> Platform;

    async fn discover(&self, scope: &ObserveScope) -> Result<Vec<ResourceHeartbeat>>;
}

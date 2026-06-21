mod error;
#[cfg(feature = "kubernetes")]
mod kubernetes;

use alien_core::{Platform, ResourceHeartbeat};
use async_trait::async_trait;

pub use error::Result;
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

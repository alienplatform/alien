//! Linked-container binding handle.

use alien_bindings::traits::Container;
use napi_derive::napi;
use std::sync::Arc;

#[napi(object)]
pub struct InternalEndpoint {
    pub url: String,
}

/// Read-only service discovery for a linked container.
#[napi]
pub struct ContainerHandle {
    inner: Arc<dyn Container>,
}

impl ContainerHandle {
    pub(crate) fn new(inner: Arc<dyn Container>) -> Self {
        Self { inner }
    }
}

#[napi]
impl ContainerHandle {
    /// Return the named endpoints authorized for this workload.
    #[napi]
    pub async fn get_internal_endpoints(
        &self,
    ) -> std::collections::HashMap<String, InternalEndpoint> {
        self.inner
            .get_internal_endpoints()
            .into_iter()
            .map(|(name, endpoint)| (name, InternalEndpoint { url: endpoint.url }))
            .collect()
    }

    /// Return the URL reachable from the deployment's private network.
    #[napi]
    pub async fn get_internal_url(&self) -> String {
        self.inner.get_internal_url().to_string()
    }

    /// Return the public URL when the linked container is publicly exposed.
    #[napi]
    pub async fn get_public_url(&self) -> Option<String> {
        self.inner.get_public_url().map(str::to_string)
    }
}

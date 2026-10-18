//! Container binding implementations
//!
//! This module provides container implementations for different platforms:
//! - Managed cloud: For containers on AWS/GCP/Azure
//! - Local: For containers running in Docker during local development
//! - Kubernetes: For containers running as Kubernetes Services

mod horizon;
mod local;

#[cfg(feature = "kubernetes")]
mod kubernetes;

pub use horizon::HorizonContainer;
pub use local::LocalContainer;

#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesContainer;

/// Resolve endpoint values after workload-specific binding filtering.
fn resolve_internal_endpoints(
    endpoints: std::collections::HashMap<String, alien_core::ContainerInternalEndpointBinding>,
) -> crate::error::Result<
    std::collections::HashMap<String, alien_core::ContainerInternalEndpointOutput>,
> {
    use alien_error::Context;
    endpoints
        .into_iter()
        .map(|(name, endpoint)| {
            let url = endpoint
                .url
                .into_value("container", "internalEndpoints.url")
                .context(crate::error::ErrorData::BindingConfigInvalid {
                    env_var: crate::error::binding_env_var("container"),
                    binding_name: "container".to_owned(),
                    reason: format!("Failed to resolve internal endpoint {name}"),
                })?;
            Ok((name, alien_core::ContainerInternalEndpointOutput { url }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Container;

    #[test]
    fn named_endpoint_addresses_survive_provider_loading() {
        let value = serde_json::json!({"containerName":"api", "internalUrl":"http://api.svc:8080",
            "internalEndpoints":{"admin":{"url":"http://private-admin:3129"}}});
        let local = LocalContainer::new(serde_json::from_value(value.clone()).unwrap()).unwrap();
        let horizon = HorizonContainer::new(serde_json::from_value(value).unwrap()).unwrap();
        for container in [&local as &dyn Container, &horizon] {
            assert_eq!(
                container.get_internal_endpoints()["admin"].url,
                "http://private-admin:3129"
            );
            assert_eq!(container.get_internal_url(), "http://api.svc:8080");
            assert!(!container.get_internal_endpoints().contains_key("proxy"));
        }
        let old = LocalContainer::new(
            serde_json::from_value(serde_json::json!({
            "containerName":"api", "internalUrl":"http://api.svc:8080"}))
            .unwrap(),
        )
        .unwrap();
        assert!(old.get_internal_endpoints().is_empty());
    }
}

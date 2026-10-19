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

/// Resolve internal URLs supplied by the backend to every linking workload.
fn resolve_internal_urls(
    endpoints: std::collections::HashMap<u16, alien_core::BindingValue<String>>,
) -> crate::error::Result<std::collections::HashMap<u16, String>> {
    use alien_error::Context;
    endpoints
        .into_iter()
        .map(|(name, endpoint)| {
            let url = endpoint.into_value("container", "internalUrls").context(
                crate::error::ErrorData::BindingConfigInvalid {
                    env_var: crate::error::binding_env_var("container"),
                    binding_name: "container".to_owned(),
                    reason: format!("Failed to resolve internal endpoint {name}"),
                },
            )?;
            Ok((name, url))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Container;

    #[test]
    fn per_port_urls_survive_provider_loading() {
        let value = serde_json::json!({"containerName":"api", "internalUrl":"http://api.svc:8080",
            "internalUrls":{"3129":"http://private-admin:3129"}});
        let local = LocalContainer::new(serde_json::from_value(value.clone()).unwrap()).unwrap();
        let horizon = HorizonContainer::new(serde_json::from_value(value).unwrap()).unwrap();
        for container in [&local as &dyn Container, &horizon] {
            assert_eq!(
                container.get_internal_urls()[&3129],
                "http://private-admin:3129"
            );
            assert_eq!(container.get_internal_url(), "http://api.svc:8080");
            assert!(!container.get_internal_urls().contains_key(&3128));
        }
        let old = LocalContainer::new(
            serde_json::from_value(serde_json::json!({
            "containerName":"api", "internalUrl":"http://api.svc:8080"}))
            .unwrap(),
        )
        .unwrap();
        assert!(old.get_internal_urls().is_empty());
    }
}

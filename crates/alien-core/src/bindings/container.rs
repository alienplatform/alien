//! Container binding definitions for container-to-container communication
//!
//! This module defines the binding parameters for container resources:
//! - Horizon containers (AWS/GCP/Azure - using internal DNS and optional public URL)
//! - Local containers (Docker - using localhost URL)

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents a container binding for container-to-container or external communication
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum ContainerBinding {
    /// Horizon-managed container binding (AWS/GCP/Azure)
    Horizon(HorizonContainerBinding),
    /// Kubernetes container binding
    Kubernetes(KubernetesContainerBinding),
    /// Local Docker container binding
    Local(LocalContainerBinding),
}

/// Horizon container binding configuration (for cloud platforms)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizonContainerBinding {
    /// Container name in Horizon
    pub container_name: BindingValue<String>,
    /// Internal URL (e.g., "http://api.svc:8080")
    pub internal_url: BindingValue<String>,
    /// Optional public URL (if exposed publicly via load balancer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

/// Kubernetes container binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesContainerBinding {
    /// The container name
    pub name: BindingValue<String>,
    /// The Kubernetes namespace
    pub namespace: BindingValue<String>,
    /// The Kubernetes Service name
    pub service_name: BindingValue<String>,
    /// The Service port
    pub service_port: BindingValue<u16>,
    /// Optional public URL if container is exposed publicly
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

/// Local Docker container binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalContainerBinding {
    /// Container name/ID
    pub container_name: BindingValue<String>,
    /// Internal URL (Docker network DNS, e.g., "http://api.svc:8080")
    pub internal_url: BindingValue<String>,
    /// Optional public URL (localhost with mapped port, e.g., "http://localhost:62844")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

impl ContainerBinding {
    /// Creates a Horizon container binding
    pub fn horizon(
        container_name: impl Into<BindingValue<String>>,
        internal_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Horizon(HorizonContainerBinding {
            container_name: container_name.into(),
            internal_url: internal_url.into(),
            public_url: None,
        })
    }

    /// Creates a Horizon container binding with public URL
    pub fn horizon_with_public_url(
        container_name: impl Into<BindingValue<String>>,
        internal_url: impl Into<BindingValue<String>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Horizon(HorizonContainerBinding {
            container_name: container_name.into(),
            internal_url: internal_url.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Creates a local Docker container binding
    pub fn local(
        container_name: impl Into<BindingValue<String>>,
        internal_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Local(LocalContainerBinding {
            container_name: container_name.into(),
            internal_url: internal_url.into(),
            public_url: None,
        })
    }

    /// Creates a local Docker container binding with public URL
    pub fn local_with_public_url(
        container_name: impl Into<BindingValue<String>>,
        internal_url: impl Into<BindingValue<String>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Local(LocalContainerBinding {
            container_name: container_name.into(),
            internal_url: internal_url.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Creates a Kubernetes container binding
    pub fn kubernetes(
        name: impl Into<BindingValue<String>>,
        namespace: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        service_port: impl Into<BindingValue<u16>>,
    ) -> Self {
        Self::Kubernetes(KubernetesContainerBinding {
            name: name.into(),
            namespace: namespace.into(),
            service_name: service_name.into(),
            service_port: service_port.into(),
            public_url: None,
        })
    }

    /// Creates a Kubernetes container binding with public URL
    pub fn kubernetes_with_public_url(
        name: impl Into<BindingValue<String>>,
        namespace: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        service_port: impl Into<BindingValue<u16>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Kubernetes(KubernetesContainerBinding {
            name: name.into(),
            namespace: namespace.into(),
            service_name: service_name.into(),
            service_port: service_port.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Gets the internal URL for any platform
    /// For Kubernetes, constructs the cluster-local DNS name
    pub fn get_internal_url(&self) -> Option<String> {
        match self {
            ContainerBinding::Horizon(binding) => {
                // Extract value if it's a concrete value, not a template expression
                if let BindingValue::Value(url) = &binding.internal_url {
                    Some(url.clone())
                } else {
                    None
                }
            }
            ContainerBinding::Kubernetes(binding) => {
                // Construct cluster-local DNS name from components
                if let (
                    BindingValue::Value(service_name),
                    BindingValue::Value(namespace),
                    BindingValue::Value(port),
                ) = (
                    &binding.service_name,
                    &binding.namespace,
                    &binding.service_port,
                ) {
                    Some(format!(
                        "http://{}.{}.svc.cluster.local:{}",
                        service_name, namespace, port
                    ))
                } else {
                    None
                }
            }
            ContainerBinding::Local(binding) => {
                if let BindingValue::Value(url) = &binding.internal_url {
                    Some(url.clone())
                } else {
                    None
                }
            }
        }
    }

    /// Gets the public URL if available for any platform
    pub fn get_public_url(&self) -> Option<&BindingValue<String>> {
        match self {
            ContainerBinding::Horizon(binding) => binding.public_url.as_ref(),
            ContainerBinding::Kubernetes(binding) => binding.public_url.as_ref(),
            ContainerBinding::Local(binding) => binding.public_url.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizon_binding() {
        let binding = ContainerBinding::horizon("api", "http://api.svc:8080");

        if let ContainerBinding::Horizon(horizon_binding) = binding {
            assert_eq!(
                horizon_binding.container_name,
                BindingValue::Value("api".to_string())
            );
            assert_eq!(
                horizon_binding.internal_url,
                BindingValue::Value("http://api.svc:8080".to_string())
            );
            assert!(horizon_binding.public_url.is_none());
        } else {
            panic!("Expected Horizon binding");
        }
    }

    #[test]
    fn test_horizon_binding_with_public_url() {
        let binding = ContainerBinding::horizon_with_public_url(
            "api",
            "http://api.svc:8080",
            "https://api.example.com",
        );

        if let ContainerBinding::Horizon(horizon_binding) = binding {
            assert_eq!(
                horizon_binding.public_url,
                Some(BindingValue::Value("https://api.example.com".to_string()))
            );
        } else {
            panic!("Expected Horizon binding");
        }
    }

    #[test]
    fn test_local_binding() {
        let binding = ContainerBinding::local("my-container", "http://my-container.svc:8080");

        if let ContainerBinding::Local(local_binding) = binding {
            assert_eq!(
                local_binding.container_name,
                BindingValue::Value("my-container".to_string())
            );
            assert_eq!(
                local_binding.internal_url,
                BindingValue::Value("http://my-container.svc:8080".to_string())
            );
            assert!(local_binding.public_url.is_none());
        } else {
            panic!("Expected Local binding");
        }
    }

    #[test]
    fn test_local_binding_with_public_url() {
        let binding = ContainerBinding::local_with_public_url(
            "my-container",
            "http://my-container.svc:8080",
            "http://localhost:62844",
        );

        if let ContainerBinding::Local(local_binding) = binding {
            assert_eq!(
                local_binding.public_url,
                Some(BindingValue::Value("http://localhost:62844".to_string()))
            );
        } else {
            panic!("Expected Local binding");
        }
    }

    #[test]
    fn test_get_internal_url() {
        let horizon = ContainerBinding::horizon("api", "http://api.svc:8080");
        assert_eq!(
            horizon.get_internal_url(),
            Some("http://api.svc:8080".to_string())
        );

        let local = ContainerBinding::local("api", "http://api.svc:3000");
        assert_eq!(
            local.get_internal_url(),
            Some("http://api.svc:3000".to_string())
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let binding = ContainerBinding::horizon_with_public_url(
            "api",
            "http://api.svc:8080",
            "https://api.example.com",
        );

        let serialized = serde_json::to_string(&binding).unwrap();
        let deserialized: ContainerBinding = serde_json::from_str(&serialized).unwrap();
        assert_eq!(binding, deserialized);
    }
}

use crate::{
    error::{ErrorData, Result},
    traits::{Binding, Container},
};
use alien_core::bindings::KubernetesContainerBinding;
use alien_error::Context;
use async_trait::async_trait;

/// Kubernetes Container implementation that provides URLs for container-to-container communication
#[derive(Debug)]
pub struct KubernetesContainer {
    namespace: String,
    service_name: String,
    service_port: u16,
    public_url: Option<String>,
    internal_url: String,
}

impl KubernetesContainer {
    pub fn new(binding_name: String, binding: KubernetesContainerBinding) -> Result<Self> {
        let namespace = binding
            .namespace
            .into_value(&binding_name, "namespace")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract namespace from Kubernetes container binding".to_string(),
            })?;

        let service_name = binding
            .service_name
            .into_value(&binding_name, "service_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_name from Kubernetes container binding"
                    .to_string(),
            })?;

        let service_port = binding
            .service_port
            .into_value(&binding_name, "service_port")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_port from Kubernetes container binding"
                    .to_string(),
            })?;

        let public_url = binding
            .public_url
            .map(|v| v.into_value(&binding_name, "public_url"))
            .transpose()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract public_url from Kubernetes container binding"
                    .to_string(),
            })?;

        // Construct internal service URL
        let internal_url = format!(
            "http://{}.{}.svc.cluster.local:{}",
            service_name, namespace, service_port
        );

        Ok(Self {
            namespace,
            service_name: service_name.clone(),
            service_port,
            public_url,
            internal_url,
        })
    }
}

#[async_trait]
impl Binding for KubernetesContainer {}

impl Container for KubernetesContainer {
    fn get_internal_url(&self) -> &str {
        &self.internal_url
    }

    fn get_public_url(&self) -> Option<&str> {
        self.public_url.as_deref()
    }

    fn get_container_name(&self) -> &str {
        &self.service_name
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

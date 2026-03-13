//! Horizon container binding implementation
//!
//! For containers managed by Horizon on AWS/GCP/Azure cloud platforms.

use crate::error::Result;
use crate::traits::{Binding, Container};
use alien_core::bindings::HorizonContainerBinding;
use async_trait::async_trait;

/// Horizon container binding implementation
#[derive(Debug)]
pub struct HorizonContainer {
    container_name: String,
    internal_url: String,
    public_url: Option<String>,
}

impl HorizonContainer {
    /// Create a new Horizon container binding
    pub fn new(binding: HorizonContainerBinding) -> Result<Self> {
        use crate::error::ErrorData;
        use alien_error::Context;

        let container_name = binding
            .container_name
            .into_value("container", "container_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "container".to_string(),
                reason: "Failed to resolve container_name from binding".to_string(),
            })?;

        let internal_url = binding
            .internal_url
            .into_value("container", "internal_url")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "container".to_string(),
                reason: "Failed to resolve internal_url from binding".to_string(),
            })?;

        let public_url = binding
            .public_url
            .map(|v| {
                v.into_value("container", "public_url")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: "container".to_string(),
                        reason: "Failed to resolve public_url from binding".to_string(),
                    })
            })
            .transpose()?;

        Ok(Self {
            container_name,
            internal_url,
            public_url,
        })
    }
}

impl Binding for HorizonContainer {}

#[async_trait]
impl Container for HorizonContainer {
    fn get_internal_url(&self) -> &str {
        &self.internal_url
    }

    fn get_public_url(&self) -> Option<&str> {
        self.public_url.as_deref()
    }

    fn get_container_name(&self) -> &str {
        &self.container_name
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

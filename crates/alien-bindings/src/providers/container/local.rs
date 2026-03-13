//! Local container binding implementation
//!
//! For containers running in Docker during local development.

use crate::error::Result;
use crate::traits::{Binding, Container};
use alien_core::bindings::LocalContainerBinding;
use async_trait::async_trait;

/// Local Docker container binding implementation
#[derive(Debug)]
pub struct LocalContainer {
    container_name: String,
    internal_url: String,
    public_url: Option<String>,
}

impl LocalContainer {
    /// Create a new local container binding
    pub fn new(binding: LocalContainerBinding) -> Result<Self> {
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

impl Binding for LocalContainer {}

#[async_trait]
impl Container for LocalContainer {
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

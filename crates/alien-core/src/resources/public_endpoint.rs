use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};

/// Protocol for public workload endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ExposeProtocol {
    /// HTTP/HTTPS with TLS termination at load balancer.
    Http,
    /// TCP passthrough without TLS.
    Tcp,
}

/// Public endpoint configuration shared by workload resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PublicEndpoint {
    /// Workload port served by the public endpoint.
    pub port: u16,
    /// Public protocol.
    pub protocol: ExposeProtocol,
    /// Optional DNS label override for generated endpoint hostnames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_label: Option<String>,
    /// Whether to route wildcard subdomains to this endpoint.
    #[serde(default)]
    pub wildcard_subdomains: bool,
}

impl PublicEndpoint {
    /// Returns the DNS label used for generated hostnames.
    pub fn effective_host_label<'a>(&'a self, resource_id: &'a str) -> &'a str {
        self.host_label.as_deref().unwrap_or(resource_id)
    }

    /// Validates the endpoint options for a resource.
    pub fn validate_for_resource(&self, resource_id: &str) -> Result<()> {
        if let Some(host_label) = &self.host_label {
            validate_endpoint_host_label(resource_id, host_label)?;
        }

        Ok(())
    }
}

/// Validates a single DNS label used in generated endpoint hostnames.
pub fn validate_endpoint_host_label(resource_id: &str, host_label: &str) -> Result<()> {
    let valid = !host_label.is_empty()
        && host_label.len() <= 63
        && !host_label.starts_with('-')
        && !host_label.ends_with('-')
        && host_label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');

    if !valid {
        return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
            resource_id: resource_id.to_string(),
            reason:
                "public endpoint hostLabel must be a single lowercase DNS label: letters, numbers, hyphens, no dots, and no leading or trailing hyphen"
                    .to_string(),
        }));
    }

    Ok(())
}

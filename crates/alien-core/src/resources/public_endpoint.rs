use crate::error::{ErrorData, Result};
use crate::LoadBalancerEndpoint;
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

/// Public endpoint configuration for port-backed workload resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PublicEndpoint {
    /// Endpoint name within the resource.
    pub name: String,
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
    pub fn effective_host_label(&self) -> &str {
        self.host_label.as_deref().unwrap_or(&self.name)
    }

    /// Validates the endpoint options for a resource.
    pub fn validate_for_resource(&self, resource_id: &str) -> Result<()> {
        validate_endpoint_name(resource_id, &self.name)?;
        if let Some(host_label) = &self.host_label {
            validate_endpoint_host_label(resource_id, host_label)?;
        }

        Ok(())
    }
}

/// Public endpoint configuration for Worker resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkerPublicEndpoint {
    /// Endpoint name within the resource.
    pub name: String,
    /// Optional DNS label override for generated endpoint hostnames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_label: Option<String>,
    /// Whether to route wildcard subdomains to this endpoint.
    #[serde(default)]
    pub wildcard_subdomains: bool,
}

impl WorkerPublicEndpoint {
    /// Returns the DNS label used for generated hostnames.
    pub fn effective_host_label(&self) -> &str {
        self.host_label.as_deref().unwrap_or(&self.name)
    }

    /// Validates the endpoint options for a resource.
    pub fn validate_for_resource(&self, resource_id: &str) -> Result<()> {
        validate_endpoint_name(resource_id, &self.name)?;
        if let Some(host_label) = &self.host_label {
            validate_endpoint_host_label(resource_id, host_label)?;
        }

        Ok(())
    }
}

/// Runtime-resolved public endpoint metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PublicEndpointOutput {
    /// Base URL for this endpoint.
    pub url: String,
    /// Hostname for this endpoint.
    pub host: String,
    /// Wildcard hostname routed to this endpoint, when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wildcard_host: Option<String>,
    /// Load balancer endpoint information for DNS management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_endpoint: Option<LoadBalancerEndpoint>,
}

/// Validates a public endpoint name within a resource.
pub fn validate_endpoint_name(resource_id: &str, name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');

    if !valid {
        return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
            resource_id: resource_id.to_string(),
            reason:
                "public endpoint name must be a single lowercase DNS label: letters, numbers, hyphens, no dots, and no leading or trailing hyphen"
                    .to_string(),
        }));
    }

    Ok(())
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

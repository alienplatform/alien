use crate::error::{ErrorData, Result};
use crate::LoadBalancerEndpoint;
use alien_error::AlienError;
use serde::{de, Deserialize, Deserializer, Serialize};
use url::Url;

/// Host label that places a generated public endpoint at the deployment base hostname.
pub const APEX_HOST_LABEL: &str = "@";

/// Protocol for public workload endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ExposeProtocol {
    /// HTTP/HTTPS with TLS termination at load balancer.
    #[default]
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
        if self.host_label.as_deref() == Some(APEX_HOST_LABEL) && self.wildcard_subdomains {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: resource_id.to_string(),
                reason: "an apex public endpoint cannot also route wildcard subdomains".to_string(),
            }));
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
        if self.host_label.as_deref() == Some(APEX_HOST_LABEL) && self.wildcard_subdomains {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: resource_id.to_string(),
                reason: "an apex public endpoint cannot also route wildcard subdomains".to_string(),
            }));
        }

        Ok(())
    }
}

/// Runtime-resolved public endpoint metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PublicEndpointOutput {
    /// Base URL for this endpoint.
    pub url: String,
    /// Hostname for this endpoint.
    pub host: String,
    /// Public connection protocol.
    pub protocol: ExposeProtocol,
    /// Public connection port.
    #[cfg_attr(feature = "openapi", schema(minimum = 1, maximum = 65535))]
    pub port: u16,
    /// Wildcard hostname routed to this endpoint, when configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wildcard_host: Option<String>,
    /// Load balancer endpoint information for DNS management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_endpoint: Option<LoadBalancerEndpoint>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicEndpointOutputWire {
    url: String,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    protocol: Option<ExposeProtocol>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    wildcard_host: Option<String>,
    #[serde(default)]
    load_balancer_endpoint: Option<LoadBalancerEndpoint>,
}

impl<'de> Deserialize<'de> for PublicEndpointOutput {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PublicEndpointOutputWire::deserialize(deserializer)?;
        let parsed = Url::parse(&wire.url).map_err(de::Error::custom)?;
        let protocol = wire.protocol.unwrap_or_default();
        let expected_schemes: &[&str] = match protocol {
            ExposeProtocol::Http => &["http", "https"],
            ExposeProtocol::Tcp => &["tcp"],
        };
        if !expected_schemes.contains(&parsed.scheme()) {
            return Err(de::Error::custom(format!(
                "public endpoint protocol '{protocol:?}' is inconsistent with URL scheme '{}'",
                parsed.scheme()
            )));
        }
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || (!parsed.path().is_empty() && parsed.path() != "/")
        {
            return Err(de::Error::custom(
                "public endpoint URL must not include credentials, a path, query parameters, or a fragment",
            ));
        }

        let parsed_host = parsed
            .host_str()
            .map(|host| host.trim_end_matches('.').to_string())
            .filter(|host| !host.is_empty())
            .ok_or_else(|| de::Error::custom("public endpoint URL must include a host"))?;
        if let Some(host) = &wire.host {
            if host != &parsed_host {
                return Err(de::Error::custom(format!(
                    "public endpoint host '{host}' is inconsistent with URL host '{parsed_host}'"
                )));
            }
        }

        let parsed_port = parsed.port_or_known_default().ok_or_else(|| {
            de::Error::custom("public endpoint URL must include a port for this protocol")
        })?;
        if parsed_port == 0 {
            return Err(de::Error::custom(
                "public endpoint URL port must be between 1 and 65535",
            ));
        }
        if let Some(port) = wire.port {
            if port != parsed_port {
                return Err(de::Error::custom(format!(
                    "public endpoint port '{port}' is inconsistent with URL port '{parsed_port}'"
                )));
            }
        }

        Ok(Self {
            url: wire.url,
            host: parsed_host,
            protocol,
            port: parsed_port,
            wildcard_host: wire.wildcard_host,
            load_balancer_endpoint: wire.load_balancer_endpoint,
        })
    }
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
    if host_label == APEX_HOST_LABEL {
        return Ok(());
    }

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
                "public endpoint hostLabel must be '@' for apex or a single lowercase DNS label: letters, numbers, hyphens, no dots, and no leading or trailing hyphen"
                    .to_string(),
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_label_allows_apex_marker() {
        validate_endpoint_host_label("gateway", APEX_HOST_LABEL).expect("apex host label");
    }

    #[test]
    fn public_endpoint_rejects_apex_wildcard_combination() {
        let endpoint = PublicEndpoint {
            name: "api".to_string(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: Some(APEX_HOST_LABEL.to_string()),
            wildcard_subdomains: true,
        };

        let error = endpoint
            .validate_for_resource("gateway")
            .expect_err("apex wildcard should be rejected");

        assert_eq!(error.code, "INVALID_RESOURCE_UPDATE");
        assert!(error.message.contains("apex"));
    }

    #[test]
    fn worker_endpoint_rejects_apex_wildcard_combination() {
        let endpoint = WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: Some(APEX_HOST_LABEL.to_string()),
            wildcard_subdomains: true,
        };

        let error = endpoint
            .validate_for_resource("handler")
            .expect_err("apex wildcard should be rejected");

        assert_eq!(error.code, "INVALID_RESOURCE_UPDATE");
        assert!(error.message.contains("apex"));
    }

    #[test]
    fn old_http_output_derives_current_connection_metadata() {
        let output: PublicEndpointOutput = serde_json::from_value(serde_json::json!({
            "url": "https://gateway.example.test",
            "host": "gateway.example.test"
        }))
        .expect("old HTTP output should deserialize");

        assert_eq!(output.protocol, ExposeProtocol::Http);
        assert_eq!(output.host, "gateway.example.test");
        assert_eq!(output.port, 443);
    }

    #[test]
    fn captured_container_outputs_deserialize_through_current_contract() {
        let outputs: crate::ContainerOutputs = serde_json::from_value(serde_json::json!({
            "name": "gateway",
            "status": "running",
            "currentReplicas": 1,
            "desiredReplicas": 1,
            "internalDns": "gateway.svc",
            "replicas": [],
            "publicEndpoints": {
                "api": {
                    "url": "https://gateway.example.test",
                    "host": "gateway.example.test"
                }
            }
        }))
        .expect("captured container outputs should deserialize");

        let endpoint = &outputs.public_endpoints["api"];
        assert_eq!(endpoint.protocol, ExposeProtocol::Http);
        assert_eq!(endpoint.port, 443);
    }

    #[test]
    fn tcp_output_requires_truthful_connection_metadata() {
        let output: PublicEndpointOutput = serde_json::from_value(serde_json::json!({
            "url": "tcp://database.example.test:6432",
            "host": "database.example.test",
            "protocol": "tcp",
            "port": 6432
        }))
        .expect("TCP output should deserialize");

        assert_eq!(output.protocol, ExposeProtocol::Tcp);
        assert_eq!(output.port, 6432);
    }

    #[test]
    fn inconsistent_output_is_rejected() {
        let error = serde_json::from_value::<PublicEndpointOutput>(serde_json::json!({
            "url": "https://gateway.example.test",
            "host": "other.example.test",
            "protocol": "http",
            "port": 80
        }))
        .expect_err("inconsistent output should fail");

        assert!(error.to_string().contains("inconsistent"));
    }
}

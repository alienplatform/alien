//! Bootstrap for a private worker gateway. The gateway has no AWS credentials;
//! the interface endpoint and REST API policies bound its upstream access.

use alien_error::AlienError;
use serde_json::{json, Value};

use crate::{ErrorData, Result};

/// Envoy v1.39.4, pinned to its published multi-platform manifest.
/// https://github.com/envoyproxy/envoy/releases/tag/v1.39.4
pub(super) const GATEWAY_IMAGE: &str =
    "envoyproxy/envoy@sha256:74bd4eb5629708d3fadca1a5e6b85d208f338d7bb54e06b69565905c5294cc01";
pub(super) const GATEWAY_HTTP_PORT: u16 = 8080;
pub(super) const GATEWAY_HEALTH_PORT: u16 = 8081;
pub(super) const PRIVATE_API_STAGE: &str = "private";

pub(super) struct GatewaySpec<'a> {
    pub endpoint_dns: &'a str,
    pub api_id: &'a str,
    pub domains: &'a [String],
}

pub(super) fn bootstrap(spec: GatewaySpec<'_>) -> Result<Value> {
    if !dns_name(spec.endpoint_dns)
        || spec.api_id.is_empty()
        || !spec.api_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || spec.domains.is_empty()
        || spec
            .domains
            .iter()
            .any(|domain| !dns_name(domain.strip_prefix("*.").unwrap_or(domain)))
    {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message:
                "Private gateway requires an endpoint DNS name, API ID and explicit serving domains"
                    .into(),
            resource_id: None,
        }));
    }
    let route = json!({
        "name": "private_worker", "ignore_port_in_host_matching": true,
        "virtual_hosts": [{
            "name": "worker", "domains": spec.domains,
            "routes": [{
                "match": {"prefix": "/"},
                "route": {"cluster": "private_api", "prefix_rewrite": format!("/{PRIVATE_API_STAGE}/"), "timeout": "0s"},
                "request_headers_to_add": [{
                    "header": {"key": "x-apigw-api-id", "value": spec.api_id},
                    "append_action": "OVERWRITE_IF_EXISTS_OR_ADD"
                }]
            }]
        }]
    });
    // A separate health port keeps every application path available to the worker.
    // Both listeners must be reachable only from the load balancer's security group.
    let health = json!({
        "name": "health", "virtual_hosts": [{"name": "health", "domains": ["*"],
            "routes": [{"match": {"prefix": "/"}, "direct_response": {"status": 200}}]}]
    });
    Ok(json!({"static_resources": {
        "listeners": [listener("worker", GATEWAY_HTTP_PORT, route), listener("health", GATEWAY_HEALTH_PORT, health)],
        "clusters": [{
            "name": "private_api",
            "cluster_type": {"name": "envoy.clusters.dns", "typed_config": {
                "@type": "type.googleapis.com/envoy.extensions.clusters.dns.v3.DnsCluster",
                "respect_dns_ttl": true, "dns_lookup_family": "V4_ONLY"
            }},
            "connect_timeout": "5s",
            "load_assignment": {"cluster_name": "private_api", "endpoints": [{"lb_endpoints": [{
                "endpoint": {"address": {"socket_address": {"address": spec.endpoint_dns, "port_value": 443}}}
            }]}]},
            "transport_socket": {"name": "envoy.transport_sockets.tls", "typed_config": {
                "@type": "type.googleapis.com/envoy.extensions.transport_sockets.tls.v3.UpstreamTlsContext",
                "sni": spec.endpoint_dns, "auto_sni_san_validation": true,
                "common_tls_context": {"validation_context": {
                    "trusted_ca": {"filename": "/etc/ssl/certs/ca-certificates.crt"}
                }}
            }}
        }]
    }}))
}

fn dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn listener(name: &str, port: u16, route: Value) -> Value {
    json!({
        "name": name,
        "address": {"socket_address": {"address": "0.0.0.0", "port_value": port}},
        "per_connection_buffer_limit_bytes": 16384,
        "filter_chains": [{"filters": [{
            "name": "envoy.filters.network.http_connection_manager", "typed_config": {
                "@type": "type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager",
                "stat_prefix": name, "codec_type": "AUTO", "normalize_path": false,
                "merge_slashes": false, "path_with_escaped_slashes_action": "KEEP_UNCHANGED",
                "stream_idle_timeout": "0s", "request_timeout": "0s",
                // Only the ALB can connect. Trust its forwarded HTTPS scheme.
                "use_remote_address": true, "xff_num_trusted_hops": 1,
                "route_config": route,
                "http_filters": [{"name": "envoy.filters.http.router", "typed_config": {
                    "@type": "type.googleapis.com/envoy.extensions.filters.http.router.v3.Router"
                }}]
            }
        }]}]
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    #[ignore = "requires Docker, Python 3, OpenSSL and the pinned Envoy image"]
    fn envoy_gateway_preserves_wire_behavior() {
        let domains = vec![
            "private-api.example.test".into(),
            "*.private-api.example.test".into(),
        ];
        let config = bootstrap(GatewaySpec {
            endpoint_dns: "example.execute-api.region.vpce.amazonaws.com",
            api_id: "exampleapi",
            domains: &domains,
        })
        .unwrap();
        let mut child = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/worker_gateway/envoy_contract.py"
            ))
            .arg(GATEWAY_IMAGE)
            .stdin(Stdio::piped())
            .spawn()
            .expect("start native Envoy contract");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(&config).unwrap().as_bytes())
            .unwrap();
        assert!(
            child.wait().unwrap().success(),
            "native Envoy wire contract failed"
        );
    }
}

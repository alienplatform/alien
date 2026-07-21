use super::*;
use crate::gcp::GcpClientConfig;
use alien_core::{GcpCredentials, GcpServiceOverrides};
use reqwest::Client;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

const SERVICE_ATTACHMENT: &str =
    "https://www.googleapis.com/compute/v1/projects/p-producer/regions/us-east1/serviceAttachments/sql-sa";
const STACK_SUBNET: &str =
    "https://www.googleapis.com/compute/v1/projects/p-consumer/regions/us-east1/subnetworks/stack-subnet";
const STACK_NETWORK: &str =
    "https://www.googleapis.com/compute/v1/projects/p-consumer/global/networks/stack-vpc";

/// The forwarding rule half of a Private Service Connect consumer endpoint:
/// it targets the producer's service attachment over an internal IP, in the
/// consumer's network/subnet, and is *not* a load balancer.
fn psc_consumer_forwarding_rule() -> ForwardingRule {
    ForwardingRule {
        name: Some("stack-psc-endpoint".into()),
        target: Some(SERVICE_ATTACHMENT.into()),
        ip_address: Some("10.0.0.42".into()),
        network: Some(STACK_NETWORK.into()),
        subnetwork: Some(STACK_SUBNET.into()),
        // PSC consumer endpoints are not load balancers: scheme stays unset.
        load_balancing_scheme: None,
        ..Default::default()
    }
}

/// The address half of a PSC consumer endpoint: a regional INTERNAL IP
/// reserved from the consumer's subnet.
fn psc_consumer_address() -> Address {
    Address {
        name: Some("stack-psc-ip".into()),
        address_type: Some(AddressType::Internal),
        address: Some("10.0.0.42".into()),
        subnetwork: Some(STACK_SUBNET.into()),
        ..Default::default()
    }
}

#[test]
fn psc_forwarding_rule_serializes_for_consumer_endpoint() {
    let json = serde_json::to_value(psc_consumer_forwarding_rule())
        .expect("forwarding rule should serialize");

    assert_eq!(json["name"], "stack-psc-endpoint");
    // The target is the producer service attachment — this is what makes it PSC.
    assert_eq!(json["target"], SERVICE_ATTACHMENT);
    // Internal reachability: a fixed internal IP in the consumer subnet.
    assert_eq!(json["IPAddress"], "10.0.0.42");
    assert_eq!(json["network"], STACK_NETWORK);
    assert_eq!(json["subnetwork"], STACK_SUBNET);
    // A PSC consumer endpoint must NOT carry a load-balancing scheme.
    assert!(
        json.get("loadBalancingScheme").is_none(),
        "PSC consumer endpoint must not set loadBalancingScheme, got {json:?}"
    );
    // No global-only target-proxy ports leak in.
    assert!(json.get("portRange").is_none());
    // GCP rejects IPProtocol on a service-attachment-target (PSC) rule outright, so it
    // must be omitted entirely.
    assert!(
        json.get("IPProtocol").is_none(),
        "PSC consumer endpoint must not set IPProtocol, got {json:?}"
    );
}

#[test]
fn psc_address_serializes_as_regional_internal() {
    let json = serde_json::to_value(psc_consumer_address()).expect("address should serialize");

    assert_eq!(json["name"], "stack-psc-ip");
    // Must be INTERNAL — an external address can't back a PSC endpoint.
    assert_eq!(json["addressType"], "INTERNAL");
    assert_eq!(json["address"], "10.0.0.42");
    // The internal IP is drawn from the consumer subnet.
    assert_eq!(json["subnetwork"], STACK_SUBNET);
    // No external-only fields should appear.
    assert!(json.get("networkTier").is_none());
}

#[test]
fn forwarding_rule_round_trips_through_get_response() {
    // A GET on the rule returns the same identity fields we sent on insert.
    let rule: ForwardingRule =
        serde_json::from_value(serde_json::to_value(psc_consumer_forwarding_rule()).unwrap())
            .expect("forwarding rule should deserialize");

    assert_eq!(rule.name.as_deref(), Some("stack-psc-endpoint"));
    assert_eq!(rule.target.as_deref(), Some(SERVICE_ATTACHMENT));
    assert_eq!(rule.subnetwork.as_deref(), Some(STACK_SUBNET));
    assert!(rule.load_balancing_scheme.is_none());
}

#[tokio::test]
async fn target_tcp_proxy_insert_and_delete_use_compute_rest_contract() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = observed.clone();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            let (header_end, content_length) = loop {
                let count = stream.read(&mut buffer).expect("read request");
                assert!(count > 0, "request ended before headers");
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    let header_end = end + 4;
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse().ok())
                        })
                        .unwrap_or(0);
                    break (header_end, length);
                }
            };
            while bytes.len() < header_end + content_length {
                let count = stream.read(&mut buffer).expect("read body");
                assert!(count > 0, "request ended before body");
                bytes.extend_from_slice(&buffer[..count]);
            }
            captured.lock().expect("capture lock").push(
                String::from_utf8(bytes[..header_end + content_length].to_vec())
                    .expect("request utf8"),
            );
            let body = r#"{"name":"operation-1","status":"DONE"}"#;
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body).expect("write response");
        }
    });
    let client = ComputeClient::new(
        Client::new(),
        GcpClientConfig {
            project_id: "example-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("compute".to_string(), endpoint)]),
            }),
            project_number: None,
        },
    );
    client
        .insert_target_tcp_proxy(
            TargetTcpProxy::builder()
                .name("example-proxy".to_string())
                .description("TCP proxy".to_string())
                .service(
                    "projects/example-project/global/backendServices/example-backend".to_string(),
                )
                .proxy_header("NONE".to_string())
                .build(),
        )
        .await
        .expect("insert should succeed");
    client
        .delete_target_tcp_proxy("example-proxy".to_string())
        .await
        .expect("delete should succeed");
    server.join().expect("server should finish");
    let requests = observed.lock().expect("capture lock");
    let (insert_headers, insert_body) = requests[0].split_once("\r\n\r\n").expect("insert request");
    assert!(insert_headers
        .starts_with("POST /projects/example-project/global/targetTcpProxies HTTP/1.1"));
    assert!(insert_headers
        .lines()
        .any(|line| line.eq_ignore_ascii_case("authorization: Bearer test-token")));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(insert_body).expect("insert JSON"),
        serde_json::json!({"name":"example-proxy","description":"TCP proxy","service":"projects/example-project/global/backendServices/example-backend","proxyHeader":"NONE"})
    );
    let (delete_headers, delete_body) = requests[1].split_once("\r\n\r\n").expect("delete request");
    assert!(delete_headers.starts_with(
        "DELETE /projects/example-project/global/targetTcpProxies/example-proxy HTTP/1.1"
    ));
    assert!(delete_headers
        .lines()
        .any(|line| line.eq_ignore_ascii_case("authorization: Bearer test-token")));
    assert!(delete_body.is_empty());
}

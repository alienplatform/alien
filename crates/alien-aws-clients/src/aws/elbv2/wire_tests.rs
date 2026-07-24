use super::*;
use alien_core::{AwsClientConfig, AwsCredentials, AwsServiceOverrides};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

#[tokio::test]
async fn modify_load_balancer_attributes_sends_aws_query_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let observed = Arc::new(Mutex::new(String::new()));
    let captured = observed.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).expect("read request");
            assert!(count > 0, "request ended before body");
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
                    .expect("content length");
                if bytes.len() >= header_end + length {
                    *captured.lock().expect("capture lock") =
                        String::from_utf8(bytes[header_end..header_end + length].to_vec())
                            .expect("body utf8");
                    break;
                }
            }
        }
        let body = "<ModifyLoadBalancerAttributesResponse><ModifyLoadBalancerAttributesResult><Attributes><member><Key>load_balancing.cross_zone.enabled</Key><Value>true</Value></member></Attributes></ModifyLoadBalancerAttributesResult></ModifyLoadBalancerAttributesResponse>";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
    });
    let config = AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access".to_string(),
            secret_access_key: "test-secret".to_string(),
            session_token: None,
        },
        service_overrides: Some(AwsServiceOverrides {
            endpoints: HashMap::from([("elasticloadbalancing".to_string(), endpoint)]),
        }),
    };
    Elbv2Client::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(config),
    )
    .modify_load_balancer_attributes(
        ModifyLoadBalancerAttributesRequest::builder()
            .load_balancer_arn(
                "arn:aws:elasticloadbalancing:us-east-1:123456789012:loadbalancer/net/example/abc"
                    .to_string(),
            )
            .attributes(vec![LoadBalancerAttribute {
                key: "load_balancing.cross_zone.enabled".to_string(),
                value: "true".to_string(),
            }])
            .build(),
    )
    .await
    .expect("request should succeed");
    server.join().expect("server should finish");
    let form: HashMap<String, String> =
        form_urlencoded::parse(observed.lock().expect("capture lock").as_bytes())
            .into_owned()
            .collect();
    assert_eq!(
        form.get("Action").map(String::as_str),
        Some("ModifyLoadBalancerAttributes")
    );
    assert_eq!(form.get("Version").map(String::as_str), Some("2015-12-01"));
    assert_eq!(
        form.get("Attributes.member.1.Key").map(String::as_str),
        Some("load_balancing.cross_zone.enabled")
    );
    assert_eq!(
        form.get("Attributes.member.1.Value").map(String::as_str),
        Some("true")
    );
    assert!(form
        .get("LoadBalancerArn")
        .is_some_and(|arn| arn.contains("loadbalancer/net/example/abc")));
    assert_eq!(form.len(), 5);
}

#![cfg(feature = "full-api")]

use alien_platform_api::{types, Client};
use futures::StreamExt;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[tokio::test]
async fn mixed_success_client_preserves_accepted_json_and_empty_noop() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let payload = r#"{"operationId":"op-test","physicalResourceId":"key-test"}"#;
    let server = thread::spawn(move || {
        for status in [202, 204] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let count = socket.read(&mut chunk).unwrap();
                assert!(count > 0, "client closed before completing request");
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    assert!(headers.starts_with("POST /v1/aws-key-setups HTTP/1.1"));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        let body: serde_json::Value =
                            serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                        assert_eq!(body["action"], "delete");
                        break;
                    }
                }
            }
            let body = if status == 202 { payload } else { "" };
            write!(socket, "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let client = Client::new(&format!("http://{address}"));
    let body: types::AcceptAwsKeySetupBody = serde_json::from_value(json!({
        "action": "delete", "connectedKeyDeploymentId": null,
        "nativeKeyLogicalId": "TestKey", "setupRoleArn": null, "deletionWindowDays": 7,
        "cloudFormation": {
            "stackId": "arn:aws:cloudformation:us-east-1:000000000000:stack/test/00000000-0000-0000-0000-000000000000",
            "requestId": "00000000-0000-0000-0000-000000000000", "logicalResourceId": "TestKey",
            "physicalResourceId": null, "responseUrl": "https://example.invalid/callback"
        }
    })).unwrap();
    for (status, expected_body) in [(202, payload), (204, "")] {
        let response = client
            .accept_aws_key_setup()
            .body(body.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let mut stream = response.into_inner_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(bytes, expected_body.as_bytes());
    }
    server.join().unwrap();
}

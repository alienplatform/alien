#![cfg(feature = "full-api")]

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use alien_platform_api::{types::AcceptAwsKeySetupBody, Client};
use futures::TryStreamExt;
use serde_json::json;

#[tokio::test]
async fn key_setup_accepts_json_and_empty_success_without_losing_api_errors() {
    for (status, body) in [
        (
            202,
            r#"{"operationId":"operation-example","physicalResourceId":"key-example"}"#,
        ),
        (204, ""),
        (
            403,
            r#"{"code":"FORBIDDEN","message":"Access denied","retryable":false,"internal":false,"httpStatusCode":403}"#,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()));
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let size = stream.read(&mut chunk).unwrap();
                assert_ne!(size, 0);
                request.extend_from_slice(&chunk[..size]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        assert!(headers.starts_with("POST /v1/aws-key-setups HTTP/1.1"));
                        break;
                    }
                }
            }
            write!(stream, "HTTP/1.1 {status} test\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let request: AcceptAwsKeySetupBody = serde_json::from_value(json!({
            "action": "delete",
            "connectedKeyDeploymentId": null,
            "nativeKeyLogicalId": "ExampleKey",
            "setupRoleArn": null,
            "deletionWindowDays": 7,
            "cloudFormation": {
                "stackId": "arn:aws:cloudformation:us-east-1:123456789012:stack/example/12345678",
                "requestId": "example-request",
                "logicalResourceId": "ExampleKey",
                "physicalResourceId": null,
                "responseUrl": "https://cloudformation-custom-resource-response-example.s3.amazonaws.com/example"
            }
        })).unwrap();
        let result = client.accept_aws_key_setup().body(request).send().await;
        if status == 403 {
            let error = result.err().expect("403 must retain the API error");
            assert_eq!(error.status().unwrap().as_u16(), status);
            assert!(error.to_string().contains("FORBIDDEN"));
        } else {
            let response = result.unwrap();
            assert_eq!(response.status().as_u16(), status);
            let received = response
                .into_inner_stream()
                .try_fold(Vec::new(), |mut bytes, chunk| async move {
                    bytes.extend_from_slice(&chunk);
                    Ok(bytes)
                })
                .await
                .unwrap();
            assert_eq!(received, body.as_bytes());
        }
        server.join().unwrap();
    }
}

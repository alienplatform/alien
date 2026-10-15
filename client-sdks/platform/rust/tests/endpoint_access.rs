use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use alien_platform_api::{Client, SdkResultExt, types};
use serde_json::{Value, json};

fn endpoint_server(status: u16, response: Value) -> (Client, thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let body = loop {
            let mut chunk = [0; 4096];
            let count = stream.read(&mut chunk).unwrap();
            assert_ne!(count, 0, "request closed before its body completed");
            bytes.extend_from_slice(&chunk[..count]);
            let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            assert!(headers.starts_with(
                "PATCH /v1/deployments/dep_0c29fq4a2yjb7kx3smwdgxlcmpqr/endpoint-access?workspace=example HTTP/1.1\r\n"
            ));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if bytes.len() >= end + 4 + length {
                break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length]).unwrap();
            }
        };
        let response = response.to_string();
        write!(stream, "HTTP/1.1 {status} Result\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}", response.len()).unwrap();
        body
    });
    (client, server)
}

#[tokio::test]
async fn sends_all_access_modes_and_decodes_unchanged_results() {
    for mode in ["internet", "private", "both"] {
        let (client, server) =
            endpoint_server(200, json!({"outcome":"unchanged","operation":null}));
        let body: types::UpdateDeploymentEndpointAccessRequest =
            serde_json::from_value(json!({"endpointAccess":mode})).unwrap();
        let response = client
            .update_deployment_endpoint_access()
            .id("dep_0c29fq4a2yjb7kx3smwdgxlcmpqr")
            .workspace("example")
            .body(body)
            .send()
            .await
            .unwrap()
            .into_inner();
        assert_eq!(server.join().unwrap(), json!({"endpointAccess":mode}));
        assert!(response.operation.0.is_none());
        assert_eq!(response.outcome.to_string(), "unchanged");
    }
}

#[tokio::test]
async fn preserves_endpoint_access_authorization_errors() {
    let (client, server) = endpoint_server(
        403,
        json!({
            "code":"DEPLOYMENT_RUNTIME_CONFIG_UPDATE_FORBIDDEN", "message":"Denied",
            "retryable":false, "internal":false, "httpStatusCode":403,
            "requestId":"00000000-0000-4000-8000-000000000000"
        }),
    );
    let body: types::UpdateDeploymentEndpointAccessRequest =
        serde_json::from_value(json!({"endpointAccess":"both"})).unwrap();
    let error = client
        .update_deployment_endpoint_access()
        .id("dep_0c29fq4a2yjb7kx3smwdgxlcmpqr")
        .workspace("example")
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .unwrap_err();
    assert_eq!(server.join().unwrap(), json!({"endpointAccess":"both"}));
    assert_eq!(error.code, "DEPLOYMENT_RUNTIME_CONFIG_UPDATE_FORBIDDEN");
    assert_eq!(error.http_status_code, Some(403));
    assert!(!error.retryable);
}

#[tokio::test]
async fn connection_info_keeps_public_and_private_routes_separate() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0; 4096];
            let count = stream.read(&mut chunk).unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&chunk[..count]);
        }
        assert!(std::str::from_utf8(&request).unwrap().starts_with(
            "GET /v1/deployments/dep_0c29fq4a2yjb7kx3smwdgxlcmpqr/info?workspace=example HTTP/1.1\r\n"
        ));
        let body = json!({
            "arc":{"url":"https://manager.example.com","deploymentId":"dep_0c29fq4a2yjb7kx3smwdgxlcmpqr"},
            "status":"running", "platform":"aws",
            "resources":{"service":{"type":"daemon",
                "publicEndpoints":{"api":{"url":"tcp://public.example.com:7000","protocol":"tcp","host":"public.example.com","port":7000}},
                "privateEndpoints":{"api":{"url":"tcp://private.example.com:7000","protocol":"tcp","host":"private.example.com","port":7000}}
            }}
        });
        let encoded = body.to_string();
        write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{encoded}", encoded.len()).unwrap();
        body
    });
    let response = client
        .get_deployment_connection_info()
        .id("dep_0c29fq4a2yjb7kx3smwdgxlcmpqr")
        .workspace("example")
        .send()
        .await
        .unwrap()
        .into_inner();
    let expected = server.join().unwrap();
    let actual = serde_json::to_value(response).unwrap();
    assert_eq!(
        actual["resources"]["service"]["publicEndpoints"],
        expected["resources"]["service"]["publicEndpoints"]
    );
    assert_eq!(
        actual["resources"]["service"]["privateEndpoints"],
        expected["resources"]["service"]["privateEndpoints"]
    );
}

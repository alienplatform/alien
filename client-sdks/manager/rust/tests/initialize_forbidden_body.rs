//! A refused `POST /v1/initialize` must reach the caller with the manager's own error body.

use std::io::{Read, Write};
use std::net::TcpListener;

fn serve_once(status_line: &'static str, body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let _ = stream.read(&mut buf);
        let response = format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    url
}

#[tokio::test]
async fn a_forbidden_initialize_keeps_the_server_message() {
    let body = serde_json::json!({
        "code": "FORBIDDEN",
        "message": "Caller cannot initialize a deployment in this group",
        "retryable": false,
        "internal": false,
        "httpStatusCode": 403,
    })
    .to_string();
    let url = serve_once("403 Forbidden", body);
    let client = alien_manager_api::Client::new(&url);
    let request: alien_manager_api::types::InitializeRequest =
        serde_json::from_value(serde_json::json!({
            "name": "agent-b",
            "initialDesiredRelease": "none",
        }))
        .unwrap();

    let err = client.initialize().body(request).send().await.unwrap_err();
    let err = alien_manager_api::convert_sdk_error_reading_body(err).await;

    assert_eq!(err.code, "FORBIDDEN", "{err:?}");
    assert_eq!(
        err.message, "Caller cannot initialize a deployment in this group",
        "{err:?}"
    );
}

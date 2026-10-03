// Most of the operations below are outside the filtered client the CLI builds.
#![cfg(feature = "full-api")]

use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use alien_error::{AlienError, GenericError};
use alien_platform_api::{
    types::{CompleteCommandRequestState, UpdateCommandRequestState},
    Client, SdkResultExt,
};
use chrono::Utc;
use serde_json::json;

const DEPLOYMENT_ID: &str = "dep_0c29fq4a2yjb7kx3smwdgxlcmpqr";
const COMMAND_ID: &str = "cmd_2sxjXxvOYct7IohT3ukliAzfmpqr";

/// A client whose server answers one request with the API's 503 error and closes.
fn client_of_an_unavailable_api() -> Client {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let base_url = format!("http://{}", listener.local_addr().expect("local address"));
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept the request");
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request).expect("read the request");
        let body = json!({
            "code": "SERVICE_UNAVAILABLE",
            "message": "The API cannot serve the request right now.",
            "retryable": true,
            "internal": false,
            "httpStatusCode": 503,
            "requestId": "00000000-0000-4000-8000-000000000000"
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write the response");
    });
    Client::new(&base_url)
}

// A manager keeps its commands in the API through these operations. When the API is briefly
// unable to serve one it answers 503 with its own error; the caller needs that code and its
// retryable flag, not "Unexpected response: 503".
#[tokio::test]
async fn keeps_the_api_error_when_a_command_operation_is_unavailable() {
    let failures: [(&str, Option<AlienError<GenericError>>); 6] = [
        (
            "resolveCommandTarget",
            client_of_an_unavailable_api()
                .resolve_command_target()
                .deployment_id(DEPLOYMENT_ID)
                .command("reindex")
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
        (
            "getCommand",
            client_of_an_unavailable_api()
                .get_command()
                .id(COMMAND_ID)
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
        (
            "updateCommand",
            client_of_an_unavailable_api()
                .update_command()
                .id(COMMAND_ID)
                .body_map(|body| body.state(UpdateCommandRequestState::Dispatched))
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
        (
            "dispatchCommand",
            client_of_an_unavailable_api()
                .dispatch_command()
                .id(COMMAND_ID)
                .body_map(|body| body.dispatched_at(Utc::now()))
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
        (
            "completeCommand",
            client_of_an_unavailable_api()
                .complete_command()
                .id(COMMAND_ID)
                .body_map(|body| {
                    body.state(CompleteCommandRequestState::Succeeded)
                        .completed_at(Utc::now())
                })
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
        (
            "incrementCommandAttempt",
            client_of_an_unavailable_api()
                .increment_command_attempt()
                .id(COMMAND_ID)
                .send()
                .await
                .into_sdk_error()
                .err(),
        ),
    ];

    for (operation, failure) in failures {
        let error = failure.unwrap_or_else(|| panic!("{operation}: a 503 is an error"));
        assert_eq!(error.code, "SERVICE_UNAVAILABLE", "{operation}");
        assert_eq!(
            error.message, "The API cannot serve the request right now.",
            "{operation}"
        );
        assert!(error.retryable, "{operation}");
        assert_eq!(error.http_status_code, Some(503), "{operation}");
        assert_eq!(
            error.context,
            Some(json!({ "requestId": "00000000-0000-4000-8000-000000000000" })),
            "{operation}"
        );
    }
}

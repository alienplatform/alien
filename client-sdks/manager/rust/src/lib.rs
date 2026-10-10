//! Alien Manager API
//!
//! Auto-generated from OpenAPI spec using Progenitor.
//! Provides a type-safe Rust client for the alien-manager API.
//!
//! ## Usage
//!
//! ```ignore
//! use alien_manager_api::Client;
//!
//! let client = Client::new("http://localhost:8080");
//!
//! // Create a deployment
//! let response = client
//!     .create_deployment()
//!     .body(&CreateDeploymentRequest {
//!         name: "my-deployment".into(),
//!         platform: Platform::Aws,
//!         ..Default::default()
//!     })
//!     .send()
//!     .await?;
//! ```

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

use alien_error::{AlienError, GenericError, HumanLayerPresentation};

/// Extension trait for converting manager SDK results to `AlienError`.
///
/// Async because it reads the body of an error response. The manager answers
/// every failure with an Alien error JSON body, and the spec this client is
/// generated from declares no error responses (see
/// `client-sdks/manager/scripts/fix-openapi.mjs`), so every error status
/// arrives as `Error::UnexpectedResponse` with its body unread. Reading it keeps
/// the server's code, message, hint and source instead of "Unexpected response".
pub trait SdkResultExt<T> {
    /// Convert SDK result to `AlienError` result, preserving API error details.
    fn into_sdk_error(
        self,
    ) -> impl std::future::Future<Output = Result<T, AlienError<GenericError>>> + Send;
}

impl<T: Send> SdkResultExt<ResponseValue<T>> for Result<ResponseValue<T>, Error<()>> {
    fn into_sdk_error(
        self,
    ) -> impl std::future::Future<Output = Result<ResponseValue<T>, AlienError<GenericError>>> + Send
    {
        async move {
            match self {
                Ok(response) => Ok(response),
                Err(error) => Err(convert_sdk_error(error).await),
            }
        }
    }
}

/// Convert a progenitor SDK error to `AlienError`, preserving all details.
///
/// For an error status it reads the response body, so a structured Alien
/// error returned by the manager survives the round-trip. A body that is not
/// an Alien error (a proxy's HTML page) gives an `UNEXPECTED_RESPONSE` error
/// that keeps the status, so retry decisions still work.
pub async fn convert_sdk_error(err: Error<()>) -> AlienError<GenericError> {
    match err {
        Error::UnexpectedResponse(response) => convert_unexpected_response(response).await,
        other => convert_sdk_error_without_body(other),
    }
}

async fn convert_unexpected_response(response: reqwest::Response) -> AlienError<GenericError> {
    let status = response.status();
    let url = response.url().to_string();
    let header_request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = match response.text().await {
        Ok(body) => body,
        Err(read_error) => {
            let mut error = unexpected_status_error(status, Some(&url));
            error.source = Some(Box::new(AlienError::new(GenericError {
                message: reqwest_failure_message("HTTP error response body read", &read_error),
            })));
            return error;
        }
    };

    if let Ok(mut api_error) = serde_json::from_str::<AlienError<GenericError>>(&body) {
        if api_error.http_status_code.is_none() {
            api_error.http_status_code = Some(status.as_u16());
        }
        let body_request_id = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("requestId")?.as_str().map(str::to_string));
        api_error.context = context_with_request_id(
            api_error.context,
            header_request_id.as_deref().or(body_request_id.as_deref()),
        );
        return api_error;
    }

    unexpected_status_error(status, Some(&url))
}

/// An error for an HTTP status whose body carried no Alien error.
fn unexpected_status_error(
    status: reqwest::StatusCode,
    url: Option<&str>,
) -> AlienError<GenericError> {
    let code = status.as_u16();
    let mut context = serde_json::json!({ "status": code });
    if let Some(url) = url {
        context["url"] = serde_json::Value::String(url.to_string());
    }
    AlienError {
        code: "UNEXPECTED_RESPONSE".to_string(),
        message: format!(
            "Unexpected response: {} {}",
            code,
            status.canonical_reason().unwrap_or("Unknown")
        ),
        context: Some(context),
        hint: None,
        retryable: is_retryable_http_status(code),
        internal: false,
        http_status_code: Some(code),
        source: None,
        human_layer_presentation: HumanLayerPresentation::Normal,
        error: Some(GenericError {
            message: format!("Unexpected response status: {}", code),
        }),
    }
}

fn context_with_request_id(
    context: Option<serde_json::Value>,
    request_id: Option<&str>,
) -> Option<serde_json::Value> {
    let Some(request_id) = request_id else {
        return context;
    };

    match context {
        Some(serde_json::Value::Object(mut object)) => {
            object
                .entry("requestId")
                .or_insert_with(|| serde_json::Value::String(request_id.to_string()));
            Some(serde_json::Value::Object(object))
        }
        Some(value) => Some(serde_json::json!({
            "requestId": request_id,
            "details": value,
        })),
        None => Some(serde_json::json!({ "requestId": request_id })),
    }
}

/// Returns whether an HTTP response represents a transient failure that a
/// caller may safely retry.
pub fn is_retryable_http_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429) || (500..=599).contains(&status)
}

/// Convert an SDK error that carries no unread response body.
fn convert_sdk_error_without_body(err: Error<()>) -> AlienError<GenericError> {
    match err {
        // The generation spec declares no error statuses, so this only covers a
        // body-less error type; there is no body left to read.
        Error::ErrorResponse(response) => unexpected_status_error(response.status(), None),
        Error::CommunicationError(reqwest_err) => {
            let retryable =
                reqwest_err.is_connect() || reqwest_err.is_timeout() || reqwest_err.is_request();
            let message = reqwest_failure_message("HTTP request", &reqwest_err);

            AlienError {
                code: "COMMUNICATION_ERROR".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
        Error::InvalidRequest(msg) => AlienError {
            code: "INVALID_REQUEST".to_string(),
            message: format!("Invalid Request: {}", msg),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: Some(400),
            source: None,
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError {
                message: format!("Invalid Request: {}", msg),
            }),
        },
        Error::ResponseBodyError(reqwest_err) => {
            let message = reqwest_failure_message("HTTP response body read", &reqwest_err);

            AlienError {
                code: "RESPONSE_BODY_ERROR".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable: true,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
        Error::InvalidResponsePayload(bytes, json_err) => {
            AlienError {
                code: "INVALID_RESPONSE_PAYLOAD".to_string(),
                message: format!("Failed to parse response: {}", json_err),
                context: Some(serde_json::json!({
                    "parseError": json_err.to_string(),
                    // Manager responses can contain short-lived credentials.
                    // Preserve enough metadata to diagnose truncation or
                    // schema drift without copying response bytes into errors.
                    "responseBodyLength": bytes.len(),
                })),
                hint: None,
                retryable: false,
                internal: false,
                http_status_code: None,
                source: Some(Box::new(AlienError::new(GenericError {
                    message: json_err.to_string(),
                }))),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError {
                    message: format!("Failed to parse response: {}", json_err),
                }),
            }
        }
        Error::InvalidUpgrade(reqwest_err) => {
            let message = reqwest_failure_message("HTTP connection upgrade", &reqwest_err);

            AlienError {
                code: "INVALID_UPGRADE".to_string(),
                message: message.clone(),
                context: reqwest_failure_context(&reqwest_err),
                hint: None,
                retryable: false,
                internal: false,
                http_status_code: reqwest_err.status().map(|s| s.as_u16()),
                source: build_reqwest_source(&reqwest_err),
                human_layer_presentation: HumanLayerPresentation::Normal,
                error: Some(GenericError { message }),
            }
        }
        Error::UnexpectedResponse(response) => {
            unexpected_status_error(response.status(), Some(response.url().as_str()))
        }
        Error::Custom(msg) => AlienError {
            code: "SDK_HOOK_ERROR".to_string(),
            message: msg.clone(),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: None,
            human_layer_presentation: HumanLayerPresentation::Normal,
            error: Some(GenericError { message: msg }),
        },
    }
}

fn reqwest_failure_message(operation: &str, err: &reqwest::Error) -> String {
    match err.url() {
        Some(url) => format!("{operation} {} failed: {err}", url),
        None => format!("{operation} failed: {err}"),
    }
}

fn reqwest_failure_context(err: &reqwest::Error) -> Option<serde_json::Value> {
    err.url().map(|url| {
        serde_json::json!({
            "url": url.to_string(),
        })
    })
}

fn build_reqwest_source(reqwest_err: &reqwest::Error) -> Option<Box<AlienError<GenericError>>> {
    use std::error::Error as _;

    reqwest_err.source().map(|source| {
        Box::new(AlienError {
            code: "GENERIC_ERROR".to_string(),
            message: source.to_string(),
            context: None,
            hint: None,
            retryable: false,
            internal: false,
            http_status_code: None,
            source: None,
            human_layer_presentation: HumanLayerPresentation::Transparent,
            error: Some(GenericError {
                message: source.to_string(),
            }),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn unexpected_response<E>(status: u16, body: &str) -> Error<E> {
        let response = http::Response::builder()
            .status(status)
            .body(body.to_string())
            .expect("test response should build");
        Error::UnexpectedResponse(reqwest::Response::from(response))
    }

    #[tokio::test]
    async fn reading_body_preserves_structured_alien_errors() {
        let body = serde_json::json!({
            "code": "PUBLIC_SUBDOMAIN_REQUIRES_CUSTOM_DOMAIN",
            "message": "Choosing a public subdomain requires a custom project domain",
            "hint": "Configure a custom domain first",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 400,
            "requestId": "req_body_123",
        })
        .to_string();

        let error = convert_sdk_error(unexpected_response(400, &body)).await;

        assert_eq!(error.code, "PUBLIC_SUBDOMAIN_REQUIRES_CUSTOM_DOMAIN");
        assert_eq!(
            error.message,
            "Choosing a public subdomain requires a custom project domain"
        );
        assert_eq!(error.http_status_code, Some(400));
        assert_eq!(
            error.hint.as_deref(),
            Some("Configure a custom domain first")
        );
        assert_eq!(error.context.as_ref().unwrap()["requestId"], "req_body_123");
        assert!(!error.retryable);
        assert!(!error.internal);
    }

    #[tokio::test]
    async fn reading_body_falls_back_to_generic_error_for_non_alien_payloads() {
        let error = convert_sdk_error(unexpected_response(502, "<html>bad gateway</html>")).await;

        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert_eq!(error.message, "Unexpected response: 502 Bad Gateway");
        assert_eq!(error.http_status_code, Some(502));
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn reading_body_classifies_unstructured_rate_limits_as_retryable() {
        let error = convert_sdk_error(unexpected_response(429, "rate limited")).await;

        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert_eq!(error.http_status_code, Some(429));
        assert!(error.retryable);
    }

    /// Serves exactly one HTTP response on a loopback port and returns its base URL.
    /// `head` is the status line plus any extra header lines, each ending in CRLF.
    fn serve_one_response(head: &str, body: &str) -> (String, std::thread::JoinHandle<()>) {
        let response = format!(
            "{head}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("test server should bind to a loopback port");
        let address = listener
            .local_addr()
            .expect("test server should have a local address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener
                .accept()
                .expect("test server should accept the SDK request");
            let mut request = [0_u8; 4096];
            stream
                .read(&mut request)
                .expect("test server should read the SDK request");
            stream
                .write_all(response.as_bytes())
                .expect("test server should write its response");
        });
        (format!("http://{address}"), server)
    }

    #[tokio::test]
    async fn generated_endpoint_preserves_malformed_server_error_status() {
        let (base_url, server) = serve_one_response(
            "HTTP/1.1 500 Internal Server Error\r\ncontent-type: text/html\r\n",
            "upstream exploded",
        );

        let error = Client::new(&base_url)
            .resolve_binding()
            .body(types::ResolveBindingRequest {
                deployment_id: "dep_test".to_string(),
                kind: None,
                resource_id: Some("storage".to_string()),
            })
            .send()
            .await
            .into_sdk_error()
            .await
            .expect_err("the generated SDK should return the server error");
        server.join().expect("test server should stop cleanly");

        assert_eq!(error.code, "UNEXPECTED_RESPONSE");
        assert_eq!(error.http_status_code, Some(500));
        assert!(error.retryable);
    }

    /// Every manager route answers failures with an Alien error body. The
    /// generated client must hand that error to callers for any status, whether
    /// or not the route's OpenAPI annotation lists it: GET /v1/deployments/{id}
    /// lists 404 but not 400.
    #[tokio::test]
    async fn generated_endpoint_returns_the_server_error_for_listed_and_unlisted_statuses() {
        let (base_url, server) = serve_one_response(
            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nx-request-id: req_400\r\n",
            concat!(
            r#"{"code":"DEPLOYMENT_CONTEXT_UNAVAILABLE","message":"Failed to load deployment context","retryable":false,"internal":false,"httpStatusCode":400,"#,
            r#""source":{"code":"HOSTNAME_CONFLICT","message":"Hostname 'a.example.com' is used by 'd1' and 'd2'","retryable":false,"internal":false,"httpStatusCode":400}}"#,
            ),
        );
        let error = Client::new(&base_url)
            .get_deployment()
            .id("dep_test")
            .send()
            .await
            .into_sdk_error()
            .await
            .expect_err("a 400 should be an error");
        server.join().expect("test server should stop cleanly");

        assert_eq!(error.code, "DEPLOYMENT_CONTEXT_UNAVAILABLE");
        assert_eq!(error.message, "Failed to load deployment context");
        assert_eq!(error.http_status_code, Some(400));
        assert!(!error.retryable);
        assert_eq!(error.context.as_ref().unwrap()["requestId"], "req_400");
        let source = error
            .source
            .as_ref()
            .expect("the source error should survive");
        assert_eq!(source.code, "HOSTNAME_CONFLICT");
        assert_eq!(
            source.message,
            "Hostname 'a.example.com' is used by 'd1' and 'd2'"
        );

        let (base_url, server) = serve_one_response(
            "HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\n",
            r#"{"code":"DEPLOYMENT_NOT_FOUND","message":"Deployment 'dep_test' not found","retryable":false,"internal":false,"httpStatusCode":404}"#,
        );
        let error = Client::new(&base_url)
            .get_deployment()
            .id("dep_test")
            .send()
            .await
            .into_sdk_error()
            .await
            .expect_err("a 404 should be an error");
        server.join().expect("test server should stop cleanly");

        assert_eq!(error.code, "DEPLOYMENT_NOT_FOUND");
        assert_eq!(error.message, "Deployment 'dep_test' not found");
        assert_eq!(error.http_status_code, Some(404));
    }

    #[test]
    fn retryable_http_statuses_are_limited_to_transient_failures() {
        for status in [408, 425, 429, 500, 502, 503, 504, 599] {
            assert!(
                is_retryable_http_status(status),
                "status {status} should be retryable"
            );
        }
        for status in [400, 401, 403, 404, 409, 422, 600] {
            assert!(
                !is_retryable_http_status(status),
                "status {status} should not be retryable"
            );
        }
    }

    #[tokio::test]
    async fn communication_error_includes_url_in_message_and_context() {
        let reqwest_err = reqwest::Client::new()
            .get("http://127.0.0.1:9/v1/initialize")
            .send()
            .await
            .expect_err("localhost discard port should refuse the connection");

        let error = super::convert_sdk_error(Error::CommunicationError(reqwest_err)).await;

        assert_eq!(error.code, "COMMUNICATION_ERROR");
        assert!(error
            .message
            .starts_with("HTTP request http://127.0.0.1:9/v1/initialize failed:"));
        assert_eq!(
            error.context.as_ref().unwrap()["url"],
            "http://127.0.0.1:9/v1/initialize"
        );
    }

    #[test]
    fn invalid_success_payload_never_copies_response_credentials_into_errors() {
        let body = br#"{"accessToken":"sensitive-token","unexpected":true}"#.to_vec();
        let parse_error = serde_json::from_slice::<serde_json::Value>(b"{")
            .expect_err("fixture JSON should be invalid");
        let error = super::convert_sdk_error_without_body(Error::InvalidResponsePayload(
            body.clone().into(),
            parse_error,
        ));
        let rendered = format!("{error:?}");

        assert_eq!(error.code, "INVALID_RESPONSE_PAYLOAD");
        assert_eq!(
            error.context.as_ref().unwrap()["responseBodyLength"],
            body.len()
        );
        assert!(!rendered.contains("sensitive-token"));
        assert!(error
            .context
            .as_ref()
            .unwrap()
            .get("responseBody")
            .is_none());
    }
}

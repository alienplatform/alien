use std::any::Any;
use std::fmt::Debug;

use alien_error::{Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;

use crate::{error::Result, types::Envelope};

/// Trait for dispatching command envelopes to agents via platform-specific transport
#[async_trait]
pub trait CommandDispatcher: Send + Sync + Debug {
    /// Dispatch an envelope to the target agent
    async fn dispatch(&self, envelope: &Envelope) -> Result<()>;

    /// Helper method for downcasting to concrete types in tests
    fn as_any(&self) -> &dyn Any;
}

/// No-op command dispatcher that succeeds without doing anything
#[derive(Debug)]
pub struct NullCommandDispatcher;

#[async_trait]
impl CommandDispatcher for NullCommandDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
        tracing::debug!(
            command_id = %envelope.command_id,
            command = %envelope.command,
            "NullCommandDispatcher: no-op dispatch"
        );
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(any(feature = "server", feature = "dispatchers"))]
mod platform_dispatchers {
    use super::*;
    use alien_aws_clients::aws::{
        lambda::{InvocationType, InvokeRequest, LambdaApi, LambdaClient},
        AwsClientConfig,
    };
    use alien_aws_clients::AwsCredentialProvider;
    use alien_azure_clients::azure::{
        service_bus::{
            AzureServiceBusDataPlaneClient, SendMessageParameters, ServiceBusDataPlaneApi,
        },
        token_cache::AzureTokenCache,
        AzureClientConfig,
    };
    use alien_client_core::{
        redact_request_body, Error as CloudClientError, ErrorData as CloudClientErrorData,
    };
    use alien_gcp_clients::gcp::{
        pubsub::{PubSubApi, PubSubClient, PublishRequest, PubsubMessage},
        GcpClientConfig,
    };
    use base64::prelude::*;
    use reqwest::Client;
    use std::collections::HashMap;
    use std::time::Duration;

    const HTTP_COMMAND_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// Provider responses that unambiguously reject the current request.
    ///
    /// Timeouts and server errors are deliberately excluded: the provider may
    /// have accepted the command before the acknowledgement was lost. Keep the
    /// allowlist narrow instead of treating every 4xx as proof of non-delivery.
    fn is_definite_cloud_rejection_status(status: u16) -> bool {
        matches!(status, 400 | 401 | 403 | 404 | 409 | 413 | 415 | 422 | 429)
    }

    /// Service Bus obtains credentials and builds the request before sending
    /// it. Those local failures, plus an explicit allowlisted HTTP response,
    /// prove that the queue did not accept this command.
    fn is_definite_service_bus_rejection(error: &CloudClientError) -> bool {
        match error.error.as_ref() {
            Some(
                CloudClientErrorData::AuthenticationError { .. }
                | CloudClientErrorData::InvalidClientConfig { .. }
                | CloudClientErrorData::SerializationError { .. },
            ) => true,
            Some(CloudClientErrorData::HttpResponseError { http_status, .. }) => {
                is_definite_cloud_rejection_status(*http_status)
            }
            _ => false,
        }
    }

    fn http_status_from_context(context: Option<&serde_json::Value>) -> Option<u16> {
        context
            .and_then(serde_json::Value::as_object)
            .and_then(|fields| {
                fields
                    .get("http_status")
                    .or_else(|| fields.get("httpStatus"))
            })
            .and_then(serde_json::Value::as_u64)
            .and_then(|status| u16::try_from(status).ok())
    }

    /// Find the provider HTTP status before erasing response-derived context.
    fn command_provider_http_status(error: &CloudClientError) -> Option<u16> {
        if let Some(CloudClientErrorData::HttpResponseError { http_status, .. }) =
            error.error.as_ref()
        {
            return Some(*http_status);
        }
        if let Some(status) = http_status_from_context(error.context.as_ref()) {
            return Some(status);
        }

        let mut layer = error.source.as_deref();
        while let Some(source) = layer {
            if let Some(status) = http_status_from_context(source.context.as_ref()) {
                return Some(status);
            }
            layer = source.source.as_deref();
        }
        None
    }

    /// Remove all provider request/response-derived details from a command
    /// dispatch error while retaining machine-readable error codes and the
    /// provider HTTP status. Provider and proxy response bodies can reflect the
    /// submitted envelope, including inline params and signed URLs, so keeping
    /// only the category and numeric status is the safe command boundary.
    fn scrub_command_provider_error(mut error: CloudClientError) -> CloudClientError {
        let provider_status = command_provider_http_status(&error);
        error.message = provider_status.map_or_else(
            || format!("Cloud provider request failed ({})", error.code),
            |status| format!("Cloud provider request returned HTTP {status}"),
        );
        error.context = provider_status.map(|status| {
            serde_json::json!({
                "http_status": status,
            })
        });
        error.hint = None;
        error.error = None;

        let mut layer = error.source.as_deref_mut();
        while let Some(source) = layer {
            source.message = format!("Cloud provider error ({})", source.code);
            source.context = None;
            source.hint = None;
            source.error = None;
            layer = source.source.as_deref_mut();
        }

        error
    }

    /// HTTP command dispatcher used by Local and Kubernetes Workers.
    ///
    /// The target is the runtime-owned command endpoint, not an application
    /// route. The deployment token authenticates the operator/manager relay to
    /// the Worker runtime.
    pub struct HttpCommandDispatcher {
        client: Client,
        target_url: String,
        token: String,
        request_timeout: Duration,
    }

    impl Debug for HttpCommandDispatcher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("HttpCommandDispatcher")
                .field("target_url", &self.target_url)
                .field("token", &"[REDACTED]")
                .finish()
        }
    }

    impl HttpCommandDispatcher {
        pub fn new(client: Client, target_url: String, token: String) -> Self {
            Self {
                client,
                target_url,
                token,
                request_timeout: HTTP_COMMAND_REQUEST_TIMEOUT,
            }
        }

        #[cfg(test)]
        pub(crate) fn with_request_timeout(mut self, request_timeout: Duration) -> Self {
            self.request_timeout = request_timeout;
            self
        }
    }

    #[async_trait]
    impl CommandDispatcher for HttpCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            let response = match self
                .client
                .post(&self.target_url)
                .bearer_auth(&self.token)
                .json(envelope)
                .timeout(self.request_timeout)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    // Builder/connect failures happen before the runtime can
                    // accept the envelope. Timeouts and other request errors
                    // are ambiguous: the runtime may have returned 202 after
                    // the acknowledgement path was lost.
                    let definite_non_delivery = error.is_builder() || error.is_connect();
                    let error = error.without_url();
                    let context = if definite_non_delivery {
                        crate::ErrorData::TransportDispatchRejected {
                            message: "Worker runtime was unreachable before command delivery"
                                .to_string(),
                            transport_type: Some("http".to_string()),
                            target: Some(envelope.command_id.clone()),
                        }
                    } else {
                        crate::ErrorData::TransportDispatchFailed {
                            message: "Worker runtime acknowledgement was not received".to_string(),
                            transport_type: Some("http".to_string()),
                            target: Some(envelope.command_id.clone()),
                        }
                    };
                    return Err(error.into_alien_error().context(context));
                }
            };

            // The runtime contract returns 202 only after validation,
            // duplicate suppression, and tracked-task acceptance. A legacy
            // application route returning another 2xx is not delivery.
            if response.status() != reqwest::StatusCode::ACCEPTED {
                return Err(alien_error::AlienError::new(
                    crate::ErrorData::TransportDispatchRejected {
                        message: format!(
                            "Worker runtime rejected command push with HTTP {}",
                            response.status()
                        ),
                        transport_type: Some("http".to_string()),
                        target: Some(envelope.command_id.clone()),
                    },
                ));
            }

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                target_url = %self.target_url,
                "Successfully pushed command envelope to Worker runtime"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// AWS Lambda command dispatcher using InvokeFunction API
    #[derive(Debug)]
    pub struct LambdaCommandDispatcher {
        lambda_client: LambdaClient,
        function_name: String,
    }

    impl LambdaCommandDispatcher {
        pub async fn new(
            client: Client,
            config: AwsClientConfig,
            function_name: String,
        ) -> Result<Self> {
            let credentials = AwsCredentialProvider::from_config(config)
                .await
                .into_alien_error()
                .context(crate::ErrorData::TransportDispatchFailed {
                    message: "Failed to create AWS credential provider".to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: None,
                })?;
            Ok(Self {
                lambda_client: LambdaClient::new(client, credentials),
                function_name,
            })
        }
    }

    #[async_trait]
    impl CommandDispatcher for LambdaCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON payload
            let payload = serde_json::to_vec(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchRejected {
                    message: "Failed to serialize command envelope before Lambda dispatch"
                        .to_string(),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            let function_name = self.function_name.clone();

            // Use async invocation to send the envelope to the Lambda function
            // The Lambda function should have alien-worker-runtime configured to handle command envelopes
            let invoke_request = InvokeRequest::builder()
                .function_name(function_name.clone())
                .invocation_type(InvocationType::Event) // Async invocation
                .payload(payload)
                .build();

            let invoke_response = self.lambda_client.invoke(invoke_request).await.context(
                crate::ErrorData::TransportDispatchFailed {
                    message: format!("Failed to invoke Lambda function {}", function_name),
                    transport_type: Some("lambda".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // AWS Lambda's Event invocation contract acknowledges queueing
            // with exactly 202. The client intentionally exposes other HTTP
            // statuses in InvokeResponse, so classify them here without
            // changing the general-purpose Lambda client API.
            if invoke_response.status_code != reqwest::StatusCode::ACCEPTED.as_u16() {
                let context = if is_definite_cloud_rejection_status(invoke_response.status_code) {
                    crate::ErrorData::TransportDispatchRejected {
                        message: format!(
                            "Lambda rejected asynchronous invocation with HTTP {}",
                            invoke_response.status_code
                        ),
                        transport_type: Some("lambda".to_string()),
                        target: Some(envelope.command_id.clone()),
                    }
                } else {
                    crate::ErrorData::TransportDispatchFailed {
                        message: format!(
                            "Lambda asynchronous invocation acknowledgement was HTTP {} instead of 202",
                            invoke_response.status_code
                        ),
                        transport_type: Some("lambda".to_string()),
                        target: Some(envelope.command_id.clone()),
                    }
                };
                return Err(alien_error::AlienError::new(context));
            }

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                function_name = %function_name,
                "Successfully dispatched command envelope to Lambda function"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// GCP Pub/Sub command dispatcher
    #[derive(Debug)]
    pub struct PubSubCommandDispatcher {
        pubsub_client: PubSubClient,
        #[allow(dead_code)]
        project_id: String,
        topic_id: String,
    }

    impl PubSubCommandDispatcher {
        pub fn new(client: Client, config: GcpClientConfig, topic_id: String) -> Self {
            let project_id = config.project_id.clone();
            Self {
                pubsub_client: PubSubClient::new(client, config),
                project_id,
                topic_id,
            }
        }
    }

    #[async_trait]
    impl CommandDispatcher for PubSubCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchRejected {
                    message: "Failed to serialize command envelope before Pub/Sub dispatch"
                        .to_string(),
                    transport_type: Some("pubsub".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            // Base64 encode the JSON payload as required by Pub/Sub
            let data = BASE64_STANDARD.encode(envelope_json.as_bytes());

            let topic_id = self.topic_id.clone();

            // Create the Pub/Sub message with command envelope metadata
            let mut attributes = HashMap::new();
            attributes.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            attributes.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            attributes.insert("cmd-command".to_string(), envelope.command.clone());

            let message = PubsubMessage::builder()
                .data(data)
                .attributes(attributes)
                .build();

            let publish_request = PublishRequest::builder().messages(vec![message]).build();

            // Pub/Sub's generic client retries internally. A final 4xx does
            // not prove an earlier timed-out attempt was not accepted, so all
            // provider-call errors remain ambiguous. The canonical scrub must
            // run before adding a non-internal command error layer because the
            // captured request contains the base64 command envelope.
            let publish_result = redact_request_body(
                self.pubsub_client
                    .publish(topic_id.clone(), publish_request)
                    .await,
            );
            if let Err(error) = publish_result {
                return Err(scrub_command_provider_error(error).context(
                    crate::ErrorData::TransportDispatchFailed {
                        message: format!("Failed to publish to Pub/Sub topic {}", topic_id),
                        transport_type: Some("pubsub".to_string()),
                        target: Some(envelope.command_id.clone()),
                    },
                ));
            }

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                topic_id = %topic_id,
                "Successfully dispatched command envelope to Pub/Sub topic"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// Azure Service Bus command dispatcher
    #[derive(Debug)]
    pub struct ServiceBusCommandDispatcher {
        servicebus_client: AzureServiceBusDataPlaneClient,
        namespace_name: String,
        queue_name: String,
    }

    impl ServiceBusCommandDispatcher {
        pub fn new(
            client: Client,
            config: AzureClientConfig,
            namespace_name: String,
            queue_name: String,
        ) -> Self {
            Self {
                servicebus_client: AzureServiceBusDataPlaneClient::new(
                    client,
                    AzureTokenCache::new(config),
                ),
                namespace_name,
                queue_name,
            }
        }
    }

    #[async_trait]
    impl CommandDispatcher for ServiceBusCommandDispatcher {
        async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
            // Serialize the command envelope as JSON
            let envelope_json = serde_json::to_string(envelope).into_alien_error().context(
                crate::ErrorData::TransportDispatchRejected {
                    message: "Failed to serialize command envelope before Service Bus dispatch"
                        .to_string(),
                    transport_type: Some("servicebus".to_string()),
                    target: Some(envelope.command_id.clone()),
                },
            )?;

            let namespace_name = self.namespace_name.clone();
            let queue_name = self.queue_name.clone();

            // Create custom properties for command metadata
            let mut custom_properties = HashMap::new();
            custom_properties.insert("cmd-protocol".to_string(), envelope.protocol.clone());
            custom_properties.insert("cmd-command-id".to_string(), envelope.command_id.clone());
            custom_properties.insert("cmd-command".to_string(), envelope.command.clone());

            let message = SendMessageParameters {
                body: envelope_json,
                broker_properties: None,
                custom_properties,
            };

            // Service Bus does not retry SendMessage internally, so an
            // allowlisted 4xx is a reliable rejection. Network failures,
            // request timeouts, and 5xx responses are ambiguous. Scrub the
            // command body before wrapping either category.
            let send_result = redact_request_body(
                self.servicebus_client
                    .send_message(namespace_name.clone(), queue_name.clone(), message)
                    .await,
            );
            if let Err(error) = send_result {
                let context = if is_definite_service_bus_rejection(&error) {
                    crate::ErrorData::TransportDispatchRejected {
                        message: format!(
                            "Service Bus rejected message for queue {}/{}",
                            namespace_name, queue_name
                        ),
                        transport_type: Some("servicebus".to_string()),
                        target: Some(envelope.command_id.clone()),
                    }
                } else {
                    crate::ErrorData::TransportDispatchFailed {
                        message: format!(
                            "Service Bus acknowledgement failed for queue {}/{}",
                            namespace_name, queue_name
                        ),
                        transport_type: Some("servicebus".to_string()),
                        target: Some(envelope.command_id.clone()),
                    }
                };
                return Err(scrub_command_provider_error(error).context(context));
            }

            tracing::debug!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                namespace = %namespace_name,
                queue = %queue_name,
                "Successfully dispatched command envelope to Service Bus queue"
            );

            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

#[cfg(any(feature = "server", feature = "dispatchers"))]
pub use platform_dispatchers::*;

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use alien_core::{
        AwsClientConfig, AwsCredentials, AwsServiceOverrides, AzureClientConfig, AzureCredentials,
        AzureServiceOverrides, BodySpec, GcpClientConfig, GcpCredentials, GcpServiceOverrides,
    };
    use axum::{
        extract::Json,
        http::{header::AUTHORIZATION, HeaderMap, StatusCode},
        routing::post,
        Router,
    };
    use base64::prelude::*;

    use super::{
        CommandDispatcher, HttpCommandDispatcher, LambdaCommandDispatcher, PubSubCommandDispatcher,
        ServiceBusCommandDispatcher,
    };

    const COMMAND_BODY_SENTINEL: &str = "COMMAND_ERROR_BODY_MUST_BE_REDACTED";
    const SIGNED_URL_SENTINEL: &str =
        "https://storage.example.test/result?signature=MUST_NOT_ESCAPE";

    async fn spawn_static_response(
        status: StatusCode,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(move || async move { (status, body) }),
            )
            .await
            .unwrap();
        });
        (format!("http://{address}"), server)
    }

    fn sensitive_envelope(command_id: &str) -> crate::Envelope {
        let mut envelope = crate::test_utils::test_envelope(
            command_id,
            COMMAND_BODY_SENTINEL,
            BodySpec::inline(COMMAND_BODY_SENTINEL.as_bytes()),
        );
        envelope.response_handling.submit_response_url = SIGNED_URL_SENTINEL.to_string();
        envelope
    }

    fn aws_config(lambda_endpoint: String) -> AwsClientConfig {
        AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".to_string(),
                secret_access_key: "test-secret-key".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("lambda".to_string(), lambda_endpoint)]),
            }),
        }
    }

    fn azure_config(service_bus_endpoint: String) -> AzureClientConfig {
        AzureClientConfig {
            subscription_id: "test-subscription".to_string(),
            tenant_id: "test-tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(AzureServiceOverrides {
                endpoints: HashMap::from([("servicebus".to_string(), service_bus_endpoint)]),
            }),
        }
    }

    fn gcp_config(pubsub_endpoint: String) -> GcpClientConfig {
        GcpClientConfig {
            project_id: "test-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("pubsub".to_string(), pubsub_endpoint)]),
            }),
            project_number: Some("123456789012".to_string()),
        }
    }

    async fn accept_command(
        headers: HeaderMap,
        Json(envelope): Json<crate::Envelope>,
    ) -> StatusCode {
        if headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some("Bearer secret")
            && envelope.command_id == "cmd-http"
        {
            StatusCode::ACCEPTED
        } else {
            StatusCode::UNAUTHORIZED
        }
    }

    async fn legacy_success_route() -> StatusCode {
        StatusCode::OK
    }

    #[tokio::test]
    async fn http_dispatcher_posts_authenticated_envelope_and_checks_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(crate::WORKER_COMMAND_PUSH_PATH, post(accept_command)),
            )
            .await
            .unwrap();
        });
        let target_url = format!("http://{address}{}", crate::WORKER_COMMAND_PUSH_PATH);
        let envelope = crate::test_utils::test_simple_envelope("cmd-http", "sync");

        HttpCommandDispatcher::new(
            reqwest::Client::new(),
            target_url.clone(),
            "secret".to_string(),
        )
        .dispatch(&envelope)
        .await
        .unwrap();

        let error =
            HttpCommandDispatcher::new(reqwest::Client::new(), target_url, "wrong".to_string())
                .dispatch(&envelope)
                .await
                .unwrap_err();
        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");

        server.abort();
    }

    #[tokio::test]
    async fn http_dispatcher_rejects_legacy_200_route() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(crate::WORKER_COMMAND_PUSH_PATH, post(legacy_success_route)),
            )
            .await
            .unwrap();
        });
        let envelope = crate::test_utils::test_simple_envelope("cmd-http", "sync");
        let error = HttpCommandDispatcher::new(
            reqwest::Client::new(),
            format!("http://{address}{}", crate::WORKER_COMMAND_PUSH_PATH),
            "secret".to_string(),
        )
        .dispatch(&envelope)
        .await
        .expect_err("only the runtime's 202 acceptance is delivery");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");
        server.abort();
    }

    #[tokio::test]
    async fn http_dispatcher_classifies_connection_refusal_as_definite_rejection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let envelope = crate::test_utils::test_simple_envelope("cmd-http", "sync");

        let error = HttpCommandDispatcher::new(
            reqwest::Client::new(),
            format!("http://{address}{}", crate::WORKER_COMMAND_PUSH_PATH),
            "secret".to_string(),
        )
        .dispatch(&envelope)
        .await
        .expect_err("connection refusal happens before delivery");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");
    }

    #[tokio::test]
    async fn http_dispatcher_classifies_request_builder_failure_as_definite_rejection() {
        let envelope = crate::test_utils::test_simple_envelope("cmd-http", "sync");

        let error = HttpCommandDispatcher::new(
            reqwest::Client::new(),
            "http://[invalid-address".to_string(),
            "secret".to_string(),
        )
        .dispatch(&envelope)
        .await
        .expect_err("invalid URL fails before delivery");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");
    }

    #[tokio::test]
    async fn http_dispatcher_bounds_ambiguous_acknowledgement_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let envelope = crate::test_utils::test_simple_envelope("cmd-http", "sync");

        let error = HttpCommandDispatcher::new(
            reqwest::Client::new(),
            format!("http://{address}{}", crate::WORKER_COMMAND_PUSH_PATH),
            "secret".to_string(),
        )
        .with_request_timeout(Duration::from_millis(50))
        .dispatch(&envelope)
        .await
        .expect_err("blackholed acknowledgement must time out");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_FAILED");
        server.abort();
    }

    #[test]
    fn http_dispatcher_debug_redacts_token() {
        let debug = format!(
            "{:?}",
            HttpCommandDispatcher::new(
                reqwest::Client::new(),
                "http://worker/_alien/commands".to_string(),
                "super-secret".to_string(),
            )
        );
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret"));
    }

    #[tokio::test]
    async fn lambda_event_dispatch_accepts_exact_202() {
        let (endpoint, server) = spawn_static_response(StatusCode::ACCEPTED, "").await;
        let dispatcher = LambdaCommandDispatcher::new(
            reqwest::Client::new(),
            aws_config(endpoint),
            "test-function".to_string(),
        )
        .await
        .unwrap();

        dispatcher
            .dispatch(&sensitive_envelope("cmd-lambda-202"))
            .await
            .expect("Lambda Event invocation must accept exact HTTP 202");

        server.abort();
    }

    #[tokio::test]
    async fn lambda_event_dispatch_classifies_404_as_definite_rejection() {
        let (endpoint, server) = spawn_static_response(
            StatusCode::NOT_FOUND,
            r#"{"__type":"ResourceNotFoundException","message":"missing"}"#,
        )
        .await;
        let dispatcher = LambdaCommandDispatcher::new(
            reqwest::Client::new(),
            aws_config(endpoint),
            "missing-function".to_string(),
        )
        .await
        .unwrap();

        let error = dispatcher
            .dispatch(&sensitive_envelope("cmd-lambda-404"))
            .await
            .expect_err("an explicit Lambda 404 cannot have accepted the invocation");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");
        server.abort();
    }

    #[tokio::test]
    async fn lambda_event_dispatch_keeps_500_ambiguous() {
        let (endpoint, server) = spawn_static_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"__type":"ServiceException","message":"unknown outcome"}"#,
        )
        .await;
        let dispatcher = LambdaCommandDispatcher::new(
            reqwest::Client::new(),
            aws_config(endpoint),
            "test-function".to_string(),
        )
        .await
        .unwrap();

        let error = dispatcher
            .dispatch(&sensitive_envelope("cmd-lambda-500"))
            .await
            .expect_err("Lambda 5xx does not prove non-delivery");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_FAILED");
        server.abort();
    }

    #[tokio::test]
    async fn service_bus_dispatch_accepts_success_status() {
        let (endpoint, server) = spawn_static_response(StatusCode::CREATED, "").await;
        let dispatcher = ServiceBusCommandDispatcher::new(
            reqwest::Client::new(),
            azure_config(endpoint),
            "test-namespace".to_string(),
            "test-queue".to_string(),
        );

        dispatcher
            .dispatch(&sensitive_envelope("cmd-servicebus-201"))
            .await
            .expect("Service Bus success status must acknowledge message acceptance");

        server.abort();
    }

    #[tokio::test]
    async fn service_bus_dispatch_rejects_404_and_redacts_command_body() {
        let (endpoint, server) = spawn_static_response(
            StatusCode::NOT_FOUND,
            "COMMAND_ERROR_BODY_MUST_BE_REDACTED https://storage.example.test/result?signature=MUST_NOT_ESCAPE",
        )
        .await;
        let dispatcher = ServiceBusCommandDispatcher::new(
            reqwest::Client::new(),
            azure_config(endpoint),
            "test-namespace".to_string(),
            "missing-queue".to_string(),
        );

        let error = dispatcher
            .dispatch(&sensitive_envelope("cmd-servicebus-404"))
            .await
            .expect_err("an explicit Service Bus 404 cannot have accepted the message");
        let serialized = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, "TRANSPORT_DISPATCH_REJECTED");
        assert!(serialized.contains(r#""http_status":404"#));
        assert!(!serialized.contains(COMMAND_BODY_SENTINEL));
        assert!(!serialized.contains(SIGNED_URL_SENTINEL));
        assert!(!serialized.contains("http_request_text"));
        server.abort();
    }

    #[tokio::test]
    async fn service_bus_dispatch_keeps_500_ambiguous_and_redacts_command_body() {
        let (endpoint, server) = spawn_static_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "COMMAND_ERROR_BODY_MUST_BE_REDACTED https://storage.example.test/result?signature=MUST_NOT_ESCAPE",
        )
        .await;
        let dispatcher = ServiceBusCommandDispatcher::new(
            reqwest::Client::new(),
            azure_config(endpoint),
            "test-namespace".to_string(),
            "test-queue".to_string(),
        );

        let error = dispatcher
            .dispatch(&sensitive_envelope("cmd-servicebus-500"))
            .await
            .expect_err("Service Bus 5xx does not prove non-delivery");
        let serialized = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, "TRANSPORT_DISPATCH_FAILED");
        assert!(serialized.contains(r#""http_status":500"#));
        assert!(!serialized.contains(COMMAND_BODY_SENTINEL));
        assert!(!serialized.contains(SIGNED_URL_SENTINEL));
        assert!(!serialized.contains("http_request_text"));
        server.abort();
    }

    #[tokio::test]
    async fn service_bus_dispatch_keeps_request_timeout_ambiguous() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let dispatcher = ServiceBusCommandDispatcher::new(
            client,
            azure_config(format!("http://{address}")),
            "test-namespace".to_string(),
            "test-queue".to_string(),
        );

        let error = dispatcher
            .dispatch(&sensitive_envelope("cmd-servicebus-timeout"))
            .await
            .expect_err("lost Service Bus acknowledgement is ambiguous");

        assert_eq!(error.code, "TRANSPORT_DISPATCH_FAILED");
        server.abort();
    }

    #[tokio::test]
    async fn pubsub_403_remains_ambiguous_and_redacts_command_body() {
        let (endpoint, server) = spawn_static_response(
            StatusCode::FORBIDDEN,
            r#"{"error":{"code":403,"message":"COMMAND_ERROR_BODY_MUST_BE_REDACTED https://storage.example.test/result?signature=MUST_NOT_ESCAPE","status":"PERMISSION_DENIED"}}"#,
        )
        .await;
        let dispatcher = PubSubCommandDispatcher::new(
            reqwest::Client::new(),
            gcp_config(format!("{endpoint}/v1")),
            "test-topic".to_string(),
        );
        let envelope = sensitive_envelope("cmd-pubsub-403");
        let encoded_envelope = BASE64_STANDARD.encode(serde_json::to_vec(&envelope).unwrap());

        let error = dispatcher
            .dispatch(&envelope)
            .await
            .expect_err("Pub/Sub attempt history is insufficient for a definite rejection");
        let serialized = serde_json::to_string(&error).unwrap();

        assert_eq!(error.code, "TRANSPORT_DISPATCH_FAILED");
        assert!(serialized.contains(r#""http_status":403"#));
        assert!(!serialized.contains(COMMAND_BODY_SENTINEL));
        assert!(!serialized.contains(SIGNED_URL_SENTINEL));
        assert!(!serialized.contains(&encoded_envelope));
        assert!(!serialized.contains("http_request_text"));
        server.abort();
    }
}

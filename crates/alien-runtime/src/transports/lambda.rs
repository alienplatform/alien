//! Lambda Transport
//!
//! Receives work via Lambda Runtime API:
//! - API Gateway → HTTP request forwarded to app
//! - S3 event → StorageEvent via gRPC
//! - SQS message → QueueMessage via gRPC (or Command if command envelope)
//! - CloudWatch scheduled → CronEvent via gRPC
//! - InvokeFunction → Command via gRPC
//!
//! Supports both buffered and streaming response modes.

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use alien_bindings::grpc::control_service::{
    alien_bindings::control::{
        task::Payload, ArcCommand, CronEvent, QueueMessage, StorageEvent, Task,
    },
    ControlGrpcServer,
};
use alien_commands::{runtime::submit_response, types::CommandResponse};
use alien_error::AlienError;
use aws_lambda_events::{
    cloudwatch_events::CloudWatchEvent, event::s3::S3Event, event::sqs::SqsEvent,
};
use bytes::Bytes;
use futures_util::future::BoxFuture;
use http_body::{Body as HttpBody, SizeHint};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Frame;
use lambda_http::{
    aws_lambda_events::apigw::ApiGatewayV2httpResponse,
    http::{header::SET_COOKIE, Response, StatusCode},
    Body as LambdaBody, Request as LambdaRequest, RequestExt,
};
use lambda_runtime::{
    self as lambda, Diagnostic, Error as LambdaError, LambdaEvent, MetadataPrelude, Service,
    StreamResponse,
};
use pin_project_lite::pin_project;
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::Stream;
use tracing::{debug, error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::{
    config::LambdaMode,
    error::{ErrorData, Result},
};

/// Lambda extension for wait_until task draining.
/// This extension waits for the Lambda handler to complete before triggering drains.
/// Uses a channel to coordinate between the handler and extension.
pub mod wait_until_extension {
    use crate::error::{ErrorData, Result};
    use alien_error::AlienError;
    use lambda_extension::{service_fn, Extension, LambdaEvent, NextEvent};
    use tokio::sync::{mpsc::UnboundedReceiver, Mutex};
    use tracing::{error, info, warn};

    /// Internal extension that drains wait_until tasks after Lambda invocations.
    pub struct WaitUntilExtension {
        /// Receiver for signals that the handler has completed
        request_done_receiver: Mutex<UnboundedReceiver<()>>,
    }

    impl WaitUntilExtension {
        /// Creates a new WaitUntilExtension with a receiver for handler completion signals.
        pub fn new(request_done_receiver: UnboundedReceiver<()>) -> Self {
            Self {
                request_done_receiver: Mutex::new(request_done_receiver),
            }
        }

        /// Creates and registers the Lambda extension, returning the registered extension ready to run.
        pub async fn create_and_register(
            request_done_receiver: UnboundedReceiver<()>,
        ) -> Result<
            impl std::future::Future<Output = std::result::Result<(), lambda_extension::Error>>,
        > {
            let extension_instance = std::sync::Arc::new(Self::new(request_done_receiver));

            info!("Registering wait_until Lambda extension");
            let extension = Extension::new()
                // Internal extensions only support INVOKE events
                .with_events(&["INVOKE"])
                .with_events_processor(service_fn(move |event| {
                    let ext = extension_instance.clone();
                    async move { ext.invoke(event).await }
                }))
                .with_extension_name("internal-wait-until")
                .register()
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::Other {
                        message: format!("Failed to register wait_until extension: {}", e),
                    })
                })?;

            Ok(extension.run())
        }

        /// Invoked on every Lambda event.
        /// Waits for the handler to signal completion before triggering drain.
        async fn invoke(&self, event: LambdaEvent) -> Result<()> {
            match event.next {
                NextEvent::Shutdown(shutdown) => {
                    // Internal extensions should not receive SHUTDOWN events
                    return Err(AlienError::new(ErrorData::Other {
                        message: format!(
                            "Extension received unexpected SHUTDOWN event: {:?}",
                            shutdown
                        ),
                    }));
                }
                NextEvent::Invoke(invoke) => {
                    let request_id = &invoke.request_id;

                    info!(
                        request_id = %request_id,
                        "Extension waiting for handler to complete"
                    );

                    // Wait for the handler to signal completion
                    self.request_done_receiver
                        .lock()
                        .await
                        .recv()
                        .await
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::Other {
                                message: "Handler completion channel closed".to_string(),
                            })
                        })?;

                    info!(
                        request_id = %request_id,
                        "Handler completed, triggering wait_until drain"
                    );

                    // Now that the handler has completed, trigger the drain
                    if let Some(wait_until_server) = crate::runtime::get_wait_until_server() {
                        let timeout_secs = 10;

                        if let Err(e) = wait_until_server
                            .trigger_drain_all("lambda_invoke_end", timeout_secs)
                            .await
                        {
                            error!(
                                request_id = %request_id,
                                error = %e,
                                "Failed to trigger wait_until drain"
                            );
                        }
                    } else {
                        warn!(
                            request_id = %request_id,
                            "No wait_until server available, skipping drain"
                        );
                    }

                    Ok(())
                }
            }
        }
    }
}

/// Determines if we should register the wait_until extension.
/// Returns false when ALIEN_SKIP_WAIT_UNTIL_EXTENSION is set.
fn should_register_wait_until_extension() -> bool {
    use std::env;

    // Simple check: skip extension if environment variable is set
    if env::var("ALIEN_SKIP_WAIT_UNTIL_EXTENSION").is_ok() {
        return false;
    }

    true
}

/// A boxed body with a single non-empty frame for Lambda streaming.
/// Lambda streaming mode REQUIRES at least one NON-EMPTY data frame to be sent.
#[inline]
fn empty_box_body() -> BoxBody<Bytes, crate::error::Error> {
    // CRITICAL: Must send at least one byte for Lambda streaming to work
    Full::new(Bytes::from_static(b" "))
        .map_err(|e: std::convert::Infallible| match e {})
        .boxed()
}

/// Convenience for _streaming_ 500 responses.
#[inline]
fn internal_error_response() -> Response<AlienBodyAdapter> {
    warn!("Creating internal_error_response (500) for streaming Lambda");
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(AlienBodyAdapter {
            inner: empty_box_body(),
            request_id: "unknown".to_string(),
        })
        .expect("building 500 response never fails")
}

/// Creates a Lambda-streaming-compatible body from bytes.
///
/// Lambda streaming requires at least one non-empty data frame to be sent.
/// If bytes are empty, this function adds a single newline to prevent hangs.
pub fn lambda_streaming_body(bytes: Vec<u8>) -> BoxBody<Bytes, crate::error::Error> {
    let final_bytes = if bytes.is_empty() {
        Bytes::from_static(b"\n")
    } else {
        Bytes::from(bytes)
    };

    Full::new(final_bytes)
        .map_err(|e: std::convert::Infallible| match e {})
        .boxed()
}

// ============================================================================
// Body adapters for streaming
// ============================================================================

pin_project! {
    pub struct AlienBodyAdapter {
        #[pin]
        inner: BoxBody<Bytes, crate::error::Error>,
        request_id: String,
    }
}

impl HttpBody for AlienBodyAdapter {
    type Data = Bytes;
    type Error = crate::error::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        this.inner.poll_frame(cx)
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
}

pin_project! {
    pub struct BodyStream<B> {
        #[pin] body: B,
    }
}

impl<B> Stream for BodyStream<B>
where
    B: HttpBody + Unpin + Send + 'static,
    B::Data: Into<Bytes> + Send,
    B::Error: Into<LambdaError> + Send + std::fmt::Debug,
{
    type Item = std::result::Result<B::Data, B::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let frame_result = match futures_util::ready!(self.as_mut().project().body.poll_frame(cx)) {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                error!(error = ?e, "Error polling body frame");
                return Poll::Ready(Some(Err(e)));
            }
            None => {
                return Poll::Ready(None);
            }
        };

        // Try to extract data from the frame
        match frame_result.into_data() {
            Ok(data) => Poll::Ready(Some(Ok(data))),
            Err(_frame) => Poll::Ready(None),
        }
    }
}

// ============================================================================
// Lambda state and transport
// ============================================================================

/// Lambda transport state
struct LambdaState {
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
}

/// Lambda transport
pub struct LambdaTransport {
    mode: LambdaMode,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
}

impl LambdaTransport {
    pub fn new(mode: LambdaMode, control_server: Arc<ControlGrpcServer>) -> Self {
        Self {
            mode,
            control_server,
            app_http_port: None,
        }
    }

    pub fn with_app_port(mut self, port: u16) -> Self {
        self.app_http_port = Some(port);
        self
    }

    /// Run the Lambda transport
    pub async fn run(self) -> Result<()> {
        info!(mode = ?self.mode, "Starting Lambda transport");

        let state = Arc::new(LambdaState {
            control_server: self.control_server,
            app_http_port: self.app_http_port,
        });

        match self.mode {
            LambdaMode::Streaming => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = StreamingAdapter {
                    state,
                    request_done_sender,
                };
                run_streaming(adapter, request_done_receiver).await
            }
            LambdaMode::Buffered => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = BufferedAdapter {
                    state,
                    request_done_sender,
                };
                run_buffered(adapter, request_done_receiver).await
            }
        }
    }
}

// ============================================================================
// Service adapters
// ============================================================================

/// Convert LambdaEvent<Value> to LambdaRequest by parsing the event payload
fn event_to_request(event: LambdaEvent<Value>) -> LambdaRequest {
    use lambda_http::aws_lambda_events::apigw::ApiGatewayV2httpRequest;
    use lambda_http::request::LambdaRequest as LambdaRequestInternal;

    let payload = event.payload.clone();

    // Try to parse as API Gateway HTTP request
    if let Ok(http_req) = serde_json::from_value::<ApiGatewayV2httpRequest>(payload.clone()) {
        return LambdaRequest::from(LambdaRequestInternal::ApiGatewayV2(http_req))
            .with_lambda_context(event.context);
    }

    // For non-HTTP events (S3, SQS, CloudWatch, Commands), create a synthetic POST request
    // with the raw JSON payload as the body
    let body_json = serde_json::to_string(&payload).unwrap_or_default();
    let mut req = LambdaRequest::new(LambdaBody::Text(body_json));
    *req.method_mut() = lambda_http::http::Method::POST;
    *req.uri_mut() = "/__event".parse().unwrap();
    req.with_lambda_context(event.context)
}

#[derive(Clone)]
struct StreamingAdapter {
    state: Arc<LambdaState>,
    request_done_sender: UnboundedSender<()>,
}

impl Service<LambdaRequest> for StreamingAdapter {
    type Response = Response<AlienBodyAdapter>;
    type Error = LambdaError;
    type Future = BoxFuture<'static, std::result::Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: LambdaRequest) -> Self::Future {
        let state = self.state.clone();
        let request_done_sender = self.request_done_sender.clone();
        let request_id = req.lambda_context().request_id.clone();
        let span = info_span!("lambda_request", request_id = %request_id);

        Box::pin(
            async move {
                let result = handle_streaming_event(&state, &request_id, req).await;

                // Signal that the handler has completed
                if let Err(e) = request_done_sender.send(()) {
                    warn!(
                        request_id = %request_id,
                        error = ?e,
                        "Failed to signal request completion to extension"
                    );
                }

                result
            }
            .instrument(span),
        )
    }
}

#[derive(Clone)]
struct BufferedAdapter {
    state: Arc<LambdaState>,
    request_done_sender: UnboundedSender<()>,
}

impl Service<LambdaRequest> for BufferedAdapter {
    type Response = ApiGatewayV2httpResponse;
    type Error = LambdaError;
    type Future = BoxFuture<'static, std::result::Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: LambdaRequest) -> Self::Future {
        let state = self.state.clone();
        let request_done_sender = self.request_done_sender.clone();
        let request_id = req.lambda_context().request_id.clone();
        let span = info_span!("lambda_request", request_id = %request_id);

        Box::pin(
            async move {
                let result = handle_buffered_event(&state, &request_id, req).await;

                // Signal that the handler has completed
                if let Err(e) = request_done_sender.send(()) {
                    warn!(
                        request_id = %request_id,
                        error = ?e,
                        "Failed to signal request completion to extension"
                    );
                }

                result
            }
            .instrument(span),
        )
    }
}

// ============================================================================
// Runtime launchers
// ============================================================================

/// Launch Lambda **streaming** runtime.
async fn run_streaming(
    handler: StreamingAdapter,
    request_done_receiver: UnboundedReceiver<()>,
) -> Result<()> {
    info!("run_streaming: Setting up Lambda streaming runtime");

    use lambda_runtime::tower::ServiceExt;

    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(handler)
        .map_response(|res: Response<AlienBodyAdapter>| {
            let (parts, body) = res.into_parts();

            let cookies: Vec<_> = parts
                .headers
                .get_all(SET_COOKIE)
                .iter()
                .filter_map(|v| std::str::from_utf8(v.as_bytes()).ok().map(str::to_owned))
                .collect();

            let mut headers = parts.headers;
            headers.remove(SET_COOKIE);

            StreamResponse {
                metadata_prelude: MetadataPrelude {
                    headers,
                    status_code: parts.status,
                    cookies,
                },
                stream: BodyStream { body },
            }
        });

    // Register the wait_until extension only if not running in cargo lambda
    let wait_until_extension_future = if should_register_wait_until_extension() {
        Some(
            wait_until_extension::WaitUntilExtension::create_and_register(request_done_receiver)
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::TransportStartupFailed {
                        transport_name: "Lambda".to_string(),
                        message: format!("Failed to register wait_until extension: {}", e),
                        address: None,
                    })
                })?,
        )
    } else {
        info!("Skipping wait_until extension registration");
        None
    };

    // Run Lambda runtime, optionally with extension
    let lambda_runtime_future = lambda::run(svc);

    match wait_until_extension_future {
        Some(extension_future) => {
            tokio::try_join!(
                async move {
                    lambda_runtime_future.await.map_err(|e| {
                        AlienError::new(ErrorData::TransportStartupFailed {
                            transport_name: "Lambda".to_string(),
                            message: format!("Streaming runtime execution failed: {}", e),
                            address: None,
                        })
                    })
                },
                async move {
                    extension_future.await.map_err(|e| {
                        AlienError::new(ErrorData::TransportStartupFailed {
                            transport_name: "Lambda".to_string(),
                            message: format!("Wait until extension execution failed: {}", e),
                            address: None,
                        })
                    })
                }
            )?;
        }
        None => {
            lambda_runtime_future.await.map_err(|e| {
                AlienError::new(ErrorData::TransportStartupFailed {
                    transport_name: "Lambda".to_string(),
                    message: format!("Streaming runtime execution failed: {}", e),
                    address: None,
                })
            })?;
        }
    }

    Ok(())
}

/// Launch Lambda **buffered** runtime.
async fn run_buffered(
    adapter: BufferedAdapter,
    request_done_receiver: UnboundedReceiver<()>,
) -> Result<()> {
    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(adapter);

    // Register the wait_until extension only if not running in cargo lambda
    let wait_until_extension_future = if should_register_wait_until_extension() {
        Some(
            wait_until_extension::WaitUntilExtension::create_and_register(request_done_receiver)
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::TransportStartupFailed {
                        transport_name: "Lambda".to_string(),
                        message: format!("Failed to register wait_until extension: {}", e),
                        address: None,
                    })
                })?,
        )
    } else {
        info!("Skipping wait_until extension registration");
        None
    };

    // Run Lambda runtime, optionally with extension
    let lambda_runtime_future = lambda::run(svc);

    match wait_until_extension_future {
        Some(extension_future) => {
            tokio::try_join!(
                async move {
                    lambda_runtime_future.await.map_err(|e| {
                        AlienError::new(ErrorData::TransportStartupFailed {
                            transport_name: "Lambda".to_string(),
                            message: format!("Buffered runtime execution failed: {}", e),
                            address: None,
                        })
                    })
                },
                async move {
                    extension_future.await.map_err(|e| {
                        AlienError::new(ErrorData::TransportStartupFailed {
                            transport_name: "Lambda".to_string(),
                            message: format!("Wait until extension execution failed: {}", e),
                            address: None,
                        })
                    })
                }
            )?;
        }
        None => {
            lambda_runtime_future.await.map_err(|e| {
                AlienError::new(ErrorData::TransportStartupFailed {
                    transport_name: "Lambda".to_string(),
                    message: format!("Buffered runtime execution failed: {}", e),
                    address: None,
                })
            })?;
        }
    }

    Ok(())
}

// ============================================================================
// Event handlers - Streaming mode
// ============================================================================

/// Handle a Lambda event (streaming mode)
async fn handle_streaming_event(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
) -> std::result::Result<Response<AlienBodyAdapter>, LambdaError> {
    debug!(request_id = %request_id, "Handling Lambda event (streaming)");

    // Get body bytes
    let body_bytes = match event.body() {
        LambdaBody::Empty => vec![],
        LambdaBody::Text(s) => s.as_bytes().to_vec(),
        LambdaBody::Binary(b) => b.to_vec(),
    };

    // Try to parse as S3 event
    if let Ok(s3_event) = serde_json::from_slice::<S3Event>(&body_bytes) {
        if !s3_event.records.is_empty() {
            handle_s3_event_streaming(state, request_id, s3_event).await;
            return Ok(Response::builder()
                .status(200)
                .body(AlienBodyAdapter {
                    inner: lambda_streaming_body(vec![]),
                    request_id: request_id.to_string(),
                })
                .unwrap());
        }
    }

    // Try to parse as SQS event
    if let Ok(sqs_event) = serde_json::from_slice::<SqsEvent>(&body_bytes) {
        if !sqs_event.records.is_empty() {
            return handle_sqs_event_streaming(state, request_id, sqs_event).await;
        }
    }

    // Try to parse as CloudWatch scheduled event
    if let Ok(cw_event) = serde_json::from_slice::<CloudWatchEvent>(&body_bytes) {
        if cw_event.source.is_some() {
            handle_cloudwatch_event_streaming(state, request_id, cw_event).await;
            return Ok(Response::builder()
                .status(200)
                .body(AlienBodyAdapter {
                    inner: lambda_streaming_body(vec![]),
                    request_id: request_id.to_string(),
                })
                .unwrap());
        }
    }

    // Try to parse as command envelope
    if let Ok(envelope) = serde_json::from_slice::<alien_commands::types::Envelope>(&body_bytes) {
        return handle_command_streaming(state, request_id, envelope).await;
    }

    // Default: forward as HTTP request
    forward_http_request_streaming(state, request_id, event).await
}

async fn handle_s3_event_streaming(state: &LambdaState, request_id: &str, s3_event: S3Event) {
    for record in s3_event.records {
        let bucket = record.s3.bucket.name.unwrap_or_default();
        let key = record.s3.object.key.unwrap_or_default();
        let event_type = record.event_name.unwrap_or_default();
        let size = record.s3.object.size.unwrap_or(0) as u64;

        info!(bucket = %bucket, key = %key, event_type = %event_type, "S3 event");

        let task = Task {
            task_id: format!("{}-{}", request_id, key),
            payload: Some(Payload::StorageEvent(StorageEvent {
                bucket: bucket.clone(),
                key,
                event_type,
                size,
                content_type: String::new(),
                timestamp: None,
            })),
        };

        match state
            .control_server
            .send_task(task, std::time::Duration::from_secs(300))
            .await
        {
            Ok(result) => {
                if !result.success {
                    error!(
                        error_code = ?result.error_code,
                        error_message = ?result.error_message,
                        "Application failed to process storage event"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to send storage event");
            }
        }
    }
}

async fn handle_sqs_event_streaming(
    state: &LambdaState,
    request_id: &str,
    sqs_event: SqsEvent,
) -> std::result::Result<Response<AlienBodyAdapter>, LambdaError> {
    for record in sqs_event.records {
        let message_id = record.message_id.unwrap_or_default();
        let body = record.body.unwrap_or_default();
        let receipt_handle = record.receipt_handle.unwrap_or_default();
        let source = record.event_source_arn.unwrap_or_default();

        // Check if body is a command envelope
        if let Ok(envelope) = serde_json::from_str::<alien_commands::types::Envelope>(&body) {
            return handle_command_streaming(state, request_id, envelope).await;
        }

        info!(message_id = %message_id, source = %source, "SQS message");

        let task = Task {
            task_id: format!("{}-{}", request_id, message_id),
            payload: Some(Payload::QueueMessage(QueueMessage {
                id: message_id,
                payload: body.into_bytes(),
                receipt_handle,
                source,
                attempt_count: 1,
                timestamp: None,
            })),
        };

        match state
            .control_server
            .send_task(task, std::time::Duration::from_secs(300))
            .await
        {
            Ok(result) => {
                if !result.success {
                    error!(
                        error_code = ?result.error_code,
                        error_message = ?result.error_message,
                        "Application failed to process queue message"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to send queue message event");
            }
        }
    }

    Ok(Response::builder()
        .status(200)
        .body(AlienBodyAdapter {
            inner: lambda_streaming_body(vec![]),
            request_id: request_id.to_string(),
        })
        .unwrap())
}

async fn handle_cloudwatch_event_streaming(
    state: &LambdaState,
    request_id: &str,
    cw_event: CloudWatchEvent,
) {
    let schedule_name = cw_event.resources.first().cloned().unwrap_or_default();
    let scheduled_time = Some(prost_types::Timestamp {
        seconds: cw_event.time.timestamp(),
        nanos: cw_event.time.timestamp_subsec_nanos() as i32,
    });

    info!(schedule = %schedule_name, time = ?scheduled_time, "CloudWatch scheduled event");

    let task = Task {
        task_id: request_id.to_string(),
        payload: Some(Payload::CronEvent(CronEvent {
            schedule_name,
            scheduled_time,
        })),
    };

    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => {
            if !result.success {
                error!(
                    error_code = ?result.error_code,
                    error_message = ?result.error_message,
                    "Application failed to process cron event"
                );
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to send cron event");
        }
    }
}

async fn handle_command_streaming(
    state: &LambdaState,
    request_id: &str,
    envelope: alien_commands::types::Envelope,
) -> std::result::Result<Response<AlienBodyAdapter>, LambdaError> {
    let command_id = envelope.command_id.clone();
    let command_name = envelope.command.clone();

    info!(command_id = %command_id, command = %command_name, "Command received via Lambda");

    // Decode params
    let params = alien_commands::runtime::decode_params_bytes(&envelope)
        .await
        .unwrap_or_default();

    let task = Task {
        task_id: command_id.clone(),
        payload: Some(Payload::ArcCommand(ArcCommand {
            command_id: command_id.clone(),
            command_name,
            params,
            attempt: envelope.attempt,
            deadline: envelope.deadline.map(|d| prost_types::Timestamp {
                seconds: d.timestamp(),
                nanos: d.timestamp_subsec_nanos() as i32,
            }),
            response_url: envelope.response_handling.submit_response_url.clone(),
            storage_upload_url: envelope.response_handling.storage_upload_request.url(),
            max_inline_bytes: envelope.response_handling.max_inline_bytes,
        })),
    };

    // Send task and wait for result.
    // Use 120s timeout (well under Lambda's 180s function timeout) so that if the
    // app never responds, we still have time to submit an error response.
    debug!(command_id = %command_id, "Sending command task to application via gRPC");
    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(120))
        .await
    {
        Ok(result) => {
            debug!(
                command_id = %command_id,
                success = result.success,
                response_size = result.response_data.len(),
                "Received command result from application"
            );
            let command_response = if result.success {
                if result.response_data.is_empty() {
                    CommandResponse::success(b"{}")
                } else {
                    CommandResponse::success(&result.response_data)
                }
            } else {
                CommandResponse::error(
                    result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                    result
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string()),
                )
            };

            debug!(command_id = %command_id, "Submitting command response to manager");
            if let Err(e) = submit_response(&envelope, command_response).await {
                error!(command_id = %command_id, error = %e, "Failed to submit command response");
            } else {
                debug!(command_id = %command_id, "Command response submitted successfully");
            }
        }
        Err(e) => {
            error!(command_id = %command_id, error = %e, "Command task failed — send_task error");
            let command_response = CommandResponse::error("HANDLER_ERROR", &e);
            let _ = submit_response(&envelope, command_response).await;
        }
    }

    Ok(Response::builder()
        .status(200)
        .body(AlienBodyAdapter {
            inner: lambda_streaming_body(vec![]),
            request_id: request_id.to_string(),
        })
        .unwrap())
}

async fn forward_http_request_streaming(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
) -> std::result::Result<Response<AlienBodyAdapter>, LambdaError> {
    let Some(app_port) = state.app_http_port else {
        warn!("No app HTTP port registered, returning 503");
        return Ok(Response::builder()
            .status(503)
            .body(AlienBodyAdapter {
                inner: lambda_streaming_body(b"App HTTP server not registered".to_vec()),
                request_id: request_id.to_string(),
            })
            .unwrap());
    };

    let method = event.method().to_string();
    let uri = event.uri().to_string();
    debug!(request_id = %request_id, method = %method, uri = %uri, app_port = app_port, "Forwarding to app");

    let client = reqwest::Client::new();
    let path_and_query = event
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("http://127.0.0.1:{}{}", app_port, path_and_query);

    let mut req = client.request(
        reqwest::Method::from_bytes(event.method().as_str().as_bytes()).unwrap(),
        &url,
    );

    // Copy headers
    for (name, value) in event.headers() {
        if let Ok(v) = value.to_str() {
            req = req.header(name.as_str(), v);
        }
    }

    // Copy body
    let body_bytes = match event.body() {
        LambdaBody::Empty => vec![],
        LambdaBody::Text(s) => s.as_bytes().to_vec(),
        LambdaBody::Binary(b) => b.to_vec(),
    };
    req = req.body(body_bytes);

    // Send request and stream response
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let response_headers = resp.headers().clone();

            // Stream the response body instead of buffering it
            // This enables SSE and other streaming responses (within Lambda's timeout limits)
            use futures_util::TryStreamExt;
            let byte_stream = resp
                .bytes_stream()
                .map_ok(|bytes| hyper::body::Frame::data(bytes))
                .map_err(|e| {
                    crate::error::Error::new(crate::error::ErrorData::Other {
                        message: format!("Stream error: {}", e),
                    })
                });
            let stream_body = BoxBody::new(http_body_util::StreamBody::new(byte_stream));

            // Build response with headers
            let mut builder = Response::builder().status(status);

            // Copy response headers
            for (name, value) in response_headers.iter() {
                builder = builder.header(name.as_str(), value.as_bytes());
            }

            Ok(builder
                .body(AlienBodyAdapter {
                    inner: stream_body,
                    request_id: request_id.to_string(),
                })
                .unwrap())
        }
        Err(e) => {
            error!(error = %e, "Failed to forward request to app");
            Ok(Response::builder()
                .status(502)
                .body(AlienBodyAdapter {
                    inner: lambda_streaming_body(format!("Failed to forward: {}", e).into_bytes()),
                    request_id: request_id.to_string(),
                })
                .unwrap())
        }
    }
}

// ============================================================================
// Event handlers - Buffered mode
// ============================================================================

/// Handle a Lambda event (buffered mode)
async fn handle_buffered_event(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    debug!(request_id = %request_id, "Handling Lambda event (buffered)");

    // Get body bytes
    let body_bytes = match event.body() {
        LambdaBody::Empty => vec![],
        LambdaBody::Text(s) => s.as_bytes().to_vec(),
        LambdaBody::Binary(b) => b.to_vec(),
    };

    // Try to parse as S3 event
    if let Ok(s3_event) = serde_json::from_slice::<S3Event>(&body_bytes) {
        if !s3_event.records.is_empty() {
            return handle_s3_event_buffered(state, request_id, s3_event).await;
        }
    }

    // Try to parse as SQS event
    if let Ok(sqs_event) = serde_json::from_slice::<SqsEvent>(&body_bytes) {
        if !sqs_event.records.is_empty() {
            return handle_sqs_event_buffered(state, request_id, sqs_event).await;
        }
    }

    // Try to parse as CloudWatch scheduled event
    if let Ok(cw_event) = serde_json::from_slice::<CloudWatchEvent>(&body_bytes) {
        if cw_event.source.is_some() {
            return handle_cloudwatch_event_buffered(state, request_id, cw_event).await;
        }
    }

    // Try to parse as command envelope
    if let Ok(envelope) = serde_json::from_slice::<alien_commands::types::Envelope>(&body_bytes) {
        return handle_command_buffered(state, request_id, envelope).await;
    }

    // Default: forward as HTTP request
    forward_http_request_buffered(state, request_id, event).await
}

async fn handle_s3_event_buffered(
    state: &LambdaState,
    request_id: &str,
    s3_event: S3Event,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    for record in s3_event.records {
        let bucket = record.s3.bucket.name.unwrap_or_default();
        let key = record.s3.object.key.unwrap_or_default();
        let event_type = record.event_name.unwrap_or_default();
        let size = record.s3.object.size.unwrap_or(0) as u64;

        info!(bucket = %bucket, key = %key, event_type = %event_type, "S3 event");

        let task = Task {
            task_id: format!("{}-{}", request_id, key),
            payload: Some(Payload::StorageEvent(StorageEvent {
                bucket: bucket.clone(),
                key,
                event_type,
                size,
                content_type: String::new(),
                timestamp: None,
            })),
        };

        match state
            .control_server
            .send_task(task, std::time::Duration::from_secs(300))
            .await
        {
            Ok(result) => {
                if !result.success {
                    error!(
                        error_code = ?result.error_code,
                        error_message = ?result.error_message,
                        "Application failed to process storage event"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to send storage event");
            }
        }
    }

    Ok(ApiGatewayV2httpResponse {
        status_code: 200,
        ..Default::default()
    })
}

async fn handle_sqs_event_buffered(
    state: &LambdaState,
    request_id: &str,
    sqs_event: SqsEvent,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    for record in sqs_event.records {
        let message_id = record.message_id.unwrap_or_default();
        let body = record.body.unwrap_or_default();
        let receipt_handle = record.receipt_handle.unwrap_or_default();
        let source = record.event_source_arn.unwrap_or_default();

        // Check if body is a command envelope
        if let Ok(envelope) = serde_json::from_str::<alien_commands::types::Envelope>(&body) {
            return handle_command_buffered(state, request_id, envelope).await;
        }

        info!(message_id = %message_id, source = %source, "SQS message");

        let task = Task {
            task_id: format!("{}-{}", request_id, message_id),
            payload: Some(Payload::QueueMessage(QueueMessage {
                id: message_id,
                payload: body.into_bytes(),
                receipt_handle,
                source,
                attempt_count: 1,
                timestamp: None,
            })),
        };

        match state
            .control_server
            .send_task(task, std::time::Duration::from_secs(300))
            .await
        {
            Ok(result) => {
                if !result.success {
                    error!(
                        error_code = ?result.error_code,
                        error_message = ?result.error_message,
                        "Application failed to process queue message"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to send queue message event");
            }
        }
    }

    Ok(ApiGatewayV2httpResponse {
        status_code: 200,
        ..Default::default()
    })
}

async fn handle_cloudwatch_event_buffered(
    state: &LambdaState,
    request_id: &str,
    cw_event: CloudWatchEvent,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    let schedule_name = cw_event.resources.first().cloned().unwrap_or_default();
    let scheduled_time = Some(prost_types::Timestamp {
        seconds: cw_event.time.timestamp(),
        nanos: cw_event.time.timestamp_subsec_nanos() as i32,
    });

    info!(schedule = %schedule_name, time = ?scheduled_time, "CloudWatch scheduled event");

    let task = Task {
        task_id: request_id.to_string(),
        payload: Some(Payload::CronEvent(CronEvent {
            schedule_name,
            scheduled_time,
        })),
    };

    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => {
            if !result.success {
                error!(
                    error_code = ?result.error_code,
                    error_message = ?result.error_message,
                    "Application failed to process cron event"
                );
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to send cron event");
        }
    }

    Ok(ApiGatewayV2httpResponse {
        status_code: 200,
        ..Default::default()
    })
}

async fn handle_command_buffered(
    state: &LambdaState,
    _request_id: &str,
    envelope: alien_commands::types::Envelope,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    let command_id = envelope.command_id.clone();
    let command_name = envelope.command.clone();

    info!(command_id = %command_id, command = %command_name, "Command received");

    // Decode params
    let params = alien_commands::runtime::decode_params_bytes(&envelope)
        .await
        .unwrap_or_default();

    let task = Task {
        task_id: command_id.clone(),
        payload: Some(Payload::ArcCommand(ArcCommand {
            command_id: command_id.clone(),
            command_name,
            params,
            attempt: envelope.attempt,
            deadline: envelope.deadline.map(|d| prost_types::Timestamp {
                seconds: d.timestamp(),
                nanos: d.timestamp_subsec_nanos() as i32,
            }),
            response_url: envelope.response_handling.submit_response_url.clone(),
            storage_upload_url: envelope.response_handling.storage_upload_request.url(),
            max_inline_bytes: envelope.response_handling.max_inline_bytes,
        })),
    };

    // Send task and wait for result
    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => {
            let command_response = if result.success {
                if result.response_data.is_empty() {
                    CommandResponse::success(b"{}")
                } else {
                    CommandResponse::success(&result.response_data)
                }
            } else {
                CommandResponse::error(
                    result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                    result
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string()),
                )
            };

            if let Err(e) = submit_response(&envelope, command_response).await {
                error!(error = %e, "Failed to submit command response");
            }
        }
        Err(e) => {
            let command_response = CommandResponse::error("HANDLER_ERROR", &e);
            let _ = submit_response(&envelope, command_response).await;
            error!(error = %e, "Command handler error");
        }
    }

    Ok(ApiGatewayV2httpResponse {
        status_code: 200,
        ..Default::default()
    })
}

async fn forward_http_request_buffered(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    let Some(app_port) = state.app_http_port else {
        warn!("No app HTTP port registered, returning 503");
        return Ok(ApiGatewayV2httpResponse {
            status_code: 503,
            body: Some(LambdaBody::Text(
                "App HTTP server not registered".to_string(),
            )),
            ..Default::default()
        });
    };

    let method = event.method().to_string();
    let uri = event.uri().to_string();
    debug!(request_id = %request_id, method = %method, uri = %uri, app_port = app_port, "Forwarding to app");

    let client = reqwest::Client::new();
    let path_and_query = event
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("http://127.0.0.1:{}{}", app_port, path_and_query);

    let mut req = client.request(
        reqwest::Method::from_bytes(event.method().as_str().as_bytes()).unwrap(),
        &url,
    );

    // Copy headers
    for (name, value) in event.headers() {
        if let Ok(v) = value.to_str() {
            req = req.header(name.as_str(), v);
        }
    }

    // Copy body
    let body_bytes = match event.body() {
        LambdaBody::Empty => vec![],
        LambdaBody::Text(s) => s.as_bytes().to_vec(),
        LambdaBody::Binary(b) => b.to_vec(),
    };
    req = req.body(body_bytes);

    // Send request
    match req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            // Clone headers before consuming the body
            let headers = resp.headers().clone();
            let body_bytes = resp.bytes().await.unwrap_or_default().to_vec();

            let cookies: Vec<_> = headers
                .get_all(SET_COOKIE)
                .iter()
                .filter_map(|v| std::str::from_utf8(v.as_bytes()).ok().map(str::to_owned))
                .collect();

            let mut response_headers = headers;
            response_headers.remove(SET_COOKIE);

            Ok(ApiGatewayV2httpResponse {
                status_code: status,
                headers: response_headers,
                body: Some(LambdaBody::Binary(body_bytes)),
                is_base64_encoded: true,
                cookies,
                ..Default::default()
            })
        }
        Err(e) => {
            error!(error = %e, "Failed to forward request to app");
            Ok(ApiGatewayV2httpResponse {
                status_code: 502,
                body: Some(LambdaBody::Text(format!("Failed to forward: {}", e))),
                ..Default::default()
            })
        }
    }
}

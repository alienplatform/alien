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
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alien_commands::{runtime::submit_response, types::CommandResponse};
use alien_error::AlienError;
use alien_worker_protocol::{
    control::{task::Payload, ArcCommand, CronEvent, QueueMessage, StorageEvent, Task},
    ControlGrpcServer,
};
use aws_lambda_events::{
    cloudwatch_events::CloudWatchEvent, event::s3::S3Event, event::sqs::SqsEvent,
};
use bytes::Bytes;
use futures_util::future::BoxFuture;
use http_body::Body as HttpBody;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use lambda_http::{
    aws_lambda_events::apigw::ApiGatewayV2httpResponse,
    http::{
        header::{RETRY_AFTER, SET_COOKIE},
        Response,
    },
    Body as LambdaBody, Request as LambdaRequest, RequestExt,
};
use lambda_runtime::{
    self as lambda, Error as LambdaError, LambdaEvent, MetadataPrelude, Service, StreamResponse,
};
use pin_project_lite::pin_project;
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::Stream;
use tracing::{debug, error, info, info_span, warn, Instrument};

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

/// Response body type for the streaming mode.
type StreamingBody = BoxBody<Bytes, crate::error::Error>;

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
}

/// Lambda transport
pub struct LambdaTransport {
    mode: LambdaMode,
    control_server: Arc<ControlGrpcServer>,
}

impl LambdaTransport {
    pub fn new(mode: LambdaMode, control_server: Arc<ControlGrpcServer>) -> Self {
        Self {
            mode,
            control_server,
        }
    }

    /// Run the Lambda transport
    pub async fn run(self) -> Result<()> {
        info!(mode = ?self.mode, "Starting Lambda transport");

        let state = Arc::new(LambdaState {
            control_server: self.control_server,
        });

        match self.mode {
            LambdaMode::Streaming => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = StreamingAdapter {
                    state: state.clone(),
                    request_done_sender,
                };
                run_streaming(adapter, request_done_receiver, state.control_server.clone()).await
            }
            LambdaMode::Buffered => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = BufferedAdapter {
                    state: state.clone(),
                    request_done_sender,
                };
                run_buffered(adapter, request_done_receiver, state.control_server.clone()).await
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
    type Response = Response<StreamingBody>;
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

/// Drive a Lambda runtime future to completion, registering the wait_until
/// extension (unless disabled) and running both concurrently. Shared by the
/// streaming and buffered launchers, which differ only in how `svc` (and thus
/// `lambda_runtime_future`) is built and in the label used for error messages.
async fn drive_lambda_runtime<E: std::fmt::Display>(
    lambda_runtime_future: impl std::future::Future<Output = std::result::Result<(), E>>,
    request_done_receiver: UnboundedReceiver<()>,
    runtime_label: &str,
    control_server: Arc<ControlGrpcServer>,
) -> Result<()> {
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

    wait_for_initial_readiness(control_server).await;

    // Run Lambda runtime, optionally with extension
    match wait_until_extension_future {
        Some(extension_future) => {
            tokio::try_join!(
                async move {
                    lambda_runtime_future.await.map_err(|e| {
                        AlienError::new(ErrorData::TransportStartupFailed {
                            transport_name: "Lambda".to_string(),
                            message: format!("{} runtime execution failed: {}", runtime_label, e),
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
                    message: format!("{} runtime execution failed: {}", runtime_label, e),
                    address: None,
                })
            })?;
        }
    }

    Ok(())
}

/// Launch Lambda **streaming** runtime.
async fn run_streaming(
    handler: StreamingAdapter,
    request_done_receiver: UnboundedReceiver<()>,
    control_server: Arc<ControlGrpcServer>,
) -> Result<()> {
    info!("run_streaming: Setting up Lambda streaming runtime");

    use lambda_runtime::tower::ServiceExt;

    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(handler)
        .map_response(|res: Response<StreamingBody>| {
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

    drive_lambda_runtime(
        lambda::run(svc),
        request_done_receiver,
        "Streaming",
        control_server,
    )
    .await
}

/// Launch Lambda **buffered** runtime.
async fn run_buffered(
    adapter: BufferedAdapter,
    request_done_receiver: UnboundedReceiver<()>,
    control_server: Arc<ControlGrpcServer>,
) -> Result<()> {
    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(adapter);

    drive_lambda_runtime(
        lambda::run(svc),
        request_done_receiver,
        "Buffered",
        control_server,
    )
    .await
}

const INITIAL_READINESS_BUDGET: Duration = Duration::from_secs(8);
const INVOCATION_DEADLINE_MARGIN: Duration = Duration::from_secs(1);

async fn wait_for_initial_readiness(control_server: Arc<ControlGrpcServer>) {
    wait_for_initial_readiness_with_budget(control_server, INITIAL_READINESS_BUDGET).await;
}

async fn wait_for_initial_readiness_with_budget(
    control_server: Arc<ControlGrpcServer>,
    budget: Duration,
) {
    let ready = async {
        tokio::join!(
            control_server.wait_for_http_server(),
            control_server.wait_for_task_subscriber()
        );
    };

    if tokio::time::timeout(budget, ready).await.is_err() {
        warn!(
            budget_seconds = budget.as_secs(),
            "Application is not fully ready; starting Lambda Runtime API polling"
        );
    } else {
        info!("Application is ready; starting Lambda Runtime API polling");
    }
}

fn invocation_readiness_budget(event: &LambdaRequest) -> Duration {
    let deadline_ms = event.lambda_context().deadline;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Duration::from_millis(deadline_ms.saturating_sub(now_ms))
        .saturating_sub(INVOCATION_DEADLINE_MARGIN)
}

fn invocation_failure(message: impl Into<String>) -> LambdaError {
    std::io::Error::other(message.into()).into()
}

async fn wait_for_task_readiness(
    state: &LambdaState,
    budget: Duration,
) -> std::result::Result<(), LambdaError> {
    tokio::time::timeout(budget, state.control_server.wait_for_task_subscriber())
        .await
        .map_err(|_| invocation_failure("Application task handler was not ready before deadline"))
}

async fn wait_for_http_readiness(state: &LambdaState, budget: Duration) -> Option<u16> {
    tokio::time::timeout(budget, state.control_server.wait_for_http_server())
        .await
        .ok()
        .flatten()
}

// ============================================================================
// Event classification and shared handlers (both modes)
// ============================================================================

/// A non-HTTP Lambda event, classified once for both response modes.
/// Payloads are boxed to keep the enum small.
enum TaskEvent {
    S3(Box<S3Event>),
    Sqs(Box<SqsEvent>),
    CloudWatch(Box<CloudWatchEvent>),
    Command(Box<alien_commands::types::Envelope>),
}

/// The outcome of classifying an incoming Lambda event.
enum ClassifiedEvent {
    /// A recognized event payload, dispatched to the app as a Task.
    Task(TaskEvent),
    /// Not a recognized event payload: forward as an HTTP request to the app.
    Http(Box<LambdaRequest>),
}

/// Extract the raw body bytes from a Lambda request.
fn event_body_bytes(event: &LambdaRequest) -> Vec<u8> {
    match event.body() {
        LambdaBody::Empty => vec![],
        LambdaBody::Text(s) => s.as_bytes().to_vec(),
        LambdaBody::Binary(b) => b.to_vec(),
    }
}

/// Classify an incoming Lambda event by trying each known payload shape in
/// order (S3, SQS, CloudWatch schedule, command envelope), falling back to
/// HTTP forwarding. Classification is identical for buffered and streaming
/// modes; only response construction differs per mode.
fn classify_event(event: LambdaRequest) -> ClassifiedEvent {
    let body_bytes = event_body_bytes(&event);

    if let Ok(s3_event) = serde_json::from_slice::<S3Event>(&body_bytes) {
        if !s3_event.records.is_empty() {
            return ClassifiedEvent::Task(TaskEvent::S3(Box::new(s3_event)));
        }
    }

    if let Ok(sqs_event) = serde_json::from_slice::<SqsEvent>(&body_bytes) {
        if !sqs_event.records.is_empty() {
            return ClassifiedEvent::Task(TaskEvent::Sqs(Box::new(sqs_event)));
        }
    }

    if let Ok(cw_event) = serde_json::from_slice::<CloudWatchEvent>(&body_bytes) {
        if cw_event.source.is_some() {
            return ClassifiedEvent::Task(TaskEvent::CloudWatch(Box::new(cw_event)));
        }
    }

    if let Ok(envelope) = serde_json::from_slice::<alien_commands::types::Envelope>(&body_bytes) {
        // Gate on the protocol field like `shared::try_parse_envelope`: JSON
        // that merely matches Envelope's shape (or a future protocol
        // version) must not be executed under v1 semantics.
        if envelope.protocol == alien_commands::PROTOCOL_VERSION {
            return ClassifiedEvent::Task(TaskEvent::Command(Box::new(envelope)));
        }
    }

    ClassifiedEvent::Http(Box::new(event))
}

/// Dispatch a classified non-HTTP event to the app. Shared by both modes.
/// The caller returns an empty 200 only after every task succeeds; failures
/// become invocation errors so the AWS event source retries them.
async fn dispatch_task_event(
    state: &LambdaState,
    request_id: &str,
    event: TaskEvent,
) -> Result<()> {
    match event {
        TaskEvent::S3(s3_event) => handle_s3_event(state, request_id, *s3_event).await,
        TaskEvent::Sqs(sqs_event) => handle_sqs_event(state, request_id, *sqs_event).await,
        TaskEvent::CloudWatch(cw_event) => {
            handle_cloudwatch_event(state, request_id, *cw_event).await
        }
        TaskEvent::Command(envelope) => handle_command(state, &envelope).await,
    }
}

/// Build the StorageEvent task for one S3 record via the canonical
/// `events::s3_event_record_to_storage_event` conversion, which maps S3 event
/// names (ObjectCreated:Put → created) and extracts size, etag, region,
/// version id, and timestamp. Both buffered and streaming modes route through
/// this, so buffered tasks carry the full field set.
fn s3_record_to_task(
    request_id: &str,
    record: aws_lambda_events::s3::S3EventRecord,
) -> Result<Task> {
    let storage_event = crate::events::s3_event_record_to_storage_event(record)?;

    let event_type_str = serde_json::to_value(&storage_event.event_type)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Task {
        task_id: format!("{}-{}", request_id, storage_event.object_key),
        payload: Some(Payload::StorageEvent(StorageEvent {
            bucket: storage_event.bucket_name,
            key: storage_event.object_key,
            event_type: event_type_str,
            size: storage_event.size.unwrap_or(0),
            content_type: storage_event.content_type.unwrap_or_default(),
            timestamp: Some(prost_types::Timestamp {
                seconds: storage_event.timestamp.timestamp(),
                nanos: storage_event.timestamp.timestamp_subsec_nanos() as i32,
            }),
            etag: storage_event.etag.unwrap_or_default(),
            region: storage_event.region.unwrap_or_default(),
            version_id: storage_event.version_id.unwrap_or_default(),
            current_tier: storage_event.current_tier.unwrap_or_default(),
            metadata: storage_event.metadata,
        })),
    })
}

/// Build the QueueMessage task for one SQS record via the canonical
/// `events::sqs_message_to_queue_message` conversion, which extracts
/// timestamp, attempt_count, source, and attributes. Both buffered and
/// streaming modes route through this, so buffered tasks carry the full
/// field set.
fn sqs_record_to_task(
    request_id: &str,
    record: aws_lambda_events::sqs::SqsMessage,
) -> Result<Task> {
    let queue_msg = crate::events::sqs_message_to_queue_message(record)?;

    let timestamp = Some(prost_types::Timestamp {
        seconds: queue_msg.timestamp.timestamp(),
        nanos: queue_msg.timestamp.timestamp_subsec_nanos() as i32,
    });

    let payload_bytes = match &queue_msg.payload {
        alien_core::MessagePayload::Json(v) => serde_json::to_vec(v).unwrap_or_default(),
        alien_core::MessagePayload::Text(s) => s.as_bytes().to_vec(),
    };

    Ok(Task {
        task_id: format!("{}-{}", request_id, queue_msg.id),
        payload: Some(Payload::QueueMessage(QueueMessage {
            id: queue_msg.id,
            payload: payload_bytes,
            receipt_handle: queue_msg.receipt_handle,
            source: queue_msg.source,
            attempt_count: queue_msg.attempt_count.unwrap_or(1),
            timestamp,
            attributes: queue_msg.attributes,
        })),
    })
}

/// Send an event task with the standard event timeout, logging an error on
/// either a non-success result or a transport failure. Shared by the S3,
/// SQS, and CloudWatch handlers, which differ only in the task built and the
/// `desc` used in log messages.
async fn send_event_task(state: &LambdaState, task: Task, desc: &str) -> Result<()> {
    let task_id = task.task_id.clone();
    let result = state
        .control_server
        .send_task(task, super::shared::EVENT_TASK_TIMEOUT)
        .await
        .map_err(|message| {
            AlienError::new(ErrorData::TaskDeliveryFailed {
                task_id: task_id.clone(),
                task_type: desc.to_string(),
                message,
            })
        })?;

    if !result.success {
        return Err(AlienError::new(ErrorData::TaskDeliveryFailed {
            task_id,
            task_type: desc.to_string(),
            message: result
                .error_message
                .unwrap_or_else(|| "unknown application error".to_string()),
        }));
    }

    Ok(())
}

async fn handle_s3_event(state: &LambdaState, request_id: &str, s3_event: S3Event) -> Result<()> {
    for record in s3_event.records {
        let task = s3_record_to_task(request_id, record)?;

        send_event_task(state, task, "storage event").await?;
    }
    Ok(())
}

async fn handle_sqs_event(
    state: &LambdaState,
    request_id: &str,
    sqs_event: SqsEvent,
) -> Result<()> {
    for record in sqs_event.records {
        // Check if body is a command envelope before converting. Gate on the
        // protocol field like `shared::try_parse_envelope`: any JSON that
        // merely satisfies Envelope's shape must not be swallowed as a
        // command. `continue`, never `return`, so every record in the event is
        // processed before the invocation is acknowledged.
        if let Some(ref body) = record.body {
            if let Ok(envelope) = serde_json::from_str::<alien_commands::types::Envelope>(body) {
                if envelope.protocol == alien_commands::PROTOCOL_VERSION {
                    handle_command(state, &envelope).await?;
                    continue;
                }
            }
        }

        let task = sqs_record_to_task(request_id, record)?;

        send_event_task(state, task, "queue message").await?;
    }
    Ok(())
}

async fn handle_cloudwatch_event(
    state: &LambdaState,
    request_id: &str,
    cw_event: CloudWatchEvent,
) -> Result<()> {
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

    send_event_task(state, task, "cron event").await
}

async fn handle_command(
    state: &LambdaState,
    envelope: &alien_commands::types::Envelope,
) -> Result<()> {
    let command_id = envelope.command_id.clone();
    let command_name = envelope.command.clone();

    info!(command_id = %command_id, command = %command_name, "Command received via Lambda");

    // Decode params. On failure, submit a typed error response under the decode
    // error's own code (matching the pull receiver's semantics) and return —
    // never run the handler on empty/garbage params.
    let params = match alien_commands::runtime::decode_params_bytes(envelope).await {
        Ok(params) => params,
        Err(e) => {
            error!(command_id = %command_id, error = %e, "Failed to decode command params");
            let command_response = CommandResponse::error(&e.code, e.to_string());
            if let Err(submit_err) = submit_response(envelope, command_response).await {
                return Err(AlienError::new(ErrorData::ResponseDeliveryFailed {
                    request_id: command_id,
                    message: submit_err.to_string(),
                    destination: Some("command response endpoint".to_string()),
                }));
            }
            return Ok(());
        }
    };

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
            if let Err(e) = submit_response(envelope, command_response).await {
                return Err(AlienError::new(ErrorData::ResponseDeliveryFailed {
                    request_id: command_id,
                    message: e.to_string(),
                    destination: Some("command response endpoint".to_string()),
                }));
            } else {
                debug!(command_id = %command_id, "Command response submitted successfully");
            }
        }
        Err(e) => {
            error!(command_id = %command_id, error = %e, "Command task failed — send_task error");
            let command_response = CommandResponse::error("HANDLER_ERROR", &e);
            submit_response(envelope, command_response)
                .await
                .map_err(|submit_error| {
                    AlienError::new(ErrorData::ResponseDeliveryFailed {
                        request_id: command_id,
                        message: format!(
                            "Command task failed ({e}); error response submission failed: {submit_error}"
                        ),
                        destination: Some("command response endpoint".to_string()),
                    })
                })?;
        }
    }
    Ok(())
}

/// Forward a Lambda request to the app's local HTTP server. Shared by both
/// modes; only the response handling (streamed vs buffered) is mode-specific.
async fn forward_to_app(
    app_port: u16,
    event: LambdaRequest,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
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
    req = req.body(event_body_bytes(&event));

    req.send().await
}

// ============================================================================
// Event handlers - Streaming mode
// ============================================================================

/// Empty 200 response for the streaming mode.
fn empty_streaming_response() -> Response<StreamingBody> {
    Response::builder()
        .status(200)
        .body(lambda_streaming_body(vec![]))
        .unwrap()
}

/// Handle a Lambda event (streaming mode)
async fn handle_streaming_event(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
) -> std::result::Result<Response<StreamingBody>, LambdaError> {
    debug!(request_id = %request_id, "Handling Lambda event (streaming)");
    let readiness_budget = invocation_readiness_budget(&event);

    match classify_event(event) {
        ClassifiedEvent::Task(task_event) => {
            wait_for_task_readiness(state, readiness_budget).await?;
            dispatch_task_event(state, request_id, task_event)
                .await
                .map_err(|error| invocation_failure(error.to_string()))?;
            Ok(empty_streaming_response())
        }
        ClassifiedEvent::Http(event) => {
            forward_http_request_streaming(state, request_id, *event, readiness_budget).await
        }
    }
}

async fn forward_http_request_streaming(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
    readiness_budget: Duration,
) -> std::result::Result<Response<StreamingBody>, LambdaError> {
    let Some(app_port) = wait_for_http_readiness(state, readiness_budget).await else {
        warn!("App HTTP server was not ready before the invocation deadline");
        return Ok(Response::builder()
            .status(503)
            .header(RETRY_AFTER, "1")
            .body(lambda_streaming_body(
                b"Service temporarily unavailable".to_vec(),
            ))
            .unwrap());
    };

    let method = event.method().to_string();
    let uri = event.uri().to_string();
    debug!(request_id = %request_id, method = %method, uri = %uri, app_port = app_port, "Forwarding to app");

    // Send request and stream response
    match forward_to_app(app_port, event).await {
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

            Ok(builder.body(stream_body).unwrap())
        }
        Err(e) => {
            error!(error = %e, "Failed to forward request to app");
            Ok(Response::builder()
                .status(502)
                .body(lambda_streaming_body(
                    format!("Failed to forward: {}", e).into_bytes(),
                ))
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
    let readiness_budget = invocation_readiness_budget(&event);

    match classify_event(event) {
        ClassifiedEvent::Task(task_event) => {
            wait_for_task_readiness(state, readiness_budget).await?;
            dispatch_task_event(state, request_id, task_event)
                .await
                .map_err(|error| invocation_failure(error.to_string()))?;
            Ok(ApiGatewayV2httpResponse {
                status_code: 200,
                ..Default::default()
            })
        }
        ClassifiedEvent::Http(event) => {
            forward_http_request_buffered(state, request_id, *event, readiness_budget).await
        }
    }
}

async fn forward_http_request_buffered(
    state: &LambdaState,
    request_id: &str,
    event: LambdaRequest,
    readiness_budget: Duration,
) -> std::result::Result<ApiGatewayV2httpResponse, LambdaError> {
    let Some(app_port) = wait_for_http_readiness(state, readiness_budget).await else {
        warn!("App HTTP server was not ready before the invocation deadline");
        let mut headers = lambda_http::http::HeaderMap::new();
        headers.insert(
            RETRY_AFTER,
            lambda_http::http::HeaderValue::from_static("1"),
        );
        return Ok(ApiGatewayV2httpResponse {
            status_code: 503,
            headers,
            body: Some(LambdaBody::Text(
                "Service temporarily unavailable".to_string(),
            )),
            ..Default::default()
        });
    };

    let method = event.method().to_string();
    let uri = event.uri().to_string();
    debug!(request_id = %request_id, method = %method, uri = %uri, app_port = app_port, "Forwarding to app");

    // Send request
    match forward_to_app(app_port, event).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn s3_event_json() -> serde_json::Value {
        serde_json::json!({
            "Records": [{
                "eventVersion": "2.1",
                "eventSource": "aws:s3",
                "awsRegion": "us-east-1",
                "eventTime": "2024-01-01T12:00:00.000Z",
                "eventName": "ObjectCreated:Put",
                "userIdentity": { "principalId": "AWS:X" },
                "requestParameters": { "sourceIPAddress": "127.0.0.1" },
                "responseElements": {
                    "x-amz-request-id": "req",
                    "x-amz-id-2": "host"
                },
                "s3": {
                    "s3SchemaVersion": "1.0",
                    "configurationId": "cfg",
                    "bucket": {
                        "name": "my-bucket",
                        "ownerIdentity": { "principalId": "X" },
                        "arn": "arn:aws:s3:::my-bucket"
                    },
                    "object": {
                        "key": "path/to/file.txt",
                        "size": 1024,
                        "eTag": "abc123etag",
                        "versionId": "v1",
                        "sequencer": "0055AED6DCD90281E5"
                    }
                }
            }]
        })
    }

    fn sqs_event_json() -> serde_json::Value {
        serde_json::json!({
            "Records": [{
                "messageId": "msg-1",
                "receiptHandle": "rh-1",
                "body": "{\"hello\":\"world\"}",
                "attributes": {
                    "ApproximateReceiveCount": "3",
                    "SentTimestamp": "1704110400000"
                },
                "messageAttributes": {
                    "trace": { "stringValue": "abc", "dataType": "String" }
                },
                "eventSourceARN": "arn:aws:sqs:us-east-1:123456789012:my-queue",
                "eventSource": "aws:sqs",
                "awsRegion": "us-east-1"
            }]
        })
    }

    /// Buffered and streaming modes share `s3_record_to_task`, so buffered S3
    /// tasks must carry the full canonical field set (mapped event type,
    /// etag, region, version id, timestamp) — previously the buffered handler
    /// hand-extracted only bucket/key/size and passed the raw event name.
    #[test]
    fn s3_tasks_use_canonical_conversion_in_both_modes() {
        let s3_event: S3Event = serde_json::from_value(s3_event_json()).expect("parse S3 event");
        let record = s3_event.records.into_iter().next().expect("one record");

        let task = s3_record_to_task("req-1", record).expect("task conversion should succeed");
        assert_eq!(task.task_id, "req-1-path/to/file.txt");

        let Some(Payload::StorageEvent(event)) = task.payload else {
            panic!("expected StorageEvent payload");
        };
        assert_eq!(event.bucket, "my-bucket");
        assert_eq!(event.key, "path/to/file.txt");
        // Mapped through StorageEventType, not the raw "ObjectCreated:Put".
        assert_eq!(event.event_type, "created");
        assert_eq!(event.size, 1024);
        assert_eq!(event.etag, "abc123etag");
        assert_eq!(event.region, "us-east-1");
        assert_eq!(event.version_id, "v1");
        let timestamp = event.timestamp.expect("timestamp must be set");
        assert_eq!(timestamp.seconds, 1704110400);
    }

    /// Buffered and streaming modes share `sqs_record_to_task`, so buffered
    /// SQS tasks must carry attempt_count, attributes, source (queue name),
    /// and timestamp — previously the buffered handler hardcoded
    /// attempt_count=1 with empty attributes and no timestamp.
    #[test]
    fn sqs_tasks_use_canonical_conversion_in_both_modes() {
        let sqs_event: SqsEvent =
            serde_json::from_value(sqs_event_json()).expect("parse SQS event");
        let record = sqs_event.records.into_iter().next().expect("one record");

        let task = sqs_record_to_task("req-1", record).expect("task conversion should succeed");
        assert_eq!(task.task_id, "req-1-msg-1");

        let Some(Payload::QueueMessage(message)) = task.payload else {
            panic!("expected QueueMessage payload");
        };
        assert_eq!(message.id, "msg-1");
        assert_eq!(message.receipt_handle, "rh-1");
        assert_eq!(message.source, "my-queue");
        assert_eq!(message.attempt_count, 3);
        assert_eq!(message.attributes.get("trace"), Some(&"abc".to_string()));
        let timestamp = message.timestamp.expect("timestamp must be set");
        assert_eq!(timestamp.seconds, 1704110400);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&message.payload).expect("json payload"),
            serde_json::json!({"hello": "world"})
        );
    }

    /// Classification is shared: an S3 payload classifies as a Task in both
    /// modes; an arbitrary request falls through to HTTP forwarding.
    #[test]
    fn classification_is_shared_between_modes() {
        let s3_request = LambdaRequest::new(LambdaBody::Text(s3_event_json().to_string()));
        match classify_event(s3_request) {
            ClassifiedEvent::Task(TaskEvent::S3(event)) => assert_eq!(event.records.len(), 1),
            _ => panic!("S3 payload must classify as an S3 task event"),
        }

        let sqs_request = LambdaRequest::new(LambdaBody::Text(sqs_event_json().to_string()));
        match classify_event(sqs_request) {
            ClassifiedEvent::Task(TaskEvent::Sqs(event)) => assert_eq!(event.records.len(), 1),
            _ => panic!("SQS payload must classify as an SQS task event"),
        }

        let http_request = LambdaRequest::new(LambdaBody::Text("plain body".to_string()));
        match classify_event(http_request) {
            ClassifiedEvent::Http(_) => {}
            _ => panic!("unrecognized payload must fall through to HTTP forwarding"),
        }
    }

    #[tokio::test]
    async fn initial_readiness_budget_expires_without_blocking_runtime_polling() {
        let control_server = Arc::new(ControlGrpcServer::new());

        tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_initial_readiness_with_budget(control_server, Duration::from_millis(5)),
        )
        .await
        .expect("bounded readiness must return");
    }

    #[tokio::test]
    async fn unready_http_returns_retryable_sanitized_503_in_both_modes() {
        let state = LambdaState {
            control_server: Arc::new(ControlGrpcServer::new()),
        };

        let streaming = forward_http_request_streaming(
            &state,
            "request",
            LambdaRequest::new(LambdaBody::Empty),
            Duration::ZERO,
        )
        .await
        .expect("streaming response");
        assert_eq!(streaming.status(), 503);
        assert_eq!(streaming.headers().get(RETRY_AFTER).unwrap(), "1");

        let buffered = forward_http_request_buffered(
            &state,
            "request",
            LambdaRequest::new(LambdaBody::Empty),
            Duration::ZERO,
        )
        .await
        .expect("buffered response");
        assert_eq!(buffered.status_code, 503);
        assert_eq!(buffered.headers.get(RETRY_AFTER).unwrap(), "1");
        assert_eq!(
            buffered.body,
            Some(LambdaBody::Text(
                "Service temporarily unavailable".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn event_delivery_failure_is_returned_to_lambda() {
        let state = LambdaState {
            control_server: Arc::new(ControlGrpcServer::new()),
        };

        let error = send_event_task(&state, Task::default(), "queue message")
            .await
            .expect_err("missing task subscriber must fail the invocation");
        assert!(error.to_string().contains("(queue message) failed"));
    }
}

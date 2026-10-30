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
use alien_error::{AlienError, Context as _};
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
use prost::Message as _;
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::Stream;
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::{
    config::LambdaMode,
    error::{ErrorData, Result},
    transports::shared::forward_client_builder,
};

/// Lambda extension for wait_until task draining.
/// This extension waits for the Lambda handler to complete before triggering drains.
/// Uses a channel to coordinate between the handler and extension.
pub mod wait_until_extension {
    use crate::error::{ErrorData, Result};
    use alien_error::AlienError;
    use lambda_extension::{service_fn, Extension, LambdaEvent, NextEvent};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::{
        sync::{mpsc::UnboundedReceiver, Mutex},
        task::JoinHandle,
    };
    use tracing::{debug, error, info, warn};

    /// Longest an invocation waits for its logs to export. Lambda can't hand the environment its
    /// next invocation until the extension returns, so a slow or unreachable log endpoint must
    /// not hold every invocation for the whole export.
    const MAX_LOG_EXPORT_WAIT: Duration = Duration::from_millis(500);

    /// Left for the extension to call `/next` after the export wait.
    const DEADLINE_MARGIN: Duration = Duration::from_millis(250);

    /// Internal extension that drains wait_until tasks after Lambda invocations.
    pub struct WaitUntilExtension {
        /// Receiver for signals that the handler has completed
        request_done_receiver: Mutex<UnboundedReceiver<()>>,
        /// An export that outlived its invocation's wait and may still be running.
        unfinished_log_export: Mutex<Option<JoinHandle<Result<()>>>>,
    }

    impl WaitUntilExtension {
        /// Creates a new WaitUntilExtension with a receiver for handler completion signals.
        pub fn new(request_done_receiver: UnboundedReceiver<()>) -> Self {
            Self {
                request_done_receiver: Mutex::new(request_done_receiver),
                unfinished_log_export: Mutex::new(None),
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

                    self.export_logs(request_id, invoke.deadline_ms).await;

                    Ok(())
                }
            }
        }

        /// Exports the logs this invocation produced before Lambda freezes the environment.
        ///
        /// Returning lets Lambda freeze the environment, and a batch still pending then waits for
        /// the next invocation, or is lost if none comes. This runs after the wait_until drain so
        /// its output is included. A failed or slow export is logged, never surfaced to the caller.
        async fn export_logs(&self, request_id: &str, deadline_ms: u64) {
            let mut unfinished = self.unfinished_log_export.lock().await;
            if unfinished
                .as_ref()
                .is_some_and(|export| !export.is_finished())
            {
                // The endpoint hasn't answered an earlier export. This invocation's records stay
                // in the batch and go out with the next export instead of holding it again.
                debug!(
                    request_id = %request_id,
                    "Earlier application log export still running; not waiting for another"
                );
                return;
            }

            let mut export = tokio::spawn(crate::otlp::flush_otlp_logs());
            let wait = log_export_wait(deadline_ms, SystemTime::now());
            *unfinished = match tokio::time::timeout(wait, &mut export).await {
                Ok(Ok(Ok(()))) => None,
                Ok(Ok(Err(e))) => {
                    error!(
                        request_id = %request_id,
                        error = %e,
                        "Failed to export application logs after the invocation"
                    );
                    None
                }
                Ok(Err(e)) => {
                    error!(
                        request_id = %request_id,
                        error = %e,
                        "Application log export task failed"
                    );
                    None
                }
                Err(_) => {
                    warn!(
                        request_id = %request_id,
                        wait_ms = wait.as_millis() as u64,
                        "Application log export did not finish in time; it continues in the background"
                    );
                    Some(export)
                }
            };
        }
    }

    /// How long an invocation waits for its logs to export: at most [`MAX_LOG_EXPORT_WAIT`], and
    /// never past `deadline_ms` (epoch milliseconds) less [`DEADLINE_MARGIN`], because the
    /// extension's time counts toward the function timeout.
    fn log_export_wait(deadline_ms: u64, now: SystemTime) -> Duration {
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        Duration::from_millis(deadline_ms)
            .saturating_sub(now)
            .saturating_sub(DEADLINE_MARGIN)
            .min(MAX_LOG_EXPORT_WAIT)
    }

    #[cfg(all(test, feature = "otlp"))]
    mod tests {
        use super::*;
        use lambda_extension::{InvokeEvent, Tracing};
        use opentelemetry::logs::AnyValue;
        use opentelemetry_sdk::{
            error::OTelSdkResult,
            logs::{
                BatchConfigBuilder, BatchLogProcessor, InMemoryLogExporter, LogBatch, LogExporter,
                SdkLoggerProvider,
            },
        };
        use std::{
            future::Future,
            sync::{Arc, Condvar, Mutex as StdMutex},
            time::Instant,
        };
        use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

        /// The app-log provider is process-global, so tests that install one run one at a time.
        static APP_LOG_PROVIDER: Mutex<()> = Mutex::const_new(());

        /// Installs `exporter` behind a batch that would wait an hour, so only an explicit flush
        /// exports it.
        fn install_app_log_exporter<E: LogExporter + 'static>(exporter: E) {
            let processor = BatchLogProcessor::builder(exporter)
                .with_batch_config(
                    BatchConfigBuilder::default()
                        .with_scheduled_delay(Duration::from_secs(3600))
                        .build(),
                )
                .build();
            crate::otlp::store_otlp_provider(
                SdkLoggerProvider::builder()
                    .with_log_processor(processor)
                    .build(),
            );
        }

        /// Runs one INVOKE through the extension whose handler has already finished, with the
        /// function timeout a minute away, and returns how long the extension held it.
        async fn run_invocation(
            extension: &WaitUntilExtension,
            request_done: &UnboundedSender<()>,
            request_id: &str,
        ) -> Duration {
            request_done.send(()).expect("the extension is listening");
            let deadline =
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(60);
            let started = Instant::now();
            extension
                .invoke(LambdaEvent {
                    next: NextEvent::Invoke(InvokeEvent {
                        deadline_ms: deadline.as_millis() as u64,
                        request_id: request_id.to_string(),
                        invoked_function_arn: "arn:aws:lambda:us-east-1:123456789012:function:f"
                            .to_string(),
                        tracing: Tracing::default(),
                    }),
                })
                .await
                .expect("the invocation completes");
            started.elapsed()
        }

        fn exported_bodies(exporter: &InMemoryLogExporter) -> Vec<Option<AnyValue>> {
            exporter
                .get_emitted_logs()
                .expect("exporter is readable")
                .iter()
                .map(|log| log.record.body().cloned())
                .collect()
        }

        fn body(text: &str) -> Option<AnyValue> {
            Some(AnyValue::String(text.to_string().into()))
        }

        /// An endpoint that doesn't answer until released: the export call blocks the batch
        /// processor's thread, as the blocking HTTP client does on an unreachable endpoint.
        #[derive(Debug, Clone)]
        struct UnansweredEndpoint {
            delivered: InMemoryLogExporter,
            released: Arc<(StdMutex<bool>, Condvar)>,
        }

        impl UnansweredEndpoint {
            fn release(&self) {
                let (released, answered) = &*self.released;
                *released.lock().unwrap() = true;
                answered.notify_all();
            }
        }

        /// Releases the endpoint when the test ends, even when an assertion fails first, so the
        /// batch processor's thread isn't left blocked for the rest of the test process.
        struct ReleaseOnDrop(UnansweredEndpoint);

        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                self.0.release();
            }
        }

        impl LogExporter for UnansweredEndpoint {
            fn export(&self, batch: LogBatch<'_>) -> impl Future<Output = OTelSdkResult> + Send {
                let (released, answered) = &*self.released;
                let _released = answered
                    .wait_while(released.lock().unwrap(), |released| !*released)
                    .unwrap();
                self.delivered.export(batch)
            }
        }

        #[tokio::test]
        async fn an_invocation_exports_its_logs_before_lambda_freezes_the_environment() {
            let _provider = APP_LOG_PROVIDER.lock().await;
            let exporter = InMemoryLogExporter::default();
            install_app_log_exporter(exporter.clone());
            crate::otlp::emit_log("stdout", "[e2e-endpoint] release=v2", 0);

            let (request_done, request_done_receiver) = unbounded_channel();
            let extension = WaitUntilExtension::new(request_done_receiver);
            run_invocation(&extension, &request_done, "request-1").await;

            assert_eq!(
                exported_bodies(&exporter),
                vec![body("[e2e-endpoint] release=v2")]
            );
        }

        #[tokio::test]
        async fn an_unanswered_log_endpoint_does_not_hold_every_invocation() {
            let _provider = APP_LOG_PROVIDER.lock().await;
            let endpoint = UnansweredEndpoint {
                delivered: InMemoryLogExporter::default(),
                released: Arc::new((StdMutex::new(false), Condvar::new())),
            };
            install_app_log_exporter(endpoint.clone());
            let _release = ReleaseOnDrop(endpoint.clone());
            let (request_done, request_done_receiver) = unbounded_channel();
            let extension = WaitUntilExtension::new(request_done_receiver);

            crate::otlp::emit_log("stdout", "first", 0);
            let first = run_invocation(&extension, &request_done, "request-1").await;
            crate::otlp::emit_log("stdout", "second", 0);
            let second = run_invocation(&extension, &request_done, "request-2").await;

            // The first invocation waits a bounded time for the endpoint; the second doesn't
            // wait for it again while that export is still outstanding.
            assert!(
                first >= MAX_LOG_EXPORT_WAIT && first < MAX_LOG_EXPORT_WAIT * 2,
                "first invocation held for {first:?}"
            );
            assert!(
                second < Duration::from_millis(100),
                "second invocation held for {second:?}"
            );
            assert_eq!(exported_bodies(&endpoint.delivered), vec![]);

            // Once the endpoint answers, the next invocation exports what the others left.
            endpoint.release();
            let unfinished = extension.unfinished_log_export.lock().await.take();
            unfinished
                .expect("the first export outlived its invocation")
                .await
                .expect("export task completes")
                .expect("export succeeds once the endpoint answers");
            crate::otlp::emit_log("stdout", "third", 0);
            run_invocation(&extension, &request_done, "request-3").await;

            assert_eq!(
                exported_bodies(&endpoint.delivered),
                vec![body("first"), body("second"), body("third")]
            );
        }

        #[test]
        fn the_log_export_wait_is_bounded_and_stops_short_of_the_deadline() {
            let now = UNIX_EPOCH + Duration::from_secs(1_000);
            assert_eq!(log_export_wait(1_010_000, now), MAX_LOG_EXPORT_WAIT);
            assert_eq!(log_export_wait(1_000_600, now), Duration::from_millis(350));
            assert_eq!(log_export_wait(999_000, now), Duration::ZERO);
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

/// Tells the wait_until extension that an invocation is over when dropped.
struct InvocationDone {
    sender: UnboundedSender<()>,
    request_id: String,
}

impl Drop for InvocationDone {
    fn drop(&mut self) {
        if let Err(e) = self.sender.send(()) {
            warn!(
                request_id = %self.request_id,
                error = ?e,
                "Failed to signal request completion to extension"
            );
        }
    }
}

pin_project! {
    /// A streamed response body that ends its invocation once Lambda is done with it: after the
    /// last frame was sent, the stream failed, or the response was abandoned.
    struct SignalWhenDropped<B> {
        #[pin] body: B,
        done: InvocationDone,
    }
}

impl<B: HttpBody> HttpBody for SignalWhenDropped<B> {
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.project().body.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
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
    queue_bindings: Option<alien_bindings::Bindings>,
    queue_sources: std::collections::HashMap<String, String>,
    queue_batches: std::collections::HashMap<String, alien_core::QueueBatchConfig>,
    command_timeout: Duration,
    control_server: Arc<ControlGrpcServer>,
    /// Reused by every invocation so warm invocations keep pooled connections.
    http_client: reqwest::Client,
}

/// Lambda transport
pub struct LambdaTransport {
    command_timeout: Duration,
    mode: LambdaMode,
    control_server: Arc<ControlGrpcServer>,
}

impl LambdaTransport {
    pub fn new(mode: LambdaMode, control_server: Arc<ControlGrpcServer>) -> Self {
        Self {
            mode,
            control_server,
            command_timeout: Duration::from_secs(300),
        }
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Run the Lambda transport
    pub async fn run(self) -> Result<()> {
        info!(mode = ?self.mode, "Starting Lambda transport");

        let (queue_batches, queue_sources) = super::queue_batch::batch_routing_from_env()?;
        let queue_bindings = Some(alien_bindings::Bindings::from_env().context(
            ErrorData::Other {
                message: "Cannot initialize queue bindings".into(),
            },
        )?);
        let state = Arc::new(LambdaState {
            queue_bindings,
            queue_batches,
            queue_sources,
            command_timeout: self.command_timeout,
            control_server: self.control_server,
            // No read timeout: the invocation deadline already bounds each request.
            http_client: forward_client_builder()
                .build()
                .expect("Failed to configure application HTTP forwarding"),
        });

        match self.mode {
            LambdaMode::Streaming => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = StreamingAdapter {
                    state: state.clone(),
                    request_done_sender,
                };
                run_streaming(adapter, request_done_receiver).await
            }
            LambdaMode::Buffered => {
                let (request_done_sender, request_done_receiver) = unbounded_channel::<()>();
                let adapter = BufferedAdapter {
                    state: state.clone(),
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

/// A private marker created only at the trusted Runtime API boundary. HTTP
/// callers cannot opt into task dispatch through paths, headers, or JSON bodies.
#[derive(Clone)]
struct NonHttpInvocation;

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
    req.extensions_mut().insert(NonHttpInvocation);
    req.with_lambda_context(event.context)
}

#[derive(Clone)]
struct StreamingAdapter {
    state: Arc<LambdaState>,
    request_done_sender: UnboundedSender<()>,
}

impl Service<LambdaRequest> for StreamingAdapter {
    type Response = lambda_runtime::FunctionResponse<Value, BodyStream<StreamingBody>>;
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
                // The headers return before the app has finished writing the body, so the
                // invocation ends when Lambda drops the body, or here if there is none.
                let done = InvocationDone {
                    sender: request_done_sender,
                    request_id: request_id.clone(),
                };
                if let Some(result) = handle_native_sqs_request(&state, &request_id, &req).await {
                    return result.map(lambda_runtime::FunctionResponse::BufferedResponse);
                }
                handle_streaming_event(&state, &request_id, req)
                    .await
                    .map(|response| {
                        lambda_runtime::FunctionResponse::StreamingResponse(into_stream_response(
                            response.map(|body| SignalWhenDropped { body, done }.boxed()),
                        ))
                    })
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
    type Response = Value;
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
                let result = match handle_native_sqs_request(&state, &request_id, &req).await {
                    Some(result) => result,
                    None => match handle_buffered_event(&state, &request_id, req).await {
                        Ok(response) => serde_json::to_value(response)
                            .map_err(|error| invocation_failure(error.to_string())),
                        Err(error) => Err(error),
                    },
                };

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
) -> Result<()> {
    info!("run_streaming: Setting up Lambda streaming runtime");

    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(handler);

    drive_lambda_runtime(lambda::run(svc), request_done_receiver, "Streaming").await
}

/// Lambda streams cookies in the response prelude, not as `Set-Cookie` headers.
fn into_stream_response<B>(res: Response<B>) -> StreamResponse<BodyStream<B>> {
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
}

/// Launch Lambda **buffered** runtime.
async fn run_buffered(
    adapter: BufferedAdapter,
    request_done_receiver: UnboundedReceiver<()>,
) -> Result<()> {
    let svc = lambda_runtime::tower::ServiceBuilder::new()
        .map_request(event_to_request)
        .service(adapter);

    drive_lambda_runtime(lambda::run(svc), request_done_receiver, "Buffered").await
}

const INVOCATION_DEADLINE_MARGIN: Duration = Duration::from_secs(1);

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
    if event.extensions().get::<NonHttpInvocation>().is_none() {
        return ClassifiedEvent::Http(Box::new(event));
    }
    let body_bytes = event_body_bytes(&event);

    if let Ok(s3_event) = serde_json::from_slice::<S3Event>(&body_bytes) {
        if !s3_event.records.is_empty()
            && s3_event
                .records
                .iter()
                .all(|record| record.event_source.as_deref() == Some("aws:s3"))
        {
            return ClassifiedEvent::Task(TaskEvent::S3(Box::new(s3_event)));
        }
    }

    if let Ok(sqs_event) = serde_json::from_slice::<SqsEvent>(&body_bytes) {
        if !sqs_event.records.is_empty()
            && sqs_event
                .records
                .iter()
                .all(|record| record.event_source.as_deref() == Some("aws:sqs"))
        {
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
        task_id: format!("{}-{}", request_id, uuid::Uuid::new_v4()),
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

/// Build the QueueMessage tasks for one SQS record via the canonical
/// `events::sqs_message_to_queue_messages` conversion, which extracts
/// timestamp, attempt_count, source, and attributes, and normalizes S3 event
/// notifications. Both buffered and streaming modes route through this, so
/// buffered tasks carry the full field set.
///
/// An S3 notification yields one task per object-created record, all sharing
/// the SQS message ID and receipt; an `s3:TestEvent` yields no task and the
/// SQS message counts as processed.
fn sqs_record_to_tasks(
    request_id: &str,
    record: aws_lambda_events::sqs::SqsMessage,
) -> Result<Vec<Task>> {
    let queue_messages = crate::events::sqs_message_to_queue_messages(record)?;
    queue_messages
        .into_iter()
        .map(|queue_msg| {
            let timestamp = Some(prost_types::Timestamp {
                seconds: queue_msg.timestamp.timestamp(),
                nanos: queue_msg.timestamp.timestamp_subsec_nanos() as i32,
            });

            let payload_bytes = match &queue_msg.payload {
                alien_core::MessagePayload::Json(v) => serde_json::to_vec(v).unwrap_or_default(),
                alien_core::MessagePayload::Text(s) => s.as_bytes().to_vec(),
            };

            Ok(Task {
                task_id: format!("{}-{}", request_id, uuid::Uuid::new_v4()),
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
        })
        .collect()
}

/// One SQS delivery in a Lambda event: its message ID and receipt handle.
type SqsDelivery = (String, String);

/// Report an SQS delivery as failed once, however many of its events failed.
fn record_sqs_failure(failures: &mut Vec<SqsDelivery>, message_id: &str, receipt: &str) {
    let delivery = (message_id.to_string(), receipt.to_string());
    if !failures.contains(&delivery) {
        failures.push(delivery);
    }
}

/// Send an event task with the configured Worker timeout, logging an error on
/// either a non-success result or a transport failure. Shared by the S3,
/// SQS, and CloudWatch handlers, which differ only in the task built and the
/// `desc` used in log messages.
async fn send_event_task(state: &LambdaState, task: Task, desc: &str) -> Result<()> {
    let task_id = task.task_id.clone();
    let result = state
        .control_server
        .send_task(task, state.command_timeout)
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

/// SQS replies are native Lambda payloads even when HTTP responses use streaming.
async fn handle_native_sqs_request(
    state: &LambdaState,
    request_id: &str,
    request: &LambdaRequest,
) -> Option<std::result::Result<Value, LambdaError>> {
    if request.extensions().get::<NonHttpInvocation>().is_none() {
        return None;
    }
    let event = serde_json::from_slice::<SqsEvent>(request.body().as_ref()).ok()?;
    if event.records.is_empty()
        || !event
            .records
            .iter()
            .all(|record| record.event_source.as_deref() == Some("aws:sqs"))
    {
        return None;
    }
    let deadline = tokio::time::Instant::now()
        + invocation_readiness_budget(request).min(state.command_timeout);
    let operation = async {
        wait_for_task_readiness(
            state,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await?;
        // SQS deliveries to retry. One SQS message can carry several events (an
        // S3 notification with several records), so successes are tracked per
        // event and a delivery completes only when none of its events failed.
        let mut failures: Vec<SqsDelivery> = Vec::new();
        let mut deliveries = Vec::new();
        // (logical source, SQS delivery) of each successful event.
        let mut succeeded: Vec<(String, SqsDelivery)> = Vec::new();
        for record in event.records {
            let source = record
                .event_source_arn
                .as_deref()
                .and_then(|arn| arn.rsplit(':').next())
                .unwrap_or_default();
            let logical_source = state
                .queue_sources
                .get(source)
                .map(String::as_str)
                .unwrap_or(source)
                .to_string();
            let batch_configured = state.queue_batches.contains_key(&logical_source);
            let id = record
                .message_id
                .clone()
                .ok_or_else(|| invocation_failure("SQS record missing message ID"))?;
            let receipt = record.receipt_handle.clone().unwrap_or_default();
            if let Some(envelope) = record
                .body
                .as_deref()
                .and_then(|body| serde_json::from_str::<alien_commands::types::Envelope>(body).ok())
                .filter(|envelope| envelope.protocol == alien_commands::PROTOCOL_VERSION)
            {
                if handle_command(state, &envelope).await.is_err() {
                    record_sqs_failure(&mut failures, &id, &receipt);
                } else {
                    succeeded.push((logical_source, (id, receipt)));
                }
                continue;
            }
            let tasks = sqs_record_to_tasks(request_id, record)
                .map_err(|error| invocation_failure(error.to_string()))?;
            if tasks.is_empty() {
                // An S3 test event: there is nothing to handle, so it is processed.
                succeeded.push((logical_source, (id, receipt)));
                continue;
            }
            for task in tasks {
                let Some(Payload::QueueMessage(mut message)) = task.payload else {
                    return Err(invocation_failure("Invalid queue task conversion"));
                };
                if batch_configured {
                    message.source = logical_source.clone();
                    deliveries.push(alien_worker_protocol::control::QueueDelivery {
                        delivery_id: task.task_id,
                        message: Some(message),
                    });
                } else {
                    message.source = super::shared::single_queue_source(
                        &state.control_server,
                        &message.source,
                        &logical_source,
                    )
                    .await;
                    let task = Task {
                        task_id: task.task_id,
                        payload: Some(Payload::QueueMessage(message)),
                    };
                    if send_event_task(state, task, "queue message").await.is_err() {
                        record_sqs_failure(&mut failures, &id, &receipt);
                    } else {
                        succeeded.push((logical_source.clone(), (id.clone(), receipt.clone())));
                    }
                }
            }
        }
        let mut groups: std::collections::BTreeMap<
            String,
            Vec<alien_worker_protocol::control::QueueDelivery>,
        > = Default::default();
        for delivery in deliveries {
            let message = delivery
                .message
                .as_ref()
                .ok_or_else(|| invocation_failure("Missing queue message"))?;
            groups
                .entry(message.source.clone())
                .or_default()
                .push(delivery);
        }
        for (source, deliveries) in groups {
            let config = &state.queue_batches[&source];
            let mut remaining = deliveries.into_iter().peekable();
            while remaining.peek().is_some() {
                let mut batch = alien_worker_protocol::control::QueueBatch {
                    source: source.clone(),
                    deliveries: Vec::new(),
                };
                let mut size = source.len() + 64;
                while let Some(delivery) = remaining.peek() {
                    let delivery_size = delivery.encoded_len() + 16;
                    if batch.deliveries.len() >= config.max_messages as usize
                        || size + delivery_size > super::queue_batch::MAX_BATCH_BYTES
                    {
                        if batch.deliveries.is_empty() {
                            let delivery = remaining
                                .next()
                                .ok_or_else(|| invocation_failure("Missing oversized delivery"))?;
                            let message = delivery
                                .message
                                .ok_or_else(|| invocation_failure("Missing oversized message"))?;
                            record_sqs_failure(&mut failures, &message.id, &message.receipt_handle);
                        }
                        break;
                    }
                    size += delivery_size;
                    batch.deliveries.push(
                        remaining
                            .next()
                            .ok_or_else(|| invocation_failure("Missing queued delivery"))?,
                    );
                }
                if batch.deliveries.is_empty() {
                    continue;
                }
                let result = state
                    .control_server
                    .send_task(
                        Task {
                            task_id: uuid::Uuid::new_v4().to_string(),
                            payload: Some(Payload::QueueBatch(batch.clone())),
                        },
                        deadline.saturating_duration_since(tokio::time::Instant::now()),
                    )
                    .await;
                let dispositions =
                    result
                        .ok()
                        .and_then(|result| result.queue_batch)
                        .and_then(|result| {
                            alien_worker_protocol::batch::validate_outcomes(&batch, &result)
                        });
                for (index, delivery) in batch.deliveries.iter().enumerate() {
                    let delivered = dispositions.as_ref().is_some_and(|outcomes|
                        outcomes[index] == alien_worker_protocol::control::queue_delivery_outcome::Disposition::Success);
                    let message = delivery
                        .message
                        .as_ref()
                        .ok_or_else(|| invocation_failure("Missing result message"))?;
                    if delivered {
                        succeeded.push((
                            source.clone(),
                            (message.id.clone(), message.receipt_handle.clone()),
                        ));
                    } else {
                        record_sqs_failure(&mut failures, &message.id, &message.receipt_handle);
                    }
                }
            }
        }
        // A legacy mapping ignores batchItemFailures, including during upgrades.
        // On mixed outcomes, delete only validated successes using their actual
        // delivery receipts, then fail the invocation. Failed/uncertain receipts
        // remain retryable regardless of the mapping's response mode.
        if !failures.is_empty() {
            // Deleting an SQS message for one successful event would lose its
            // failed siblings, so only messages without failures are deleted.
            let mut successful_receipts: std::collections::BTreeMap<String, Vec<String>> =
                Default::default();
            let mut completed = std::collections::HashSet::new();
            for (source, delivery) in succeeded {
                if !failures.contains(&delivery) && completed.insert(delivery.clone()) {
                    successful_receipts
                        .entry(source)
                        .or_default()
                        .push(delivery.1);
                }
            }
            for (source, receipts) in successful_receipts {
                if receipts.iter().any(String::is_empty) {
                    return Err(invocation_failure("Successful SQS delivery has no receipt"));
                }
                let bindings = state.queue_bindings.as_ref().ok_or_else(|| {
                    invocation_failure("Queue bindings unavailable for completion")
                })?;
                let queue = bindings
                    .queue(&source)
                    .await
                    .map_err(|error| invocation_failure(error.to_string()))?;
                let outcomes = queue
                    .ack_batch(&receipts)
                    .await
                    .map_err(|error| invocation_failure(error.to_string()))?;
                if outcomes.len() != receipts.len()
                    || outcomes.iter().any(|outcome| {
                        !matches!(
                            outcome,
                            alien_bindings::traits::QueueAckResult::Acknowledged
                        )
                    })
                {
                    return Err(invocation_failure(
                        "Some SQS completion results are uncertain; retry the invocation",
                    ));
                }
            }
            return Err(invocation_failure(
                "SQS delivery failed; retry remaining receipts",
            ));
        }
        // Every failure returned above, so no SQS message is reported as failed.
        Ok(serde_json::json!({"batchItemFailures": []}))
    };
    Some(match tokio::time::timeout_at(deadline, operation).await {
        Ok(result) => result,
        Err(_) => {
            state.control_server.cancel_application().await;
            Err(invocation_failure(
                "SQS execution exceeded invocation budget",
            ))
        }
    })
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

        for task in sqs_record_to_tasks(request_id, record)? {
            send_event_task(state, task, "queue message").await?;
        }
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
    let budget =
        super::shared::CommandBudget::from_envelope(state.command_timeout, envelope.deadline);
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
        task_id: uuid::Uuid::new_v4().to_string(),
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

    let Some(timeout) = budget.and_then(super::shared::CommandBudget::remaining) else {
        submit_response(
            envelope,
            CommandResponse::error(
                "COMMAND_EXPIRED",
                "Command deadline elapsed before application execution",
            ),
        )
        .await
        .map_err(|error| {
            AlienError::new(ErrorData::ResponseDeliveryFailed {
                request_id: command_id,
                message: error.to_string(),
                destination: Some("command response endpoint".to_string()),
            })
        })?;
        return Ok(());
    };
    debug!(command_id = %command_id, "Sending command task to application via gRPC");
    match state.control_server.send_task(task, timeout).await {
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
    client: &reqwest::Client,
    app_port: u16,
    event: LambdaRequest,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
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
    let task_deadline = tokio::time::Instant::now() + readiness_budget.min(state.command_timeout);

    match classify_event(event) {
        ClassifiedEvent::Task(task_event) => {
            wait_for_task_readiness(
                state,
                task_deadline.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await?;
            match tokio::time::timeout_at(
                task_deadline,
                dispatch_task_event(state, request_id, task_event),
            )
            .await
            {
                Ok(result) => result.map_err(|error| invocation_failure(error.to_string()))?,
                Err(_) => {
                    state.control_server.cancel_application().await;
                    return Err(invocation_failure(
                        "Application execution exceeded invocation budget",
                    ));
                }
            }
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
    match forward_to_app(&state.http_client, app_port, event).await {
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
    let task_deadline = tokio::time::Instant::now() + readiness_budget.min(state.command_timeout);

    match classify_event(event) {
        ClassifiedEvent::Task(task_event) => {
            wait_for_task_readiness(
                state,
                task_deadline.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await?;
            match tokio::time::timeout_at(
                task_deadline,
                dispatch_task_event(state, request_id, task_event),
            )
            .await
            {
                Ok(result) => result.map_err(|error| invocation_failure(error.to_string()))?,
                Err(_) => {
                    state.control_server.cancel_application().await;
                    return Err(invocation_failure(
                        "Application execution exceeded invocation budget",
                    ));
                }
            }
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
    match forward_to_app(&state.http_client, app_port, event).await {
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
    use alien_worker_protocol::control::{
        control_service_server::ControlService, send_task_result_request, SendTaskResultRequest,
        TaskError, TaskSuccess,
    };

    use super::*;

    #[tokio::test]
    async fn a_streamed_invocation_ends_when_its_body_is_done_not_at_the_headers() {
        let (sender, mut receiver) = unbounded_channel();
        let body = SignalWhenDropped {
            body: Full::new(Bytes::from_static(b"streamed")),
            done: InvocationDone {
                sender,
                request_id: "request-1".to_string(),
            },
        };
        assert!(
            receiver.try_recv().is_err(),
            "headers alone must not end it"
        );
        let collected = body.collect().await.expect("body streams").to_bytes();
        assert_eq!(collected, Bytes::from_static(b"streamed"));
        assert_eq!(receiver.try_recv(), Ok(()));
    }

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

        let task =
            s3_record_to_task("req-1", record.clone()).expect("task conversion should succeed");
        let redelivery = s3_record_to_task("req-1", record).expect("repeat conversion");
        assert_ne!(
            task.task_id, redelivery.task_id,
            "each delivery needs distinct correlation"
        );

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

        let [task]: [Task; 1] = sqs_record_to_tasks("req-1", record.clone())
            .expect("task conversion should succeed")
            .try_into()
            .expect("an ordinary message is one task");
        let [redelivery]: [Task; 1] = sqs_record_to_tasks("req-1", record)
            .expect("repeat conversion")
            .try_into()
            .expect("an ordinary message is one task");
        assert_ne!(
            task.task_id, redelivery.task_id,
            "each delivery needs distinct correlation"
        );

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
        let s3_request = event_to_request(LambdaEvent::new(
            s3_event_json(),
            lambda_runtime::Context::default(),
        ));
        match classify_event(s3_request) {
            ClassifiedEvent::Task(TaskEvent::S3(event)) => assert_eq!(event.records.len(), 1),
            _ => panic!("S3 payload must classify as an S3 task event"),
        }

        let sqs_request = event_to_request(LambdaEvent::new(
            sqs_event_json(),
            lambda_runtime::Context::default(),
        ));
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
    async fn legacy_sqs_failure_remains_invocation_error_in_both_adapters() {
        for streaming in [false, true] {
            let control = Arc::new(ControlGrpcServer::new());
            // Registration alone cannot establish the event source mapping's response mode.
            control
                .register_event_handler(tonic::Request::new(
                    alien_worker_protocol::control::RegisterEventHandlerRequest {
                        handler_type: "queueBatch".into(),
                        resource_name: "my-queue".into(),
                    },
                ))
                .await
                .expect("register");
            let mut tasks = control
                .wait_for_tasks(tonic::Request::new(
                    alien_worker_protocol::control::WaitForTasksRequest {
                        application_id: "test".into(),
                    },
                ))
                .await
                .expect("subscribe")
                .into_inner();
            let app_control = control.clone();
            let application = tokio::spawn(async move {
                let task = tokio_stream::StreamExt::next(&mut tasks)
                    .await
                    .expect("task")
                    .expect("delivery");
                assert!(matches!(task.payload, Some(Payload::QueueMessage(_))));
                app_control.send_task_result(tonic::Request::new(alien_worker_protocol::control::SendTaskResultRequest {
                    task_id: task.task_id,
                    result: Some(alien_worker_protocol::control::send_task_result_request::Result::Error(alien_worker_protocol::control::TaskError { code: "RETRY".into(), message: "Processing failed".into() })),
                })).await.expect("result");
            });
            let state = Arc::new(LambdaState {
                queue_bindings: None,
                queue_sources: Default::default(),
                queue_batches: Default::default(),
                command_timeout: Duration::from_secs(30),
                control_server: control,
                http_client: forward_client_builder().build().expect("client"),
            });
            let (sender, _receiver) = unbounded_channel();
            let mut context = lambda_runtime::Context::default();
            context.deadline = (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis()
                + 30_000) as u64;
            let request = event_to_request(LambdaEvent::new(sqs_event_json(), context));
            if streaming {
                let mut adapter = StreamingAdapter {
                    state,
                    request_done_sender: sender,
                };
                assert!(
                    adapter.call(request).await.is_err(),
                    "legacy failures must retry, even in streaming mode"
                );
            } else {
                let mut adapter = BufferedAdapter {
                    state,
                    request_done_sender: sender,
                };
                assert!(
                    adapter.call(request).await.is_err(),
                    "legacy mappings ignore failure lists"
                );
            }
            application.await.expect("application");
        }
    }

    #[tokio::test]
    async fn single_sqs_routing_preserves_registered_names_and_wildcards() {
        for streaming in [false, true] {
            for (registrations, expected_source) in [
                (vec!["my-queue", "jobs"], "my-queue"),
                (vec!["jobs"], "jobs"),
                (vec!["*"], "my-queue"),
            ] {
                for success in [false, true] {
                    let control = Arc::new(ControlGrpcServer::new());
                    for &registration in &registrations {
                        control
                            .register_event_handler(tonic::Request::new(
                                alien_worker_protocol::control::RegisterEventHandlerRequest {
                                    handler_type: "queue".into(),
                                    resource_name: registration.into(),
                                },
                            ))
                            .await
                            .expect("register");
                    }
                    let mut tasks = control
                        .wait_for_tasks(tonic::Request::new(
                            alien_worker_protocol::control::WaitForTasksRequest {
                                application_id: "test".into(),
                            },
                        ))
                        .await
                        .expect("subscribe")
                        .into_inner();
                    let app_control = control.clone();
                    let application = tokio::spawn(async move {
                        let task = tokio_stream::StreamExt::next(&mut tasks)
                            .await
                            .expect("task")
                            .expect("delivery");
                        let Some(Payload::QueueMessage(message)) = task.payload else {
                            panic!("single delivery expected");
                        };
                        assert_eq!(message.source, expected_source);
                        assert_eq!(message.id, "msg-1");
                        assert_eq!(message.receipt_handle, "rh-1");
                        let result = if success {
                            send_task_result_request::Result::Success(TaskSuccess {
                                response_data: Vec::new(),
                            })
                        } else {
                            send_task_result_request::Result::Error(TaskError {
                                code: "RETRY".into(),
                                message: "Processing failed".into(),
                            })
                        };
                        app_control
                            .send_task_result(tonic::Request::new(SendTaskResultRequest {
                                task_id: task.task_id,
                                result: Some(result),
                            }))
                            .await
                            .expect("result");
                    });
                    let state = Arc::new(LambdaState {
                        queue_bindings: None,
                        queue_sources: [("my-queue".into(), "jobs".into())].into(),
                        queue_batches: Default::default(),
                        command_timeout: Duration::from_secs(30),
                        control_server: control,
                        http_client: forward_client_builder().build().expect("client"),
                    });
                    let (sender, _receiver) = unbounded_channel();
                    let mut context = lambda_runtime::Context::default();
                    context.deadline = (SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_millis()
                        + 30_000) as u64;
                    let request = event_to_request(LambdaEvent::new(sqs_event_json(), context));
                    if streaming {
                        let mut adapter = StreamingAdapter {
                            state,
                            request_done_sender: sender,
                        };
                        assert_eq!(adapter.call(request).await.is_ok(), success);
                    } else {
                        let mut adapter = BufferedAdapter {
                            state,
                            request_done_sender: sender,
                        };
                        assert_eq!(adapter.call(request).await.is_ok(), success);
                    }
                    application.await.expect("application");
                }
            }
        }
    }

    #[tokio::test]
    async fn configured_sqs_outcomes_preserve_receipts_in_both_adapters() {
        for streaming in [false, true] {
            // 0: all success; 1: mixed; 2: invalid result; 3: uncertain deletion.
            for scenario in 0..4 {
                let deletes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let calls = deletes.clone();
                let app = axum::Router::new().fallback(axum::routing::post(move |body: String| {
                    let calls = calls.clone();
                    async move {
                        let form: std::collections::HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                        assert_eq!(form["Action"], "DeleteMessageBatch");
                        assert_eq!(form["DeleteMessageBatchRequestEntry.1.ReceiptHandle"], "rh-1");
                        assert!(!form.contains_key("DeleteMessageBatchRequestEntry.2.ReceiptHandle"));
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let entry = if scenario == 3 {
                            "<BatchResultErrorEntry><Id>0</Id><Code>InternalError</Code><SenderFault>false</SenderFault></BatchResultErrorEntry>"
                        } else {
                            "<DeleteMessageBatchResultEntry><Id>0</Id></DeleteMessageBatchResultEntry>"
                        };
                        format!("<DeleteMessageBatchResponse><DeleteMessageBatchResult>{entry}</DeleteMessageBatchResult></DeleteMessageBatchResponse>")
                    }
                }));
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("listen");
                let endpoint = format!("http://{}", listener.local_addr().expect("address"));
                let server = tokio::spawn(async move {
                    axum::serve(listener, app).await.expect("SQS fixture");
                });
                let environment = std::collections::HashMap::from([
                    ("ALIEN_DEPLOYMENT_TYPE".into(), "aws".into()),
                    ("AWS_ACCOUNT_ID".into(), "123456789012".into()),
                    ("AWS_REGION".into(), "us-east-1".into()),
                    ("AWS_ACCESS_KEY_ID".into(), "test-access-key".into()),
                    ("AWS_SECRET_ACCESS_KEY".into(), "test-secret-key".into()),
                    ("AWS_SERVICE_OVERRIDES_ENDPOINTS".into(), serde_json::json!({"sqs": endpoint}).to_string()),
                    ("ALIEN_QUEUE_BATCH_CONFIG".into(), r#"{"jobs":{"maxMessages":10,"maxWindowSeconds":0}}"#.into()),
                    ("ALIEN_JOBS_BINDING".into(), serde_json::json!({"service":"sqs","queueUrl": format!("{endpoint}/123456789012/run-a7d9-imported-jobs")}).to_string()),
                ]);
                let (queue_batches, queue_sources) =
                    super::super::queue_batch::batch_routing(&environment).expect("source mapping");
                let control = Arc::new(ControlGrpcServer::new());
                let mut tasks = control
                    .wait_for_tasks(tonic::Request::new(
                        alien_worker_protocol::control::WaitForTasksRequest {
                            application_id: "test".into(),
                        },
                    ))
                    .await
                    .expect("subscribe")
                    .into_inner();
                let app_control = control.clone();
                let application = tokio::spawn(async move {
                    let task = tokio_stream::StreamExt::next(&mut tasks)
                        .await
                        .expect("task")
                        .expect("delivery");
                    let Some(Payload::QueueBatch(batch)) = task.payload else {
                        panic!("typed batch");
                    };
                    assert_eq!(batch.source, "jobs");
                    assert_eq!(batch.deliveries.len(), 2);
                    assert_ne!(
                        batch.deliveries[0].delivery_id,
                        batch.deliveries[1].delivery_id
                    );
                    for delivery in &batch.deliveries {
                        assert_eq!(delivery.message.as_ref().expect("message").source, "jobs");
                    }
                    let mut outcomes: Vec<_> = batch
                        .deliveries
                        .into_iter()
                        .enumerate()
                        .map(|(index, delivery)| {
                            alien_worker_protocol::control::QueueDeliveryOutcome {
                                delivery_id: delivery.delivery_id,
                                disposition: if scenario == 0 || index == 0 { 1 } else { 2 },
                            }
                        })
                        .collect();
                    if scenario == 2 {
                        outcomes[1].delivery_id = outcomes[0].delivery_id.clone();
                    }
                    app_control.send_task_result(tonic::Request::new(alien_worker_protocol::control::SendTaskResultRequest {
                        task_id: task.task_id,
                        result: Some(alien_worker_protocol::control::send_task_result_request::Result::QueueBatch(alien_worker_protocol::control::QueueBatchResult { outcomes })),
                    })).await.expect("result");
                });
                let state = Arc::new(LambdaState {
                    queue_bindings: Some(
                        alien_bindings::Bindings::from_env_map(environment).expect("bindings"),
                    ),
                    queue_batches,
                    queue_sources,
                    command_timeout: Duration::from_secs(30),
                    control_server: control,
                    http_client: forward_client_builder().build().expect("client"),
                });
                let (sender, _receiver) = unbounded_channel();
                let mut context = lambda_runtime::Context::default();
                context.deadline = (SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_millis()
                    + 30_000) as u64;
                let mut event = sqs_event_json();
                let mut second = event["Records"][0].clone();
                second["receiptHandle"] = serde_json::json!("rh-2");
                event["Records"]
                    .as_array_mut()
                    .expect("records")
                    .push(second);
                let request = event_to_request(LambdaEvent::new(
                    serde_json::from_str(
                        &event
                            .to_string()
                            .replace("my-queue", "run-a7d9-imported-jobs"),
                    )
                    .expect("event"),
                    context,
                ));
                let response = if streaming {
                    let mut adapter = StreamingAdapter {
                        state,
                        request_done_sender: sender,
                    };
                    adapter.call(request).await.map(|response| match response {
                        lambda_runtime::FunctionResponse::BufferedResponse(value) => value,
                        _ => panic!("SQS must return raw buffered JSON"),
                    })
                } else {
                    BufferedAdapter {
                        state,
                        request_done_sender: sender,
                    }
                    .call(request)
                    .await
                };
                if scenario == 0 {
                    assert_eq!(
                        response.expect("all deliveries successful"),
                        serde_json::json!({"batchItemFailures": []})
                    );
                } else {
                    assert!(
                        response.is_err(),
                        "mapping response mode must never hide retries"
                    );
                }
                application.await.expect("application");
                assert_eq!(
                    deletes.load(std::sync::atomic::Ordering::SeqCst),
                    usize::from(scenario == 1 || scenario == 3)
                );
                server.abort();
                assert!(server.await.expect_err("fixture stopped").is_cancelled());
            }
        }
    }

    /// One S3 record as S3 delivered it to SQS, captured from a real bucket.
    fn s3_record(encoded_key: &str) -> serde_json::Value {
        serde_json::json!({
            "eventVersion": "2.6",
            "eventSource": "aws:s3",
            "awsRegion": "us-east-2",
            "eventTime": "2026-10-30T18:28:41.358Z",
            "eventName": "ObjectCreated:Put",
            "userIdentity": {"principalId": "AWS:AIDATLKJAW4BZIY5MKZTX"},
            "requestParameters": {"sourceIPAddress": "18.212.70.151"},
            "responseElements": {"x-amz-request-id": "X1E9MRMH58NVARK1", "x-amz-id-2": "s4fAzHTIcUxF5js4fKVOE3yftUageoO"},
            "s3": {
                "s3SchemaVersion": "1.0",
                "configurationId": "rule-0",
                "bucket": {"name": "n2081-s3d-78f48e", "ownerIdentity": {"principalId": "A5OBGC8W2F4L2"}, "arn": "arn:aws:s3:::n2081-s3d-78f48e"},
                "object": {"key": encoded_key, "size": 7, "eTag": "321c3cf486ed509164edec1e1981fec8", "sequencer": "006AE4E1D955757FA5"}
            }
        })
    }

    /// An SQS batch holding an S3 test event, an S3 notification with two records,
    /// and an ordinary message must hand the app exactly the two StorageEvents and
    /// the ordinary message, on both single and batched routes. The test event is
    /// processed without reaching the app; the two StorageEvents share their SQS
    /// message, so when one fails the other's success must not delete it.
    #[tokio::test]
    async fn sqs_s3_notifications_reach_the_app_as_storage_events() {
        let test_event = serde_json::json!({
            "Service": "Amazon S3", "Event": "s3:TestEvent", "Time": "2026-10-30T18:28:30.022Z",
            "Bucket": "n2081-s3d-78f48e", "RequestId": "5582815E1AEA5ADF", "HostId": "8cLeGAmw098X5cv4Zkwcmo8vvZa3eH3eKxsPzbB9wrR"
        });
        let notification = serde_json::json!({
            "Records": [s3_record("in/with+space.json"), s3_record("in/plus%2Bsign.json")]
        });
        let records: Vec<_> = [
            ("test-event", test_event.to_string()),
            ("s3-put", notification.to_string()),
            ("ordinary", r#"{"hello":"world"}"#.to_string()),
        ]
        .into_iter()
        .map(|(id, body)| {
            serde_json::json!({
                "messageId": id,
                "receiptHandle": format!("rh-{id}"),
                "body": body,
                "attributes": {"ApproximateReceiveCount": "1", "SentTimestamp": "1704110400000"},
                "messageAttributes": {},
                "eventSourceARN": "arn:aws:sqs:us-east-1:123456789012:run-a7d9-imported-jobs",
                "eventSource": "aws:sqs",
                "awsRegion": "us-east-1"
            })
        })
        .collect();
        let event = serde_json::json!({ "Records": records });

        for streaming in [false, true] {
            for batched in [false, true] {
                for fail_sibling in [false, true] {
                    let deleted = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
                    let calls = deleted.clone();
                    let sqs = axum::Router::new().fallback(axum::routing::post(move |body: String| {
                        let calls = calls.clone();
                        async move {
                            let form: std::collections::HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                            assert_eq!(form["Action"], "DeleteMessageBatch");
                            let mut entries = String::new();
                            for index in 0.. {
                                let Some(receipt) = form.get(&format!("DeleteMessageBatchRequestEntry.{}.ReceiptHandle", index + 1)) else {
                                    break;
                                };
                                calls.lock().expect("deleted").push(receipt.clone());
                                entries.push_str(&format!("<DeleteMessageBatchResultEntry><Id>{index}</Id></DeleteMessageBatchResultEntry>"));
                            }
                            format!("<DeleteMessageBatchResponse><DeleteMessageBatchResult>{entries}</DeleteMessageBatchResult></DeleteMessageBatchResponse>")
                        }
                    }));
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                        .await
                        .expect("listen");
                    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
                    let server = tokio::spawn(async move {
                        axum::serve(listener, sqs).await.expect("SQS fixture");
                    });
                    let environment = std::collections::HashMap::from([
                        ("ALIEN_DEPLOYMENT_TYPE".into(), "aws".into()),
                        ("AWS_ACCOUNT_ID".into(), "123456789012".into()),
                        ("AWS_REGION".into(), "us-east-1".into()),
                        ("AWS_ACCESS_KEY_ID".into(), "test-access-key".into()),
                        ("AWS_SECRET_ACCESS_KEY".into(), "test-secret-key".into()),
                        ("AWS_SERVICE_OVERRIDES_ENDPOINTS".into(), serde_json::json!({"sqs": endpoint}).to_string()),
                        ("ALIEN_JOBS_BINDING".into(), serde_json::json!({"service":"sqs","queueUrl": format!("{endpoint}/123456789012/run-a7d9-imported-jobs")}).to_string()),
                        ("ALIEN_QUEUE_BATCH_CONFIG".into(), if batched {
                            r#"{"jobs":{"maxMessages":10,"maxWindowSeconds":0}}"#
                        } else {
                            r#"{"jobs":null}"#
                        }.into()),
                    ]);
                    let (queue_batches, queue_sources) =
                        super::super::queue_batch::batch_routing(&environment)
                            .expect("source mapping");
                    let control = Arc::new(ControlGrpcServer::new());
                    if !batched {
                        control
                            .register_event_handler(tonic::Request::new(
                                alien_worker_protocol::control::RegisterEventHandlerRequest {
                                    handler_type: "queue".into(),
                                    resource_name: "jobs".into(),
                                },
                            ))
                            .await
                            .expect("register");
                    }
                    let mut tasks = control
                        .wait_for_tasks(tonic::Request::new(
                            alien_worker_protocol::control::WaitForTasksRequest {
                                application_id: "test".into(),
                            },
                        ))
                        .await
                        .expect("subscribe")
                        .into_inner();
                    let app_control = control.clone();
                    let application = tokio::spawn(async move {
                        let mut seen = Vec::new();
                        while seen.len() < 3 {
                            let task = tokio_stream::StreamExt::next(&mut tasks)
                                .await
                                .expect("task")
                                .expect("delivery");
                            let result = match task.payload {
                                Some(Payload::QueueBatch(batch)) => {
                                    assert!(batched, "single routes receive single messages");
                                    assert_eq!(batch.source, "jobs");
                                    let outcomes = batch
                                        .deliveries
                                        .into_iter()
                                        .map(|delivery| {
                                            let fail = fail_sibling && seen.len() == 1;
                                            seen.push(delivery.message.expect("message"));
                                            alien_worker_protocol::control::QueueDeliveryOutcome {
                                                delivery_id: delivery.delivery_id,
                                                disposition: if fail { 2 } else { 1 },
                                            }
                                        })
                                        .collect();
                                    send_task_result_request::Result::QueueBatch(
                                        alien_worker_protocol::control::QueueBatchResult {
                                            outcomes,
                                        },
                                    )
                                }
                                Some(Payload::QueueMessage(message)) => {
                                    assert!(!batched, "batched routes receive batches");
                                    let fail = fail_sibling && seen.len() == 1;
                                    seen.push(message);
                                    if fail {
                                        send_task_result_request::Result::Error(TaskError {
                                            code: "RETRY".into(),
                                            message: "Processing failed".into(),
                                        })
                                    } else {
                                        send_task_result_request::Result::Success(TaskSuccess {
                                            response_data: Vec::new(),
                                        })
                                    }
                                }
                                other => panic!("unexpected task {other:?}"),
                            };
                            app_control
                                .send_task_result(tonic::Request::new(SendTaskResultRequest {
                                    task_id: task.task_id,
                                    result: Some(result),
                                }))
                                .await
                                .expect("result");
                        }
                        seen
                    });
                    let state = Arc::new(LambdaState {
                        queue_bindings: Some(
                            alien_bindings::Bindings::from_env_map(environment).expect("bindings"),
                        ),
                        queue_batches,
                        queue_sources,
                        command_timeout: Duration::from_secs(30),
                        control_server: control,
                        http_client: forward_client_builder().build().expect("client"),
                    });
                    let (sender, _receiver) = unbounded_channel();
                    let mut context = lambda_runtime::Context::default();
                    context.deadline = (SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("clock")
                        .as_millis()
                        + 30_000) as u64;
                    let request = event_to_request(LambdaEvent::new(event.clone(), context));
                    let response = if streaming {
                        StreamingAdapter {
                            state,
                            request_done_sender: sender,
                        }
                        .call(request)
                        .await
                        .map(|response| match response {
                            lambda_runtime::FunctionResponse::BufferedResponse(value) => value,
                            _ => panic!("SQS must return raw buffered JSON"),
                        })
                    } else {
                        BufferedAdapter {
                            state,
                            request_done_sender: sender,
                        }
                        .call(request)
                        .await
                    };

                    let seen = application.await.expect("application");
                    let mut keys = Vec::new();
                    for message in &seen[..2] {
                        assert_eq!(message.id, "s3-put");
                        assert_eq!(message.receipt_handle, "rh-s3-put");
                        assert_eq!(message.source, "jobs");
                        let event: alien_core::StorageEvent =
                            serde_json::from_slice(&message.payload).expect("StorageEvent");
                        assert_eq!(event.event_type, alien_core::StorageEventType::Created);
                        assert_eq!(event.bucket_name, "n2081-s3d-78f48e");
                        assert_eq!(event.size, Some(7));
                        keys.push(event.object_key);
                    }
                    assert_eq!(keys, ["in/with space.json", "in/plus+sign.json"]);
                    assert_eq!(seen[2].id, "ordinary");
                    assert_eq!(seen[2].receipt_handle, "rh-ordinary");
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&seen[2].payload)
                            .expect("ordinary JSON"),
                        serde_json::json!({"hello": "world"})
                    );
                    if fail_sibling {
                        assert!(response.is_err(), "the failed S3 event must be retried");
                        // The test event and the ordinary message are done; the S3
                        // message keeps its receipt so both of its events are retried.
                        assert_eq!(
                            *deleted.lock().expect("deleted"),
                            ["rh-test-event", "rh-ordinary"]
                        );
                    } else {
                        assert_eq!(
                            response.expect("every SQS message processed"),
                            serde_json::json!({"batchItemFailures": []})
                        );
                        assert!(deleted.lock().expect("deleted").is_empty());
                    }
                    server.abort();
                    assert!(server.await.expect_err("fixture stopped").is_cancelled());
                }
            }
        }
    }

    #[tokio::test]
    async fn unready_http_returns_retryable_sanitized_503_in_both_modes() {
        let state = LambdaState {
            queue_bindings: None,
            queue_sources: Default::default(),
            queue_batches: Default::default(),
            command_timeout: Duration::from_secs(300),
            control_server: Arc::new(ControlGrpcServer::new()),
            http_client: forward_client_builder().build().unwrap(),
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

    /// Runs an API Gateway v2 / Function URL event through the same parsing and
    /// classification as a real invocation, with the app registered on `app_port`.
    async fn http_invocation(method: &str, app_port: u16) -> (LambdaState, LambdaRequest) {
        http_invocation_with_body(
            method,
            app_port,
            "/redirect",
            "original request body".into(),
        )
        .await
    }

    async fn http_invocation_with_body(
        method: &str,
        app_port: u16,
        path: &str,
        body: String,
    ) -> (LambdaState, LambdaRequest) {
        let control_server = Arc::new(ControlGrpcServer::new());
        control_server
            .register_http_server(tonic::Request::new(
                alien_worker_protocol::control::RegisterHttpServerRequest {
                    port: app_port.into(),
                },
            ))
            .await
            .expect("app HTTP server registers");
        let state = LambdaState {
            queue_bindings: None,
            queue_sources: Default::default(),
            queue_batches: Default::default(),
            command_timeout: Duration::from_secs(300),
            control_server,
            http_client: forward_client_builder().build().unwrap(),
        };
        let payload = serde_json::json!({
            "version": "2.0",
            "routeKey": "$default",
            "rawPath": path,
            "rawQueryString": "keep=query",
            "headers": {"host": "worker.lambda-url.us-east-1.on.aws"},
            "requestContext": {
                "http": {
                    "method": method,
                    "path": path,
                    "protocol": "HTTP/1.1",
                    "sourceIp": "127.0.0.1",
                    "userAgent": "test"
                },
                "requestId": "request",
                "routeKey": "$default",
                "stage": "$default",
                "timeEpoch": 0
            },
            "body": body,
            "isBase64Encoded": false
        });
        let mut context = lambda_runtime::Context::default();
        context.request_id = "request".to_string();
        context.deadline = (SystemTime::now() + Duration::from_secs(30))
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let event = event_to_request(LambdaEvent::new(payload, context));
        (state, event)
    }

    #[tokio::test]
    async fn adapters_dispatch_s3_without_sqs_interception() {
        for streaming in [false, true] {
            let control = Arc::new(ControlGrpcServer::new());
            let mut tasks = control
                .wait_for_tasks(tonic::Request::new(
                    alien_worker_protocol::control::WaitForTasksRequest {
                        application_id: "storage-app".into(),
                    },
                ))
                .await
                .expect("subscribe")
                .into_inner();
            let app_control = control.clone();
            let app = tokio::spawn(async move {
                let task = tokio_stream::StreamExt::next(&mut tasks)
                    .await
                    .expect("storage delivery")
                    .expect("task");
                let Some(Payload::StorageEvent(event)) = task.payload else {
                    panic!("S3 must dispatch storage");
                };
                assert_eq!(event.bucket, "my-bucket");
                assert_eq!(event.key, "path/to/file.txt");
                assert_eq!(event.size, 1024);
                app_control.send_task_result(tonic::Request::new(alien_worker_protocol::control::SendTaskResultRequest {
                    task_id: task.task_id, result: Some(alien_worker_protocol::control::send_task_result_request::Result::Success(alien_worker_protocol::control::TaskSuccess { response_data: Vec::new() })),
                })).await.expect("complete storage task");
            });
            let state = Arc::new(LambdaState {
                queue_bindings: None,
                queue_sources: Default::default(),
                queue_batches: Default::default(),
                command_timeout: Duration::from_secs(30),
                control_server: control,
                http_client: forward_client_builder().build().expect("client"),
            });
            let mut context = lambda_runtime::Context::default();
            context.deadline = (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_millis()
                + 30_000) as u64;
            let request = event_to_request(LambdaEvent::new(s3_event_json(), context));
            assert!(request.extensions().get::<NonHttpInvocation>().is_some());
            let (sender, _receiver) = unbounded_channel();
            if streaming {
                let response = StreamingAdapter {
                    state,
                    request_done_sender: sender,
                }
                .call(request)
                .await
                .expect("S3 invocation");
                let lambda_runtime::FunctionResponse::StreamingResponse(response) = response else {
                    panic!("storage response");
                };
                assert_eq!(response.metadata_prelude.status_code.as_u16(), 200);
            } else {
                let response = BufferedAdapter {
                    state,
                    request_done_sender: sender,
                }
                .call(request)
                .await
                .expect("S3 invocation");
                assert_eq!(response["statusCode"], 200);
            }
            app.await.expect("storage application");
        }
    }

    #[tokio::test]
    async fn adapters_forward_event_shaped_http_bodies_without_dispatching_tasks() {
        let envelope = alien_commands::test_utils::test_simple_envelope("command-id", "run");
        for streaming in [false, true] {
            for body in [
                s3_event_json(),
                sqs_event_json(),
                serde_json::to_value(&envelope).expect("envelope"),
            ] {
                let expected = body.to_string();
                let observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let calls = observed.clone();
                let app = axum::Router::new().fallback(axum::routing::any(
                    move |request: axum::http::Request<axum::body::Body>| {
                        let expected = expected.clone();
                        let calls = calls.clone();
                        async move {
                            assert_eq!(request.method(), "POST");
                            assert_eq!(request.uri().path(), "/__event");
                            assert_eq!(request.uri().query(), Some("keep=query"));
                            let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                                .await
                                .expect("body");
                            assert_eq!(bytes.as_ref(), expected.as_bytes());
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            (
                                axum::http::StatusCode::CREATED,
                                [("x-application", "http-handler")],
                                "application-response",
                            )
                        }
                    },
                ));
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("listen");
                let port = listener.local_addr().expect("address").port();
                let server = tokio::spawn(async move {
                    axum::serve(listener, app).await.expect("serve HTTP app");
                });
                let (state, mut request) =
                    http_invocation_with_body("POST", port, "/__event", body.to_string()).await;
                request
                    .headers_mut()
                    .insert("x-alien-event", "aws:sqs".parse().expect("header"));
                assert!(request.extensions().get::<NonHttpInvocation>().is_none());
                let (sender, _receiver) = unbounded_channel();
                if streaming {
                    let response = StreamingAdapter {
                        state: Arc::new(state),
                        request_done_sender: sender,
                    }
                    .call(request)
                    .await
                    .expect("HTTP invocation");
                    let lambda_runtime::FunctionResponse::StreamingResponse(response) = response
                    else {
                        panic!("HTTP must stay streaming");
                    };
                    assert_eq!(response.metadata_prelude.status_code.as_u16(), 201);
                    assert_eq!(
                        response.metadata_prelude.headers["x-application"],
                        "http-handler"
                    );
                    let bytes: Vec<Bytes> =
                        futures_util::TryStreamExt::try_collect(response.stream)
                            .await
                            .expect("HTTP response body");
                    assert_eq!(bytes.concat(), b"application-response");
                } else {
                    let response = BufferedAdapter {
                        state: Arc::new(state),
                        request_done_sender: sender,
                    }
                    .call(request)
                    .await
                    .expect("HTTP invocation");
                    assert_eq!(response["statusCode"], 201);
                    assert_eq!(response["headers"]["x-application"], "http-handler");
                    assert!(response.get("batchItemFailures").is_none());
                }
                assert_eq!(observed.load(std::sync::atomic::Ordering::SeqCst), 1);
                server.abort();
                assert!(server.await.expect_err("server stopped").is_cancelled());
            }
        }
    }

    #[tokio::test]
    async fn streaming_invocations_return_application_redirects() {
        use crate::transports::shared::forwarding_tests::{
            assert_application_redirects_are_returned, Forwarded,
        };
        use futures_util::TryStreamExt;

        assert_application_redirects_are_returned(|method, app_port| async move {
            let (state, event) = http_invocation(method, app_port).await;
            let response = handle_streaming_event(&state, "request", event)
                .await
                .expect("streaming invocation succeeds");
            let StreamResponse {
                metadata_prelude,
                stream,
            } = into_stream_response(response);
            let body: Vec<Bytes> = stream.try_collect().await.expect("body streams");
            Forwarded {
                status: metadata_prelude.status_code.as_u16(),
                location: metadata_prelude
                    .headers
                    .get("location")
                    .map(|v| v.to_str().unwrap().to_string()),
                cookies: metadata_prelude.cookies,
                body: body.concat().into(),
            }
        })
        .await;
    }

    #[tokio::test]
    async fn buffered_invocations_return_application_redirects() {
        use crate::transports::shared::forwarding_tests::{
            assert_application_redirects_are_returned, Forwarded,
        };

        assert_application_redirects_are_returned(|method, app_port| async move {
            let (state, event) = http_invocation(method, app_port).await;
            let response = handle_buffered_event(&state, "request", event)
                .await
                .expect("buffered invocation succeeds");
            let body = match response.body {
                Some(LambdaBody::Binary(bytes)) => Bytes::from(bytes),
                other => panic!("unexpected buffered body: {other:?}"),
            };
            Forwarded {
                status: response.status_code as u16,
                location: response
                    .headers
                    .get("location")
                    .map(|v| v.to_str().unwrap().to_string()),
                cookies: response.cookies,
                body,
            }
        })
        .await;
    }

    #[tokio::test]
    async fn event_delivery_failure_is_returned_to_lambda() {
        let state = LambdaState {
            queue_bindings: None,
            queue_sources: Default::default(),
            queue_batches: Default::default(),
            command_timeout: Duration::from_secs(300),
            control_server: Arc::new(ControlGrpcServer::new()),
            http_client: forward_client_builder().build().unwrap(),
        };

        let error = send_event_task(&state, Task::default(), "queue message")
            .await
            .expect_err("missing task subscriber must fail the invocation");
        assert!(error.to_string().contains("(queue message) failed"));
    }
}

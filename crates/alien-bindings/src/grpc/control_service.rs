//! Control service for runtime-application communication.
//!
//! This service handles:
//! - HTTP server registration
//! - Event handler registration  
//! - Task streaming from runtime to app
//! - Task result submission

use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::{broadcast, mpsc, Mutex, Notify, RwLock};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

pub mod alien_bindings {
    pub mod control {
        tonic::include_proto!("alien_bindings.control");

        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.control_descriptor");
    }
}

use alien_bindings::control::{
    control_service_server::{ControlService, ControlServiceServer},
    RegisterEventHandlerRequest, RegisterEventHandlerResponse, RegisterHttpServerRequest,
    RegisterHttpServerResponse, SendTaskResultRequest, SendTaskResultResponse, Task,
    WaitForTasksRequest,
};

/// Handler registration info
#[derive(Debug, Clone)]
pub struct HandlerRegistration {
    pub handler_type: String,
    pub resource_name: String,
}

/// Tracks registered handlers and HTTP server port
#[derive(Debug)]
pub struct ControlState {
    /// Registered HTTP server port (if any)
    http_port: Option<u16>,
    /// Registered event handlers: (handler_type, resource_name) -> registration
    handlers: HashMap<(String, String), HandlerRegistration>,
    /// Sender for notifying when HTTP server is registered
    http_ready_tx: Option<tokio::sync::oneshot::Sender<u16>>,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            http_port: None,
            handlers: HashMap::new(),
            http_ready_tx: None,
        }
    }
}

/// Control gRPC server implementation
#[derive(Clone)]
pub struct ControlGrpcServer {
    /// Shared state
    state: Arc<RwLock<ControlState>>,
    /// Task sender - runtime sends tasks here
    task_tx: broadcast::Sender<Task>,
    /// Result channels - keyed by task_id
    result_channels: Arc<Mutex<HashMap<String, mpsc::Sender<Result<TaskResult, String>>>>>,
    /// Notified when the first task stream subscriber connects
    task_subscriber_notify: Arc<Notify>,
}

/// Result for a task
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Whether the task was processed successfully
    pub success: bool,
    /// Response data (for successful processing)
    pub response_data: Vec<u8>,
    /// Error code (for failed processing)
    pub error_code: Option<String>,
    /// Error message (for failed processing)
    pub error_message: Option<String>,
}

impl TaskResult {
    /// Create a success response
    pub fn success(data: Vec<u8>) -> Self {
        Self {
            success: true,
            response_data: data,
            error_code: None,
            error_message: None,
        }
    }

    /// Create an error response
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            success: false,
            response_data: Vec::new(),
            error_code: Some(code.into()),
            error_message: Some(message.into()),
        }
    }
}

impl ControlGrpcServer {
    pub fn new() -> Self {
        let (task_tx, _) = broadcast::channel(1024);
        Self {
            state: Arc::new(RwLock::new(ControlState::default())),
            task_tx,
            task_subscriber_notify: Arc::new(Notify::new()),
            result_channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the registered HTTP port (if any)
    pub async fn get_http_port(&self) -> Option<u16> {
        self.state.read().await.http_port
    }

    /// Check if any event handlers have been registered by the application.
    pub async fn has_registered_handlers(&self) -> bool {
        !self.state.read().await.handlers.is_empty()
    }

    /// Check if a handler is registered
    pub async fn has_handler(&self, handler_type: &str, resource_name: &str) -> bool {
        let state = self.state.read().await;
        state
            .handlers
            .contains_key(&(handler_type.to_string(), resource_name.to_string()))
    }

    /// Get all registered handlers
    pub async fn get_handlers(&self) -> Vec<HandlerRegistration> {
        let state = self.state.read().await;
        state.handlers.values().cloned().collect()
    }

    /// Wait for HTTP server to be registered
    pub async fn wait_for_http_server(&self) -> Option<u16> {
        // Check if already registered
        {
            let state = self.state.read().await;
            if let Some(port) = state.http_port {
                return Some(port);
            }
        }

        // Create a oneshot channel and store sender
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut state = self.state.write().await;
            // Double-check in case it was registered while we were waiting for write lock
            if let Some(port) = state.http_port {
                return Some(port);
            }
            state.http_ready_tx = Some(tx);
        }

        // Wait for registration
        rx.await.ok()
    }

    /// Wait for at least one application to subscribe to the task stream.
    /// Returns immediately if there's already a subscriber.
    pub async fn wait_for_task_subscriber(&self) {
        if self.task_tx.receiver_count() > 0 {
            return;
        }
        // notify_one() stores a permit when no one is waiting, so even if
        // the app subscribes between our check above and this await, the
        // stored permit makes notified() return immediately.
        self.task_subscriber_notify.notified().await;
    }

    /// Send a task to the application and wait for the result.
    /// This is used for all task types - the runtime must wait for the app to process
    /// before acknowledging to the platform (storage/cron/queue) or submitting responses (commands).
    pub async fn send_task(
        &self,
        task: Task,
        timeout: std::time::Duration,
    ) -> Result<TaskResult, String> {
        let task_id = task.task_id.clone();

        // Create result channel
        let (result_tx, mut result_rx) = mpsc::channel(1);
        {
            let mut channels = self.result_channels.lock().await;
            channels.insert(task_id.clone(), result_tx);
        }

        // Send the task
        let receiver_count = self
            .task_tx
            .send(task)
            .map_err(|e| format!("Failed to send task: {}", e))?;

        debug!(task_id = %task_id, receiver_count = receiver_count, "Task broadcast to subscribers, waiting for result");

        // Wait for result with timeout
        let result = tokio::time::timeout(timeout, result_rx.recv())
            .await
            .map_err(|_| {
                warn!(task_id = %task_id, timeout_secs = timeout.as_secs(), "Task result timeout — app never sent result");
                "Task result timeout".to_string()
            })?
            .ok_or_else(|| {
                warn!(task_id = %task_id, "Result channel closed without sending result");
                "Result channel closed".to_string()
            })?;

        debug!(task_id = %task_id, success = result.as_ref().map(|r| r.success).unwrap_or(false), "Received task result from app");

        // Clean up channel
        {
            let mut channels = self.result_channels.lock().await;
            channels.remove(&task_id);
        }

        result
    }

    /// Convert to tonic service
    pub fn into_service(self) -> ControlServiceServer<Self> {
        ControlServiceServer::new(self)
    }
}

impl Default for ControlGrpcServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl ControlService for ControlGrpcServer {
    async fn register_http_server(
        &self,
        request: Request<RegisterHttpServerRequest>,
    ) -> Result<Response<RegisterHttpServerResponse>, Status> {
        let req = request.into_inner();
        let port = req.port as u16;

        info!(port = port, "Application registered HTTP server");

        let mut state = self.state.write().await;
        state.http_port = Some(port);

        // Notify any waiters
        if let Some(tx) = state.http_ready_tx.take() {
            let _ = tx.send(port);
        }

        Ok(Response::new(RegisterHttpServerResponse { success: true }))
    }

    async fn register_event_handler(
        &self,
        request: Request<RegisterEventHandlerRequest>,
    ) -> Result<Response<RegisterEventHandlerResponse>, Status> {
        let req = request.into_inner();

        info!(
            handler_type = %req.handler_type,
            resource_name = %req.resource_name,
            "Application registered event handler"
        );

        let registration = HandlerRegistration {
            handler_type: req.handler_type.clone(),
            resource_name: req.resource_name.clone(),
        };

        let mut state = self.state.write().await;
        state
            .handlers
            .insert((req.handler_type, req.resource_name), registration);

        Ok(Response::new(RegisterEventHandlerResponse {
            success: true,
        }))
    }

    type WaitForTasksStream = Pin<Box<dyn Stream<Item = Result<Task, Status>> + Send>>;

    async fn wait_for_tasks(
        &self,
        request: Request<WaitForTasksRequest>,
    ) -> Result<Response<Self::WaitForTasksStream>, Status> {
        let req = request.into_inner();
        debug!(application_id = %req.application_id, "Application waiting for tasks");

        let mut task_rx = self.task_tx.subscribe();
        self.task_subscriber_notify.notify_one();

        let stream = async_stream::stream! {
            loop {
                match task_rx.recv().await {
                    Ok(task) => {
                        yield Ok(task);
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Task stream lagged, some tasks may have been dropped");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("Task channel closed, ending stream");
                        break;
                    }
                }
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn send_task_result(
        &self,
        request: Request<SendTaskResultRequest>,
    ) -> Result<Response<SendTaskResultResponse>, Status> {
        let req = request.into_inner();
        let task_id = req.task_id;

        let (result, result_desc) = match req.result {
            Some(alien_bindings::control::send_task_result_request::Result::Success(ref s)) => {
                let desc = format!("success, response_data_len={}", s.response_data.len());
                (Ok(TaskResult::success(s.response_data.clone())), desc)
            }
            Some(alien_bindings::control::send_task_result_request::Result::Error(ref e)) => {
                let desc = format!("error, code={}, message={}", e.code, e.message);
                (
                    Ok(TaskResult::error(e.code.clone(), e.message.clone())),
                    desc,
                )
            }
            None => (Err("No result in response".to_string()), "none".to_string()),
        };

        debug!(task_id = %task_id, result = %result_desc, "Received task result from app via gRPC");

        // Send to waiting channel if any
        let channels = self.result_channels.lock().await;
        if let Some(tx) = channels.get(&task_id) {
            if let Err(e) = tx.send(result).await {
                warn!(task_id = %task_id, "Failed to send result to waiting channel: {:?}", e);
            } else {
                debug!(task_id = %task_id, "Result forwarded to send_task caller");
            }
        } else {
            warn!(task_id = %task_id, "No waiting channel found for task result (task may have already timed out)");
        }

        Ok(Response::new(SendTaskResultResponse { acknowledged: true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_http_server() {
        let server = ControlGrpcServer::new();

        assert!(server.get_http_port().await.is_none());

        let req = Request::new(RegisterHttpServerRequest { port: 8080 });
        let resp = server.register_http_server(req).await.unwrap();

        assert!(resp.into_inner().success);
        assert_eq!(server.get_http_port().await, Some(8080));
    }

    #[tokio::test]
    async fn test_register_event_handler() {
        let server = ControlGrpcServer::new();

        assert!(!server.has_handler("storage", "uploads").await);

        let req = Request::new(RegisterEventHandlerRequest {
            handler_type: "storage".to_string(),
            resource_name: "uploads".to_string(),
        });
        let resp = server.register_event_handler(req).await.unwrap();

        assert!(resp.into_inner().success);
        assert!(server.has_handler("storage", "uploads").await);
    }

    #[tokio::test]
    async fn test_wait_for_http_server() {
        let server = ControlGrpcServer::new();
        let server_clone = server.clone();

        // Spawn a task to wait for HTTP server
        let wait_task = tokio::spawn(async move { server_clone.wait_for_http_server().await });

        // Give the wait task time to start
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Register HTTP server
        let req = Request::new(RegisterHttpServerRequest { port: 3000 });
        server.register_http_server(req).await.unwrap();

        // Wait task should complete with the port
        let port = wait_task.await.unwrap();
        assert_eq!(port, Some(3000));
    }
}

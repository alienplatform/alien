//! Control service for Worker runtime-to-application communication.
//!
//! This service handles:
//! - HTTP server registration
//! - Event handler registration  
//! - Task streaming from Worker runtime to app
//! - Task result submission

use std::{
    collections::{hash_map::Entry, HashMap},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{broadcast, oneshot, watch, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::control::{
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
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            http_port: None,
            handlers: HashMap::new(),
        }
    }
}

type TaskResultSender = oneshot::Sender<Result<TaskResult, String>>;
type PendingResults = Arc<Mutex<HashMap<String, Option<TaskResultSender>>>>;

/// Control gRPC server implementation
#[derive(Debug, Clone)]
pub struct ControlGrpcServer {
    /// Shared state
    state: Arc<RwLock<ControlState>>,
    /// Task sender - the Worker runtime sends tasks here.
    task_tx: broadcast::Sender<Task>,
    /// Result channels - keyed by task_id
    result_channels: PendingResults,
    capacity: Arc<Semaphore>,
    cancelled_tx: watch::Sender<bool>,
    stopped_tx: watch::Sender<bool>,
    supervised: Arc<AtomicBool>,
    /// Readiness signal for the application HTTP server.
    http_ready_tx: watch::Sender<Option<u16>>,
    /// Readiness signal for the application task stream.
    task_ready_tx: watch::Sender<bool>,
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
        let (http_ready_tx, _) = watch::channel(None);
        let (task_ready_tx, _) = watch::channel(false);
        let (cancelled_tx, _) = watch::channel(false);
        let (stopped_tx, _) = watch::channel(false);
        Self {
            state: Arc::new(RwLock::new(ControlState::default())),
            task_tx,
            http_ready_tx,
            task_ready_tx,
            result_channels: Arc::new(Mutex::new(HashMap::new())),
            capacity: Arc::new(Semaphore::new(1024)),
            cancelled_tx,
            stopped_tx,
            supervised: Arc::new(AtomicBool::new(false)),
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
        let mut ready = self.http_ready_tx.subscribe();
        loop {
            if let Some(port) = *ready.borrow_and_update() {
                return Some(port);
            }
            if ready.changed().await.is_err() {
                return None;
            }
        }
    }

    /// Wait for at least one application to subscribe to the task stream.
    /// Returns immediately if there's already a subscriber.
    pub async fn wait_for_task_subscriber(&self) {
        let mut ready = self.task_ready_tx.subscribe();
        loop {
            if *ready.borrow_and_update() {
                return;
            }
            if ready.changed().await.is_err() {
                return;
            }
        }
    }

    /// Send a task to the application and wait for the result.
    /// This is used for all task types - the Worker runtime must wait for the app to process
    /// before acknowledging to the platform (storage/cron/queue) or submitting responses (commands).
    pub async fn send_task(
        &self,
        task: Task,
        timeout: std::time::Duration,
    ) -> Result<TaskResult, String> {
        if *self.cancelled_tx.borrow() {
            return Err("Application is stopping after a cancelled task".to_string());
        }
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Application task capacity exhausted".to_string())?;
        let task_id = task.task_id.clone();
        let (result_tx, result_rx) = oneshot::channel();
        {
            let mut channels = self
                .result_channels
                .lock()
                .map_err(|_| "Task result registry is poisoned".to_string())?;
            match channels.entry(task_id.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(Some(result_tx));
                }
                Entry::Occupied(_) => return Err("Duplicate task delivery identity".to_string()),
            }
        }
        let mut pending = PendingTask {
            server: self.clone(),
            task_id,
            dispatched: false,
            completed: false,
            _permit: permit,
        };
        self.task_tx
            .send(task)
            .map_err(|_| "No application task subscriber".to_string())?;
        pending.dispatched = true;
        let result = tokio::time::timeout(timeout, result_rx).await;
        match result {
            Ok(Ok(result)) => {
                pending.completed = true;
                result
            }
            _ => {
                self.cancel_application().await;
                Err("Application task was cancelled before completion".to_string())
            }
        }
    }

    /// Stop the application before returning a runtime timeout decision.
    pub async fn cancel_application(&self) {
        self.cancelled_tx.send_replace(true);
        if self.supervised.load(Ordering::Acquire) {
            let mut stopped = self.stopped_tx.subscribe();
            while !*stopped.borrow_and_update() {
                if stopped.changed().await.is_err() {
                    break;
                }
            }
        }
    }

    /// Require the process host to stop the application before cancelled task
    /// delivery returns. Enable this before accepting work in a process host.
    pub fn supervise_task_cancellation(&self) {
        self.supervised.store(true, Ordering::Release);
    }

    /// Wait for a dispatched task to time out or lose its waiting caller.
    pub async fn wait_for_cancelled_task(&self) {
        let mut cancelled = self.cancelled_tx.subscribe();
        while !*cancelled.borrow_and_update() {
            if cancelled.changed().await.is_err() {
                return;
            }
        }
    }

    /// Confirm that the application process can no longer execute handlers.
    pub fn application_stopped(&self) {
        self.stopped_tx.send_replace(true);
    }

    /// Convert to tonic service
    pub fn into_service(self) -> ControlServiceServer<Self> {
        ControlServiceServer::new(self)
    }
}

/// Remove correlation state even when an HTTP request or invocation is dropped.
struct PendingTask {
    server: ControlGrpcServer,
    task_id: String,
    dispatched: bool,
    completed: bool,
    _permit: OwnedSemaphorePermit,
}

impl Drop for PendingTask {
    fn drop(&mut self) {
        // Cleanup must also work during unwinding. No application code runs
        // while this registry lock is held.
        self.server
            .result_channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.task_id);
        if self.dispatched && !self.completed {
            self.server.cancelled_tx.send_replace(true);
        }
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

        self.state.write().await.http_port = Some(port);
        self.http_ready_tx.send_replace(Some(port));

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
        self.task_ready_tx.send_replace(true);

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
            Some(crate::control::send_task_result_request::Result::Success(ref s)) => {
                let desc = format!("success, response_data_len={}", s.response_data.len());
                (Ok(TaskResult::success(s.response_data.clone())), desc)
            }
            Some(crate::control::send_task_result_request::Result::Error(ref e)) => {
                let desc = format!("error, code={}, message={}", e.code, e.message);
                (
                    Ok(TaskResult::error(e.code.clone(), e.message.clone())),
                    desc,
                )
            }
            None => (Err("No result in response".to_string()), "none".to_string()),
        };

        debug!(task_id = %task_id, result = %result_desc, "Received task result from app via gRPC");

        let sender = self
            .result_channels
            .lock()
            .map_err(|_| Status::internal("Task result registry is poisoned"))?
            .get_mut(&task_id)
            .and_then(Option::take);
        let acknowledged = sender.is_some_and(|sender| sender.send(result).is_ok());
        Ok(Response::new(SendTaskResultResponse { acknowledged }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{send_task_result_request, TaskSuccess};
    use std::time::Duration;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn cancellation_waits_for_process_stop_and_rejects_late_results() {
        let server = ControlGrpcServer::new();
        server.supervise_task_cancellation();
        let mut stream = server
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test-app".to_string(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let waiting = tokio::spawn({
            let server = server.clone();
            async move {
                server
                    .send_task(
                        Task {
                            task_id: "delivery".to_string(),
                            payload: None,
                        },
                        Duration::ZERO,
                    )
                    .await
            }
        });
        assert_eq!(
            stream
                .next()
                .await
                .expect("task")
                .expect("delivery")
                .task_id,
            "delivery"
        );
        server.wait_for_cancelled_task().await;
        assert!(
            !waiting.is_finished(),
            "retry must wait until the app cannot mutate"
        );
        server.application_stopped();
        assert!(waiting.await.expect("join").is_err());
        assert!(server.result_channels.lock().expect("registry").is_empty());
        assert_eq!(server.capacity.available_permits(), 1024);
        let response = server
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: "delivery".to_string(),
                result: Some(send_task_result_request::Result::Success(TaskSuccess {
                    response_data: vec![],
                })),
            }))
            .await
            .expect("late result");
        assert!(!response.into_inner().acknowledged);
    }

    #[tokio::test]
    async fn dropped_waiter_releases_capacity_and_requests_process_stop() {
        let server = ControlGrpcServer::new();
        let mut stream = server
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test-app".to_string(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let waiting = tokio::spawn({
            let server = server.clone();
            async move {
                server
                    .send_task(
                        Task {
                            task_id: "delivery".to_string(),
                            payload: None,
                        },
                        Duration::from_secs(3600),
                    )
                    .await
            }
        });
        stream.next().await.expect("task").expect("delivery");
        let duplicate = server
            .send_task(
                Task {
                    task_id: "delivery".to_string(),
                    payload: None,
                },
                Duration::ZERO,
            )
            .await
            .expect_err("duplicate cannot replace a pending delivery");
        assert_eq!(duplicate, "Duplicate task delivery identity");
        assert_eq!(server.capacity.available_permits(), 1023);
        waiting.abort();
        assert!(waiting.await.expect_err("cancelled waiter").is_cancelled());
        server.wait_for_cancelled_task().await;
        assert!(server.result_channels.lock().expect("registry").is_empty());
        assert_eq!(server.capacity.available_permits(), 1024);
    }

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

    #[tokio::test]
    async fn multiple_http_waiters_observe_registration() {
        let server = ControlGrpcServer::new();
        let first = tokio::spawn({
            let server = server.clone();
            async move { server.wait_for_http_server().await }
        });
        let second = tokio::spawn({
            let server = server.clone();
            async move { server.wait_for_http_server().await }
        });

        server
            .register_http_server(Request::new(RegisterHttpServerRequest { port: 4000 }))
            .await
            .unwrap();

        assert_eq!(first.await.unwrap(), Some(4000));
        assert_eq!(second.await.unwrap(), Some(4000));
        assert_eq!(server.wait_for_http_server().await, Some(4000));
    }

    #[tokio::test]
    async fn multiple_task_waiters_observe_subscription() {
        let server = ControlGrpcServer::new();
        let first = tokio::spawn({
            let server = server.clone();
            async move { server.wait_for_task_subscriber().await }
        });
        let second = tokio::spawn({
            let server = server.clone();
            async move { server.wait_for_task_subscriber().await }
        });

        server
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "app".to_string(),
            }))
            .await
            .unwrap();

        first.await.unwrap();
        second.await.unwrap();
        server.wait_for_task_subscriber().await;
    }
}

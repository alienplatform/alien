//! AlienContext - Main SDK entry point for Alien applications.
//!
//! Provides access to:
//! - Resource bindings (storage, kv, queue, vault, etc.)
//! - Event handlers (storage events, cron events, queue messages, commands)
//! - Background tasks (wait_until)
//! - HTTP server registration

use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, RwLock};
#[cfg(feature = "grpc")]
use tokio_stream::StreamExt as _;
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};
use crate::wait_until::WaitUntilContext;
use crate::{BindingsMode, BindingsProvider, BindingsProviderApi, WaitUntil};
use alien_error::{AlienError, Context, IntoAlienError};

#[cfg(feature = "grpc")]
use crate::grpc::control_service::alien_bindings::control::{
    control_service_client::ControlServiceClient, send_task_result_request::Result as TaskResult,
    task::Payload as TaskPayload, RegisterEventHandlerRequest, RegisterHttpServerRequest,
    SendTaskResultRequest, Task, TaskError, TaskSuccess, WaitForTasksRequest,
};

/// Storage event delivered to handlers
#[derive(Debug, Clone)]
pub struct StorageEvent {
    pub key: String,
    pub event_type: String,
    pub bucket: String,
    pub size: u64,
    pub content_type: String,
}

/// Cron event delivered to handlers  
#[derive(Debug, Clone)]
pub struct CronEvent {
    pub schedule_name: String,
    pub scheduled_time: chrono::DateTime<chrono::Utc>,
}

/// Queue message delivered to handlers
#[derive(Debug, Clone)]
pub struct QueueMessage {
    pub id: String,
    pub payload: Vec<u8>,
    pub receipt_handle: String,
    pub source: String,
    pub attempt_count: u32,
}

/// Type alias for event handler functions
type StorageHandler =
    Box<dyn Fn(StorageEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type CronHandler =
    Box<dyn Fn(CronEvent) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type QueueHandler =
    Box<dyn Fn(QueueMessage) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;
type CommandHandler =
    Box<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>> + Send + Sync>;

/// Registered handlers
struct Handlers {
    storage: HashMap<String, StorageHandler>,
    cron: HashMap<String, CronHandler>,
    queue: HashMap<String, QueueHandler>,
    command: HashMap<String, CommandHandler>,
}

impl Default for Handlers {
    fn default() -> Self {
        Self {
            storage: HashMap::new(),
            cron: HashMap::new(),
            queue: HashMap::new(),
            command: HashMap::new(),
        }
    }
}

/// Main context for Alien applications that provides access to:
/// - Resource bindings (storage, kv, queue, vault, etc.)
/// - Event handlers (storage events, cron events, queue messages, commands)
/// - Background tasks (wait_until)
/// - HTTP server registration
pub struct AlienContext {
    /// The wait_until context for managing background tasks
    wait_until_context: Arc<WaitUntilContext>,
    /// The bindings provider for accessing resources
    bindings_provider: Arc<dyn BindingsProviderApi>,
    /// Application ID
    app_id: String,
    /// Environment variables
    env_vars: HashMap<String, String>,
    /// Registered event handlers
    handlers: Arc<RwLock<Handlers>>,
    /// gRPC control client (lazy initialized)
    #[cfg(feature = "grpc")]
    control_client: Arc<Mutex<Option<ControlServiceClient<tonic::transport::Channel>>>>,
}

impl AlienContext {
    /// Creates a new AlienContext from environment variables.
    /// This automatically sets up gRPC communication and starts the drain listener.
    pub async fn from_env() -> Result<Self> {
        Self::from_env_with_vars(&std::env::vars().collect()).await
    }

    /// Creates a new AlienContext from provided environment variables.
    /// This is useful for testing or when environment variables are not available via std::env.
    pub async fn from_env_with_vars(env_vars: &HashMap<String, String>) -> Result<Self> {
        // Choose the appropriate bindings provider based on ALIEN_BINDINGS_MODE
        let bindings_mode = crate::get_bindings_mode_from_env(env_vars)?;

        let bindings_provider: Arc<dyn BindingsProviderApi> = match bindings_mode {
            BindingsMode::Grpc => {
                #[cfg(feature = "grpc")]
                {
                    use crate::providers::grpc_provider::GrpcBindingsProvider;
                    Arc::new(GrpcBindingsProvider::new_with_env(env_vars.clone())?)
                }
                #[cfg(not(feature = "grpc"))]
                {
                    return Err(AlienError::new(ErrorData::FeatureNotEnabled {
                        feature: "grpc".to_string(),
                    }));
                }
            }
            BindingsMode::Direct => Arc::new(BindingsProvider::from_env(env_vars.clone()).await?),
        };

        let app_id = uuid::Uuid::new_v4().to_string();

        let wait_until_context =
            Arc::new(WaitUntilContext::from_env_with_vars(Some(app_id.clone()), env_vars).await?);

        // Start the drain listener automatically
        wait_until_context.start_drain_listener().await?;

        Ok(Self {
            wait_until_context,
            bindings_provider,
            app_id,
            env_vars: env_vars.clone(),
            handlers: Arc::new(RwLock::new(Handlers::default())),
            #[cfg(feature = "grpc")]
            control_client: Arc::new(Mutex::new(None)),
        })
    }

    /// Creates a new AlienContext with custom provider and wait_until context.
    /// This is mainly useful for testing or advanced use cases.
    pub fn new(
        wait_until_context: Arc<WaitUntilContext>,
        bindings_provider: Arc<dyn BindingsProviderApi>,
    ) -> Self {
        Self {
            app_id: wait_until_context.application_id().to_string(),
            wait_until_context,
            bindings_provider,
            env_vars: std::env::vars().collect(),
            handlers: Arc::new(RwLock::new(Handlers::default())),
            #[cfg(feature = "grpc")]
            control_client: Arc::new(Mutex::new(None)),
        }
    }

    /// Gets the gRPC control client, creating it if needed
    #[cfg(feature = "grpc")]
    async fn get_control_client(&self) -> Result<ControlServiceClient<tonic::transport::Channel>> {
        let mut client_guard = self.control_client.lock().await;

        if let Some(client) = client_guard.as_ref() {
            return Ok(client.clone());
        }

        let grpc_address = self
            .env_vars
            .get("ALIEN_BINDINGS_GRPC_ADDRESS")
            .ok_or_else(|| {
                AlienError::new(ErrorData::EnvironmentVariableMissing {
                    variable_name: "ALIEN_BINDINGS_GRPC_ADDRESS".to_string(),
                })
            })?;

        let endpoint = format!("http://{}", grpc_address);
        let channel = tonic::transport::Channel::from_shared(endpoint.clone())
            .into_alien_error()
            .context(ErrorData::GrpcConnectionFailed {
                endpoint: endpoint.clone(),
                reason: "Invalid gRPC endpoint format".to_string(),
            })?
            .connect()
            .await
            .into_alien_error()
            .context(ErrorData::GrpcConnectionFailed {
                endpoint,
                reason: "Failed to connect to gRPC server".to_string(),
            })?;

        let client = ControlServiceClient::new(channel);
        *client_guard = Some(client.clone());
        Ok(client)
    }

    // ==================== BINDINGS ====================

    /// Gets the bindings provider for accessing storage, build, and other resources.
    pub fn bindings(&self) -> &dyn BindingsProviderApi {
        self.bindings_provider.as_ref()
    }

    /// Gets the bindings provider as an Arc
    pub fn get_bindings(&self) -> Arc<dyn BindingsProviderApi> {
        Arc::clone(&self.bindings_provider)
    }

    // ==================== EVENT HANDLERS ====================

    /// Registers a handler for storage events on the specified bucket/resource.
    ///
    /// # Example
    /// ```ignore
    /// ctx.on_storage_event("uploads", |event| async move {
    ///     println!("File {} was {}", event.key, event.event_type);
    ///     Ok(())
    /// });
    /// ```
    pub fn on_storage_event<F, Fut>(&self, resource: &str, handler: F)
    where
        F: Fn(StorageEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let resource = resource.to_string();
        let handler = Box::new(move |event: StorageEvent| {
            let fut = handler(event);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<()>> + Send>>
        });

        let handlers = self.handlers.clone();
        let resource_clone = resource.clone();

        // Register in background to avoid blocking
        tokio::spawn(async move {
            let mut h = handlers.write().await;
            h.storage.insert(resource_clone, handler);
        });

        info!(resource = %resource, "Registered storage event handler");
    }

    /// Registers a handler for cron/scheduled events.
    ///
    /// # Example
    /// ```ignore
    /// ctx.on_cron_event("daily-cleanup", |event| async move {
    ///     cleanup_old_files().await;
    ///     Ok(())
    /// });
    /// ```
    pub fn on_cron_event<F, Fut>(&self, schedule: &str, handler: F)
    where
        F: Fn(CronEvent) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let schedule = schedule.to_string();
        let handler = Box::new(move |event: CronEvent| {
            let fut = handler(event);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<()>> + Send>>
        });

        let handlers = self.handlers.clone();
        let schedule_clone = schedule.clone();

        tokio::spawn(async move {
            let mut h = handlers.write().await;
            h.cron.insert(schedule_clone, handler);
        });

        info!(schedule = %schedule, "Registered cron event handler");
    }

    /// Registers a handler for queue messages.
    ///
    /// # Example
    /// ```ignore
    /// ctx.on_queue_message("tasks", |message| async move {
    ///     process_task(&message.payload).await;
    ///     Ok(())
    /// });
    /// ```
    pub fn on_queue_message<F, Fut>(&self, queue: &str, handler: F)
    where
        F: Fn(QueueMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let queue = queue.to_string();
        let handler = Box::new(move |message: QueueMessage| {
            let fut = handler(message);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<()>> + Send>>
        });

        let handlers = self.handlers.clone();
        let queue_clone = queue.clone();

        tokio::spawn(async move {
            let mut h = handlers.write().await;
            h.queue.insert(queue_clone, handler);
        });

        info!(queue = %queue, "Registered queue message handler");
    }

    /// Registers a command handler for ARC remote calls.
    ///
    /// # Example
    /// ```ignore
    /// ctx.on_command::<GenerateReportParams, ReportResult>("generate-report", |params| async move {
    ///     let report = generate_report(params.start_date, params.end_date).await?;
    ///     Ok(report)
    /// });
    /// ```
    pub fn on_command<P, R, F, Fut>(&self, command: &str, handler: F)
    where
        P: DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        F: Fn(P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
    {
        let command = command.to_string();
        let handler = Box::new(move |params_bytes: Vec<u8>| {
            // Deserialize params
            let params: P = match serde_json::from_slice(&params_bytes) {
                Ok(p) => p,
                Err(e) => {
                    return Box::pin(async move {
                        Err(AlienError::new(ErrorData::DeserializationFailed {
                            message: format!("Failed to deserialize command params: {}", e),
                            type_name: std::any::type_name::<P>().to_string(),
                        }))
                    })
                        as Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;
                }
            };

            let fut = handler(params);
            Box::pin(async move {
                let result = fut.await?;
                serde_json::to_vec(&result).into_alien_error().context(
                    ErrorData::SerializationFailed {
                        message: "Failed to serialize command result".to_string(),
                    },
                )
            }) as Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>
        });

        let handlers = self.handlers.clone();
        let command_clone = command.clone();

        tokio::spawn(async move {
            let mut h = handlers.write().await;
            h.command.insert(command_clone, handler);
        });

        info!(command = %command, "Registered command handler");
    }

    // ==================== HTTP SERVER ====================

    /// Registers the application's HTTP server port with the runtime.
    /// The runtime will forward HTTP requests to this port.
    ///
    /// # Example
    /// ```ignore
    /// let listener = TcpListener::bind("127.0.0.1:0").await?;
    /// let port = listener.local_addr()?.port();
    /// ctx.register_http_server(port).await?;
    /// ```
    #[cfg(feature = "grpc")]
    pub async fn register_http_server(&self, port: u16) -> Result<()> {
        info!(port = port, "Registering HTTP server with runtime");

        let mut client = self.get_control_client().await?;

        let request = tonic::Request::new(RegisterHttpServerRequest { port: port as u32 });

        client
            .register_http_server(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcCallFailed {
                service: "ControlService".to_string(),
                method: "RegisterHttpServer".to_string(),
                reason: "gRPC call failed".to_string(),
            })?;

        info!(port = port, "HTTP server registered successfully");
        Ok(())
    }

    #[cfg(not(feature = "grpc"))]
    pub async fn register_http_server(&self, _port: u16) -> Result<()> {
        Err(AlienError::new(ErrorData::FeatureNotEnabled {
            feature: "grpc".to_string(),
        }))
    }

    // ==================== EVENT LOOP ====================

    /// Enters the main event loop and processes events from the runtime.
    /// This blocks until shutdown is signaled.
    ///
    /// Call this after registering all event handlers.
    ///
    /// # Example
    /// ```ignore
    /// ctx.on_storage_event("uploads", handler);
    /// ctx.on_command("process", cmd_handler);
    /// ctx.run().await?;
    /// ```
    #[cfg(feature = "grpc")]
    pub async fn run(&self) -> Result<()> {
        info!(app_id = %self.app_id, "Entering event loop");

        // Register handlers with runtime
        self.register_handlers_with_runtime().await?;

        // Get control client and start task stream
        let mut client = self.get_control_client().await?;

        let request = tonic::Request::new(WaitForTasksRequest {
            application_id: self.app_id.clone(),
        });

        let mut stream = client
            .wait_for_tasks(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcCallFailed {
                service: "ControlService".to_string(),
                method: "WaitForTasks".to_string(),
                reason: "Failed to start task stream".to_string(),
            })?
            .into_inner();

        info!("Task stream established, waiting for tasks");

        // Process tasks until stream ends
        while let Some(task_result) = stream.next().await {
            match task_result {
                Ok(task) => {
                    if let Err(e) = self.handle_task(task).await {
                        error!(error = %e, "Error handling task");
                    }
                }
                Err(status) => {
                    if status.code() == tonic::Code::Cancelled {
                        info!("Task stream cancelled, shutting down");
                        break;
                    }
                    error!(error = %status, "Error receiving task from stream");
                }
            }
        }

        info!("Task loop ended");
        Ok(())
    }

    #[cfg(not(feature = "grpc"))]
    pub async fn run(&self) -> Result<()> {
        Err(AlienError::new(ErrorData::FeatureNotEnabled {
            feature: "grpc".to_string(),
        }))
    }

    /// Register all handlers with the runtime
    #[cfg(feature = "grpc")]
    async fn register_handlers_with_runtime(&self) -> Result<()> {
        let handlers = self.handlers.read().await;
        let mut client = self.get_control_client().await?;

        // Register storage handlers
        for resource in handlers.storage.keys() {
            let request = tonic::Request::new(RegisterEventHandlerRequest {
                handler_type: "storage".to_string(),
                resource_name: resource.clone(),
            });
            client
                .register_event_handler(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcCallFailed {
                    service: "ControlService".to_string(),
                    method: "RegisterEventHandler".to_string(),
                    reason: "Failed to register storage handler".to_string(),
                })?;
            debug!(handler_type = "storage", resource = %resource, "Registered handler with runtime");
        }

        // Register cron handlers
        for schedule in handlers.cron.keys() {
            let request = tonic::Request::new(RegisterEventHandlerRequest {
                handler_type: "cron".to_string(),
                resource_name: schedule.clone(),
            });
            client
                .register_event_handler(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcCallFailed {
                    service: "ControlService".to_string(),
                    method: "RegisterEventHandler".to_string(),
                    reason: "Failed to register cron handler".to_string(),
                })?;
            debug!(handler_type = "cron", resource = %schedule, "Registered handler with runtime");
        }

        // Register queue handlers
        for queue in handlers.queue.keys() {
            let request = tonic::Request::new(RegisterEventHandlerRequest {
                handler_type: "queue".to_string(),
                resource_name: queue.clone(),
            });
            client
                .register_event_handler(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcCallFailed {
                    service: "ControlService".to_string(),
                    method: "RegisterEventHandler".to_string(),
                    reason: "Failed to register queue handler".to_string(),
                })?;
            debug!(handler_type = "queue", resource = %queue, "Registered handler with runtime");
        }

        // Register command handlers
        for command in handlers.command.keys() {
            let request = tonic::Request::new(RegisterEventHandlerRequest {
                handler_type: "command".to_string(),
                resource_name: command.clone(),
            });
            client
                .register_event_handler(request)
                .await
                .into_alien_error()
                .context(ErrorData::GrpcCallFailed {
                    service: "ControlService".to_string(),
                    method: "RegisterEventHandler".to_string(),
                    reason: "Failed to register command handler".to_string(),
                })?;
            debug!(handler_type = "command", resource = %command, "Registered handler with runtime");
        }

        Ok(())
    }

    /// Handle a single task from the runtime
    #[cfg(feature = "grpc")]
    async fn handle_task(&self, task: Task) -> Result<()> {
        let task_id = task.task_id.clone();
        debug!(task_id = %task_id, "Handling task");

        // Check if it's a command before consuming the payload
        let is_command = matches!(&task.payload, Some(TaskPayload::ArcCommand(_)));

        let result = match task.payload {
            Some(TaskPayload::StorageEvent(se)) => {
                self.handle_storage_event(
                    &se.bucket,
                    StorageEvent {
                        key: se.key,
                        event_type: se.event_type,
                        bucket: se.bucket.clone(),
                        size: se.size,
                        content_type: se.content_type,
                    },
                )
                .await
            }
            Some(TaskPayload::CronEvent(ce)) => {
                let scheduled_time = ce
                    .scheduled_time
                    .map(|ts| {
                        chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                            .unwrap_or_else(chrono::Utc::now)
                    })
                    .unwrap_or_else(chrono::Utc::now);

                self.handle_cron_event(
                    &ce.schedule_name,
                    CronEvent {
                        schedule_name: ce.schedule_name.clone(),
                        scheduled_time,
                    },
                )
                .await
            }
            Some(TaskPayload::QueueMessage(qm)) => {
                self.handle_queue_message(
                    &qm.source,
                    QueueMessage {
                        id: qm.id,
                        payload: qm.payload,
                        receipt_handle: qm.receipt_handle,
                        source: qm.source.clone(),
                        attempt_count: qm.attempt_count,
                    },
                )
                .await
            }
            Some(TaskPayload::ArcCommand(cmd)) => {
                self.handle_command(&task_id, &cmd.command_name, cmd.params, &cmd.response_url)
                    .await
            }
            None => {
                warn!(task_id = %task_id, "Received task with no payload");
                Ok(())
            }
        };

        // For non-command tasks, send result to runtime
        if !is_command {
            self.send_task_result(&task_id, result).await?;
        }

        Ok(())
    }

    async fn handle_storage_event(&self, bucket: &str, event: StorageEvent) -> Result<()> {
        let handlers = self.handlers.read().await;
        // Try exact match first, then wildcard
        if let Some(handler) = handlers
            .storage
            .get(bucket)
            .or_else(|| handlers.storage.get("*"))
        {
            handler(event).await
        } else {
            warn!(bucket = %bucket, "No handler registered for storage event");
            Ok(())
        }
    }

    async fn handle_cron_event(&self, schedule: &str, event: CronEvent) -> Result<()> {
        let handlers = self.handlers.read().await;
        // Try exact match first, then wildcard
        if let Some(handler) = handlers
            .cron
            .get(schedule)
            .or_else(|| handlers.cron.get("*"))
        {
            handler(event).await
        } else {
            warn!(schedule = %schedule, "No handler registered for cron event");
            Ok(())
        }
    }

    async fn handle_queue_message(&self, queue: &str, message: QueueMessage) -> Result<()> {
        let handlers = self.handlers.read().await;
        // Try exact match first, then wildcard
        if let Some(handler) = handlers
            .queue
            .get(queue)
            .or_else(|| handlers.queue.get("*"))
        {
            handler(message).await
        } else {
            warn!(queue = %queue, "No handler registered for queue message");
            Ok(())
        }
    }

    #[cfg(feature = "grpc")]
    async fn handle_command(
        &self,
        event_id: &str,
        command: &str,
        params: Vec<u8>,
        _response_url: &str,
    ) -> Result<()> {
        let handlers = self.handlers.read().await;

        let result = if let Some(handler) = handlers.command.get(command) {
            match handler(params).await {
                Ok(response_data) => {
                    // Send success response
                    self.send_command_response(event_id, Ok(response_data))
                        .await
                }
                Err(e) => {
                    // Send error response
                    self.send_command_response(event_id, Err(e.to_string()))
                        .await
                }
            }
        } else {
            warn!(command = %command, "No handler registered for command");
            self.send_command_response(
                event_id,
                Err(format!("No handler for command: {}", command)),
            )
            .await
        };

        result
    }

    #[cfg(feature = "grpc")]
    async fn send_task_result(&self, task_id: &str, result: Result<()>) -> Result<()> {
        let mut client = self.get_control_client().await?;

        let task_result = match result {
            Ok(()) => TaskResult::Success(TaskSuccess {
                response_data: vec![],
            }),
            Err(e) => TaskResult::Error(TaskError {
                code: "HANDLER_ERROR".to_string(),
                message: e.to_string(),
            }),
        };

        let request = tonic::Request::new(SendTaskResultRequest {
            task_id: task_id.to_string(),
            result: Some(task_result),
        });

        client
            .send_task_result(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcCallFailed {
                service: "ControlService".to_string(),
                method: "SendTaskResult".to_string(),
                reason: "Failed to send task result".to_string(),
            })?;

        Ok(())
    }

    #[cfg(feature = "grpc")]
    async fn send_command_response(
        &self,
        task_id: &str,
        result: std::result::Result<Vec<u8>, String>,
    ) -> Result<()> {
        let mut client = self.get_control_client().await?;

        let task_result = match result {
            Ok(data) => TaskResult::Success(TaskSuccess {
                response_data: data,
            }),
            Err(e) => TaskResult::Error(TaskError {
                code: "COMMAND_ERROR".to_string(),
                message: e,
            }),
        };

        let request = tonic::Request::new(SendTaskResultRequest {
            task_id: task_id.to_string(),
            result: Some(task_result),
        });

        client
            .send_task_result(request)
            .await
            .into_alien_error()
            .context(ErrorData::GrpcCallFailed {
                service: "ControlService".to_string(),
                method: "SendTaskResult".to_string(),
                reason: "Failed to send command response".to_string(),
            })?;

        eprintln!("[ALIEN_CONTEXT] send_task_result succeeded");
        Ok(())
    }

    // ==================== WAIT UNTIL ====================

    /// Registers a background task that will run even after the main handler returns.
    /// The task runs in the application process and is tracked by the runtime for proper shutdown coordination.
    pub fn wait_until<F, Fut>(&self, task_fn: F) -> Result<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.wait_until_context.wait_until(task_fn)
    }

    // ==================== UTILITIES ====================

    /// Gets the application ID for this context.
    pub fn application_id(&self) -> &str {
        &self.app_id
    }

    /// Gets the current number of registered wait_until tasks.
    pub async fn get_task_count(&self) -> Result<u32> {
        self.wait_until_context.get_task_count().await
    }

    /// Gets the current function binding if available.
    pub async fn get_current_function(&self) -> Result<Option<Arc<dyn crate::traits::Function>>> {
        if let Some(current_function_name) =
            self.env_vars.get("ALIEN_CURRENT_FUNCTION_BINDING_NAME")
        {
            Ok(Some(
                self.bindings_provider
                    .load_function(current_function_name)
                    .await?,
            ))
        } else {
            Ok(None)
        }
    }

    /// Gets the current container binding if available.
    ///
    /// This allows a container to discover its own public and internal URLs.
    /// Useful for constructing callback URLs, OAuth redirects, etc.
    ///
    /// # Example
    /// ```ignore
    /// let ctx = AlienContext::from_env().await?;
    /// if let Some(container) = ctx.get_current_container().await? {
    ///     let public_url = container.get_public_url();
    ///     let callback_url = format!("{}/callback", public_url.unwrap_or(""));
    /// }
    /// ```
    pub async fn get_current_container(&self) -> Result<Option<Arc<dyn crate::traits::Container>>> {
        if let Some(current_container_name) =
            self.env_vars.get("ALIEN_CURRENT_CONTAINER_BINDING_NAME")
        {
            Ok(Some(
                self.bindings_provider
                    .load_container(current_container_name)
                    .await?,
            ))
        } else {
            Ok(None)
        }
    }
}

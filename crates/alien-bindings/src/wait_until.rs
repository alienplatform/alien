use crate::{
    error::{ErrorData, Result},
    traits::Binding,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, Mutex},
    task::JoinHandle,
    time::timeout,
};
#[cfg(feature = "grpc")]
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[cfg(feature = "grpc")]
use crate::grpc::wait_until_service::alien_bindings::wait_until::{
    wait_until_service_client::WaitUntilServiceClient, GetTaskCountRequest,
    NotifyDrainCompleteRequest, NotifyTaskRegisteredRequest, WaitForDrainSignalRequest,
};

/// Response from drain operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DrainResponse {
    /// Number of tasks that were drained.
    pub tasks_drained: u32,
    /// Whether all tasks completed successfully.
    pub success: bool,
    /// Optional error message if draining failed.
    pub error_message: Option<String>,
}

/// Configuration for wait_until drain behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct DrainConfig {
    /// Maximum time to wait for all tasks to complete.
    pub timeout: Duration,
    /// Reason for the drain request.
    pub reason: String,
}

/// A trait for wait_until bindings that provide task coordination capabilities.
/// Note: This trait is not object-safe due to generic methods, so we use concrete types in providers.
#[async_trait]
pub trait WaitUntil: Binding {
    /// Waits for a drain signal from the runtime.
    /// This is a blocking call that returns when the runtime decides it's time to drain.
    async fn wait_for_drain_signal(&self, timeout: Option<Duration>) -> Result<DrainConfig>;

    /// Drains all currently registered tasks.
    /// This waits for all tasks to complete or timeout.
    async fn drain_all(&self, config: DrainConfig) -> Result<DrainResponse>;

    /// Gets the current number of registered tasks.
    async fn get_task_count(&self) -> Result<u32>;

    /// Notifies the runtime that draining is complete.
    async fn notify_drain_complete(&self, response: DrainResponse) -> Result<()>;
}

/// A context for managing wait_until tasks within an application.
/// This handles local task execution and coordinates with the runtime via gRPC.
#[derive(Debug)]
pub struct WaitUntilContext {
    /// Unique identifier for this application instance.
    application_id: String,
    /// Currently running tasks.
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    /// Task counter for generating unique task IDs.
    task_counter: AtomicU32,
    /// gRPC client for communicating with the runtime.
    #[cfg(feature = "grpc")]
    grpc_client: Option<WaitUntilServiceClient<Channel>>,
    /// Whether we're currently draining tasks.
    draining: Arc<Mutex<bool>>,
}

impl WaitUntilContext {
    /// Creates a new WaitUntilContext.
    pub fn new(application_id: Option<String>) -> Self {
        let app_id = application_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        Self {
            application_id: app_id,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            task_counter: AtomicU32::new(0),
            #[cfg(feature = "grpc")]
            grpc_client: None,
            draining: Arc::new(Mutex::new(false)),
        }
    }

    /// Creates a new WaitUntilContext and connects to gRPC endpoint from environment variables.
    /// This is the recommended way to create a WaitUntilContext in production.
    pub async fn from_env(application_id: Option<String>) -> Result<Self> {
        let env_vars: std::collections::HashMap<String, String> = std::env::vars().collect();
        Self::from_env_with_vars(application_id, &env_vars).await
    }

    /// Creates a new WaitUntilContext and connects to gRPC endpoint from provided environment variables.
    pub async fn from_env_with_vars(
        application_id: Option<String>,
        env_vars: &std::collections::HashMap<String, String>,
    ) -> Result<Self> {
        let app_id = application_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        #[cfg(feature = "grpc")]
        {
            let bindings_mode = crate::get_bindings_mode_from_env(env_vars)?;

            match bindings_mode {
                crate::BindingsMode::Direct => {
                    // No gRPC needed - run in-process
                    return Ok(Self::new(Some(app_id)));
                }
                crate::BindingsMode::Grpc => {
                    // Require gRPC connection
                    let grpc_address =
                        env_vars.get("ALIEN_BINDINGS_GRPC_ADDRESS").ok_or_else(|| {
                            AlienError::new(ErrorData::EnvironmentVariableMissing {
                                variable_name: "ALIEN_BINDINGS_GRPC_ADDRESS".to_string(),
                            })
                        })?;

                    // Create gRPC client
                    let channel = Self::create_grpc_channel(grpc_address.clone()).await?;
                    let grpc_client = WaitUntilServiceClient::new(channel);

                    return Ok(Self {
                        application_id: app_id,
                        tasks: Arc::new(Mutex::new(HashMap::new())),
                        task_counter: AtomicU32::new(0),
                        grpc_client: Some(grpc_client),
                        draining: Arc::new(Mutex::new(false)),
                    });
                }
            }
        }

        #[cfg(not(feature = "grpc"))]
        {
            Ok(Self::new(Some(app_id)))
        }
    }

    /// Creates a gRPC channel from an address string.
    /// This creates a dedicated channel for wait_until with proper timeout and keep-alive configuration.
    #[cfg(feature = "grpc")]
    async fn create_grpc_channel(grpc_address: String) -> Result<Channel> {
        use std::time::Duration;

        // Ensure the address has a scheme, default to http if not present
        let endpoint_uri = if grpc_address.contains("://") {
            grpc_address.clone()
        } else {
            format!("http://{}", grpc_address)
        };

        let endpoint = Channel::from_shared(endpoint_uri.clone())
            .into_alien_error()
            .context(ErrorData::GrpcConnectionFailed {
                endpoint: endpoint_uri.clone(),
                reason: "Invalid gRPC endpoint URI format".to_string(),
            })?
            .timeout(Duration::from_secs(300)) // 5 min timeout for long-lived drain signal RPC
            .connect_timeout(Duration::from_secs(5)) // Connection establishment timeout
            .http2_keep_alive_interval(Duration::from_secs(30)) // Keep connection alive
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true); // Keep alive even when idle (important for drain listener)

        let channel = endpoint.connect().await.into_alien_error().context(
            ErrorData::GrpcConnectionFailed {
                endpoint: grpc_address.clone(),
                reason: "Failed to establish gRPC connection".to_string(),
            },
        )?;

        Ok(channel)
    }

    /// Creates a new WaitUntilContext with a gRPC client.
    #[cfg(feature = "grpc")]
    pub fn new_with_grpc_client(
        application_id: Option<String>,
        grpc_client: WaitUntilServiceClient<Channel>,
    ) -> Self {
        let app_id = application_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        Self {
            application_id: app_id,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            task_counter: AtomicU32::new(0),
            grpc_client: Some(grpc_client),
            draining: Arc::new(Mutex::new(false)),
        }
    }

    /// Sets the gRPC client for communicating with the runtime.
    #[cfg(feature = "grpc")]
    pub fn set_grpc_client(&mut self, client: WaitUntilServiceClient<Channel>) {
        self.grpc_client = Some(client);
    }

    /// Gets the application ID.
    pub fn application_id(&self) -> &str {
        &self.application_id
    }

    /// Starts a background task that waits for drain signals from the runtime.
    /// This should be called once when the application starts.
    pub async fn start_drain_listener(&self) -> Result<()> {
        #[cfg(feature = "grpc")]
        {
            if let Some(mut client) = self.grpc_client.clone() {
                let app_id = self.application_id.clone();
                let context = self.clone_for_background();

                tokio::spawn(async move {
                    loop {
                        debug!(app_id = %app_id, "Waiting for drain signal from runtime");

                        let request = WaitForDrainSignalRequest {
                            application_id: app_id.clone(),
                            timeout: Some(prost_types::Duration {
                                seconds: 300, // 5 minute timeout
                                nanos: 0,
                            }),
                        };

                        match client.wait_for_drain_signal(request).await {
                            Ok(response) => {
                                let resp = response.into_inner();
                                if resp.should_drain {
                                    info!(
                                        app_id = %app_id,
                                        reason = %resp.drain_reason,
                                        "Received drain signal from runtime"
                                    );

                                    let drain_timeout = resp
                                        .drain_timeout
                                        .map(|d| Duration::from_secs(d.seconds as u64))
                                        .unwrap_or(Duration::from_secs(10));

                                    let config = DrainConfig {
                                        timeout: drain_timeout,
                                        reason: resp.drain_reason,
                                    };

                                    // Drain all tasks
                                    match context.drain_all(config).await {
                                        Ok(drain_response) => {
                                            // Notify runtime that draining is complete
                                            let complete_request = NotifyDrainCompleteRequest {
                                                application_id: app_id.clone(),
                                                tasks_drained: drain_response.tasks_drained,
                                                success: drain_response.success,
                                                error_message: drain_response.error_message,
                                            };

                                            if let Err(e) =
                                                client.notify_drain_complete(complete_request).await
                                            {
                                                error!(app_id = %app_id, error = %e, "Failed to notify runtime of drain completion");
                                            } else {
                                                info!(app_id = %app_id, "Successfully notified runtime of drain completion");
                                            }
                                        }
                                        Err(e) => {
                                            error!(app_id = %app_id, error = %e, "Failed to drain tasks");
                                            // Still notify runtime of the failure
                                            let complete_request = NotifyDrainCompleteRequest {
                                                application_id: app_id.clone(),
                                                tasks_drained: 0,
                                                success: false,
                                                error_message: Some(e.to_string()),
                                            };
                                            let _ = client
                                                .notify_drain_complete(complete_request)
                                                .await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(app_id = %app_id, error = %e, "Failed to wait for drain signal, retrying in 5 seconds");
                                tokio::time::sleep(Duration::from_secs(5)).await;
                            }
                        }
                    }
                });
            }
        }

        Ok(())
    }

    /// Creates a clone suitable for background tasks.
    fn clone_for_background(&self) -> Self {
        Self {
            application_id: self.application_id.clone(),
            tasks: Arc::clone(&self.tasks),
            task_counter: AtomicU32::new(self.task_counter.load(Ordering::Relaxed)),
            #[cfg(feature = "grpc")]
            grpc_client: self.grpc_client.clone(),
            draining: Arc::clone(&self.draining),
        }
    }

    /// Notifies the runtime that a task has been registered (if gRPC client is available).
    async fn notify_task_registered(&self, task_description: String) -> Result<()> {
        #[cfg(feature = "grpc")]
        {
            if let Some(mut client) = self.grpc_client.clone() {
                let request = NotifyTaskRegisteredRequest {
                    application_id: self.application_id.clone(),
                    task_description: Some(task_description),
                };

                client
                    .notify_task_registered(request)
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        url: "grpc://wait_until_service".to_string(),
                        method: "notify_task_registered".to_string(),
                    })?;
            }
        }

        Ok(())
    }
}

impl WaitUntilContext {
    /// Registers a new wait_until task that will be executed immediately.
    /// The task runs in the application process but is tracked by the runtime.
    pub fn wait_until<F, Fut>(&self, task_fn: F) -> Result<()>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let task_key = format!("task_{}", task_id);
        let task_description = format!("wait_until_task_{}", task_id);

        // Check if we're currently draining - if so, reject new tasks
        let draining = self.draining.clone();
        let tasks = self.tasks.clone();
        let app_id = self.application_id.clone();
        let task_key_clone = task_key.clone();

        // Start the task immediately
        let handle = tokio::spawn(async move {
            // Double-check if we're draining
            if *draining.lock().await {
                warn!(app_id = %app_id, task_id = %task_key_clone, "Rejecting new task - currently draining");
                return;
            }

            debug!(app_id = %app_id, task_id = %task_key_clone, "Starting wait_until task");

            let future = task_fn();
            future.await;

            debug!(app_id = %app_id, task_id = %task_key_clone, "Completed wait_until task");

            // Remove ourselves from the tasks map when done
            tasks.lock().await.remove(&task_key_clone);
        });

        // Store the task handle
        {
            let mut tasks_guard = futures::executor::block_on(self.tasks.lock());
            tasks_guard.insert(task_key.clone(), handle);
        }

        // Notify the runtime in a background task (non-blocking)
        let context_clone = self.clone_for_background();
        tokio::spawn(async move {
            if let Err(e) = context_clone.notify_task_registered(task_description).await {
                warn!(app_id = %context_clone.application_id, task_id = %task_key, error = %e, "Failed to notify runtime of task registration");
            }
        });

        Ok(())
    }
}

impl Binding for WaitUntilContext {}

#[async_trait]
impl WaitUntil for WaitUntilContext {
    async fn wait_for_drain_signal(
        &self,
        timeout_duration: Option<Duration>,
    ) -> Result<DrainConfig> {
        #[cfg(feature = "grpc")]
        {
            if let Some(mut client) = self.grpc_client.clone() {
                let timeout_proto = timeout_duration.map(|d| prost_types::Duration {
                    seconds: d.as_secs() as i64,
                    nanos: d.subsec_nanos() as i32,
                });

                let request = WaitForDrainSignalRequest {
                    application_id: self.application_id.clone(),
                    timeout: timeout_proto,
                };

                let response = client
                    .wait_for_drain_signal(request)
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        url: "grpc://wait_until_service".to_string(),
                        method: "wait_for_drain_signal".to_string(),
                    })?;

                let resp = response.into_inner();
                if resp.should_drain {
                    let drain_timeout = resp
                        .drain_timeout
                        .map(|d| Duration::from_secs(d.seconds as u64))
                        .unwrap_or(Duration::from_secs(10));

                    return Ok(DrainConfig {
                        timeout: drain_timeout,
                        reason: resp.drain_reason,
                    });
                }
            }
        }

        // If no gRPC client or no drain signal, return a default config
        Err(AlienError::new(ErrorData::Other {
            message: "No drain signal received or gRPC client not available".to_string(),
        }))
    }

    async fn drain_all(&self, config: DrainConfig) -> Result<DrainResponse> {
        info!(
            app_id = %self.application_id,
            reason = %config.reason,
            timeout_secs = config.timeout.as_secs(),
            "Starting to drain all wait_until tasks"
        );

        // Mark that we're draining to prevent new tasks
        {
            let mut draining_guard = self.draining.lock().await;
            *draining_guard = true;
        }

        let tasks_to_drain = {
            let mut tasks_guard = self.tasks.lock().await;
            std::mem::take(&mut *tasks_guard) // Take all tasks out of the map
        };

        let task_count = tasks_to_drain.len() as u32;
        info!(app_id = %self.application_id, task_count = task_count, "Draining tasks");

        let mut success = true;
        let mut error_messages = Vec::new();

        // Wait for all tasks to complete or timeout
        let drain_result = timeout(config.timeout, async {
            for (task_id, handle) in tasks_to_drain {
                match handle.await {
                    Ok(_) => {
                        debug!(app_id = %self.application_id, task_id = %task_id, "Task completed successfully");
                    }
                    Err(e) => {
                        warn!(app_id = %self.application_id, task_id = %task_id, error = %e, "Task failed");
                        success = false;
                        error_messages.push(format!("Task {} failed: {}", task_id, e));
                    }
                }
            }
        })
        .await;

        match drain_result {
            Ok(_) => {
                info!(app_id = %self.application_id, "All tasks drained successfully");
            }
            Err(_) => {
                warn!(app_id = %self.application_id, "Drain timeout exceeded");
                success = false;
                error_messages.push("Drain timeout exceeded".to_string());
            }
        }

        // Reset draining flag
        {
            let mut draining_guard = self.draining.lock().await;
            *draining_guard = false;
        }

        let error_message = if error_messages.is_empty() {
            None
        } else {
            Some(error_messages.join("; "))
        };

        Ok(DrainResponse {
            tasks_drained: task_count,
            success,
            error_message,
        })
    }

    async fn get_task_count(&self) -> Result<u32> {
        let tasks_guard = self.tasks.lock().await;
        Ok(tasks_guard.len() as u32)
    }

    async fn notify_drain_complete(&self, response: DrainResponse) -> Result<()> {
        #[cfg(feature = "grpc")]
        {
            if let Some(mut client) = self.grpc_client.clone() {
                let request = NotifyDrainCompleteRequest {
                    application_id: self.application_id.clone(),
                    tasks_drained: response.tasks_drained,
                    success: response.success,
                    error_message: response.error_message,
                };

                client
                    .notify_drain_complete(request)
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        url: "grpc://wait_until_service".to_string(),
                        method: "notify_drain_complete".to_string(),
                    })?;
            }
        }

        Ok(())
    }
}
